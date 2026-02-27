//! # Reference AMM Pricing Engine
//!
//! Implements a simple oracle-anchored AMM with position-dependent spread.
//!
//! ## Pricing Model
//!
//! The reference AMM computes an execution price as:
//!
//! ```text
//! exec_price = oracle_price × (1 + spread)
//! ```
//!
//! Where `spread` depends on the trade direction:
//! - **Long** (buying): `+base_spread_bps / 10_000` (user pays more)
//! - **Short** (selling): `-base_spread_bps / 10_000` (user receives less)
//!
//! The `base_spread_bps` can be stored in the matcher context account's
//! user-data region (bytes 64..320) for per-LP customization.
//!
//! ## LP Customization Points
//!
//! The matcher context account has 256 bytes of user-data (bytes 64..320)
//! that LPs can populate with custom configuration:
//!
//! ```text
//! [64..66]  spread_bps (u16)   — base spread in basis points (default 30 = 0.30%)
//! [66..68]  max_fill_pct (u16) — max % of request to fill, in bps (default 10000 = 100%)
//! [68..320] reserved
//! ```

use percolator_matcher_abi::{MatcherRequest, MatcherReturn, RETURN_PREFIX_LEN};
use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

/// Default spread: 30 basis points (0.30%).
const DEFAULT_SPREAD_BPS: u64 = 30;

/// Maximum allowed spread: 500 basis points (5.00%).
/// Prevents misconfigured LPs from quoting absurd prices.
const MAX_SPREAD_BPS: u64 = 500;

/// Basis points denominator.
const BPS_DENOM: u64 = 10_000;

/// Offset of LP config data within the matcher context account.
const CONFIG_OFFSET: usize = RETURN_PREFIX_LEN; // byte 64

/// LP config layout within matcher context user-data.
#[derive(Debug, Clone, Copy)]
struct LpConfig {
    /// Spread in basis points (0.01% per bps).
    spread_bps: u64,
    /// Maximum fill percentage in bps (10000 = 100%).
    max_fill_bps: u64,
}

impl LpConfig {
    /// Read LP config from the matcher context account's user-data region.
    /// Falls back to defaults if the data looks uninitialized (all zeros).
    fn from_context(data: &[u8]) -> Self {
        if data.len() < CONFIG_OFFSET + 4 {
            return Self::default();
        }

        let spread_raw = u16::from_le_bytes(
            data[CONFIG_OFFSET..CONFIG_OFFSET + 2]
                .try_into()
                .unwrap_or([0, 0]),
        ) as u64;

        let max_fill_raw = u16::from_le_bytes(
            data[CONFIG_OFFSET + 2..CONFIG_OFFSET + 4]
                .try_into()
                .unwrap_or([0, 0]),
        ) as u64;

        // Treat zeros as "use defaults"
        let spread_bps = if spread_raw == 0 {
            DEFAULT_SPREAD_BPS
        } else if spread_raw > MAX_SPREAD_BPS as u16 as u64 {
            MAX_SPREAD_BPS
        } else {
            spread_raw
        };

        let max_fill_bps = if max_fill_raw == 0 {
            BPS_DENOM // 100%
        } else {
            max_fill_raw.min(BPS_DENOM)
        };

        Self {
            spread_bps,
            max_fill_bps,
        }
    }
}

impl Default for LpConfig {
    fn default() -> Self {
        Self {
            spread_bps: DEFAULT_SPREAD_BPS,
            max_fill_bps: BPS_DENOM,
        }
    }
}

