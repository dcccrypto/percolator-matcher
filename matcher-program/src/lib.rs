//! # Percolator Reference AMM Matcher
//!
//! A reference implementation of a Percolator matcher program that LP operators
//! deploy on Solana. The Percolator risk engine calls this program via CPI to
//! execute trades against an LP's liquidity.
//!
//! ## Architecture
//!
//! ```text
//! Percolator Risk Engine ─── CPI ──► Matcher Program
//!   (validates result)                 (prices trade)
//!         │                                │
//!         │                                ▼
//!         │                          Matcher Context Account
//!         │                          (64-byte return prefix)
//!         │                                │
//!         ◄────────────────────────────────┘
//!         (reads back & validates ABI)
//! ```
//!
//! ## Reference AMM Strategy
//!
//! This implementation uses a constant-product-inspired AMM that applies
//! position-dependent spread around the oracle price:
//!
//! 1. **Oracle-anchored**: Execution price is always within a spread of the oracle.
//! 2. **Position-aware**: Spread widens as LP exposure grows (inventory risk).
//! 3. **Always fills**: Accepts all valid requests at the computed price.
//!
//! LP operators can fork this and implement:
//! - CLOB matching
//! - RFQ-based pricing
//! - Custom AMM curves
//! - Hedging-aware pricing
//! - Circuit breakers / reject logic

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use percolator_matcher_abi::{MatcherRequest, CONTEXT_LEN};
#[cfg(not(feature = "no-entrypoint"))]
use solana_program::entrypoint;

mod amm;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

/// Process a matcher CPI call from the Percolator risk engine.
///
/// # Accounts
///
/// 0. `[signer]` LP PDA — proves the risk engine authorized this call
/// 1. `[writable]` Matcher context account — 320+ bytes, return data written here
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let account_iter = &mut accounts.iter();

    // Account 0: LP PDA (signer — the risk engine signs via invoke_signed)
    let lp_pda = next_account_info(account_iter)?;
    if !lp_pda.is_signer {
        msg!("Error: LP PDA must be signer");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Account 1: Matcher context account (writable, ≥320 bytes)
    let matcher_ctx = next_account_info(account_iter)?;
    if !matcher_ctx.is_writable {
        msg!("Error: Matcher context must be writable");
        return Err(ProgramError::InvalidAccountData);
    }
    {
        let ctx_data = matcher_ctx.try_borrow_data()?;
        if ctx_data.len() < CONTEXT_LEN {
            msg!(
                "Error: Matcher context too small ({} < {})",
                ctx_data.len(),
                CONTEXT_LEN
            );
            return Err(ProgramError::AccountDataTooSmall);
        }
    }

    // Parse the CPI call
    let request = MatcherRequest::from_bytes(instruction_data).ok_or_else(|| {
        msg!("Error: Invalid matcher call format");
        ProgramError::InvalidInstructionData
    })?;

    // Compute execution price and size using the AMM strategy
    let response = amm::price_trade(program_id, &request, matcher_ctx)?;

    // Write response to matcher context account
    {
        let mut ctx_data = matcher_ctx.try_borrow_mut_data()?;
        if !response.write_to(&mut ctx_data) {
            msg!("Error: Failed to write response");
            return Err(ProgramError::AccountDataTooSmall);
        }
    }

    Ok(())
}