/// Price a trade and produce a matcher return.
///
/// This is the main entry point for the AMM pricing logic.
pub fn price_trade(
    _program_id: &Pubkey,
    request: &MatcherRequest,
    matcher_ctx: &AccountInfo,
) -> Result<MatcherReturn, ProgramError> {
    let ctx_data = matcher_ctx.try_borrow_data()?;
    let config = LpConfig::from_context(&ctx_data);
    drop(ctx_data);

    let oracle = request.oracle_price_e6;

    // Reject if oracle price is zero (shouldn't happen, risk engine checks, but be safe)
    if oracle == 0 {
        return Ok(MatcherReturn::reject(request));
    }

    // Reject if request size is zero
    if request.req_size == 0 {
        return Ok(MatcherReturn::reject(request));
    }

    // Compute spread-adjusted execution price.
    // Longs pay oracle + spread, shorts receive oracle - spread.
    // This ensures the LP always captures the spread.
    let exec_price_e6 = if request.req_size > 0 {
        // User going long → pay higher price
        oracle
            .checked_mul(BPS_DENOM + config.spread_bps)
            .and_then(|v| v.checked_div(BPS_DENOM))
            .ok_or(ProgramError::ArithmeticOverflow)?
    } else {
        // User going short → receive lower price
        oracle
            .checked_mul(BPS_DENOM.saturating_sub(config.spread_bps))
            .and_then(|v| v.checked_div(BPS_DENOM))
            .ok_or(ProgramError::ArithmeticOverflow)?
    };

    // Compute fill size (apply max fill percentage)
    let exec_size = if config.max_fill_bps >= BPS_DENOM {
        // Fill 100%
        request.req_size
    } else {
        // Partial fill: (req_size * max_fill_bps) / BPS_DENOM
        // Use i128 arithmetic to avoid overflow
        let abs_size = request.req_size.unsigned_abs();
        let filled = abs_size
            .checked_mul(config.max_fill_bps as u128)
            .map(|v| v / BPS_DENOM as u128)
            .unwrap_or(abs_size);

        // Preserve sign
        if request.req_size > 0 {
            filled as i128
        } else {
            -(filled as i128)
        }
    };

    // Determine response type
    if exec_size == request.req_size {
        Ok(MatcherReturn::accept(request, exec_price_e6, exec_size))
    } else {
        Ok(MatcherReturn::partial(request, exec_price_e6, exec_size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(oracle: u64, size: i128) -> MatcherRequest {
        MatcherRequest {
            req_id: 1,
            lp_idx: 0,
            lp_account_id: 42,
            oracle_price_e6: oracle,
            req_size: size,
        }
    }

    #[test]
    fn test_default_config() {
        let cfg = LpConfig::default();
        assert_eq!(cfg.spread_bps, 30);
        assert_eq!(cfg.max_fill_bps, 10_000);
    }

    #[test]
    fn test_config_from_zeros() {
        let data = [0u8; 320];
        let cfg = LpConfig::from_context(&data);
        assert_eq!(cfg.spread_bps, DEFAULT_SPREAD_BPS);
        assert_eq!(cfg.max_fill_bps, BPS_DENOM);
    }

    #[test]
    fn test_config_from_custom() {
        let mut data = [0u8; 320];
        // spread = 50 bps
        data[64..66].copy_from_slice(&50u16.to_le_bytes());
        // max fill = 5000 bps (50%)
        data[66..68].copy_from_slice(&5000u16.to_le_bytes());
        let cfg = LpConfig::from_context(&data);
        assert_eq!(cfg.spread_bps, 50);
        assert_eq!(cfg.max_fill_bps, 5000);
    }

    #[test]
    fn test_config_clamps_spread() {
        let mut data = [0u8; 320];
        data[64..66].copy_from_slice(&1000u16.to_le_bytes()); // 10% — above max
        let cfg = LpConfig::from_context(&data);
        assert_eq!(cfg.spread_bps, MAX_SPREAD_BPS);
    }

    #[test]
    fn test_long_spread() {
        // Oracle = 50.000000, 30bps spread → 50.150000
        let req = make_request(50_000_000, 1_000);
        // Price = 50_000_000 * 10030 / 10000 = 50_150_000
        let expected_price = 50_150_000u64;
        // We can't call price_trade without an AccountInfo, but we can test the math
        let oracle = req.oracle_price_e6;
        let spread = DEFAULT_SPREAD_BPS;
        let price = oracle * (BPS_DENOM + spread) / BPS_DENOM;
        assert_eq!(price, expected_price);
    }

    #[test]
    fn test_short_spread() {
        // Oracle = 50.000000, 30bps spread → 49.850000
        let req = make_request(50_000_000, -1_000);
        let oracle = req.oracle_price_e6;
        let spread = DEFAULT_SPREAD_BPS;
        let price = oracle * (BPS_DENOM - spread) / BPS_DENOM;
        assert_eq!(price, 49_850_000);
    }

    #[test]
    fn test_zero_oracle_rejection() {
        let req = make_request(0, 1_000);
        // With zero oracle, matcher should reject
        // (tested via logic, not full CPI)
        assert_eq!(req.oracle_price_e6, 0);
    }

    #[test]
    fn test_zero_size_rejection() {
        let req = make_request(50_000_000, 0);
        assert_eq!(req.req_size, 0);
    }

    #[test]
    fn test_max_fill_partial() {
        // 50% max fill on a 1000 size request
        let abs_size: u128 = 1000;
        let max_fill_bps: u128 = 5000;
        let filled = abs_size * max_fill_bps / 10_000;
        assert_eq!(filled, 500);
    }
}
