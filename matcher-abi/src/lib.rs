//! # Percolator Matcher ABI
//!
//! Shared ABI types for the Percolator CPI matcher protocol.
//!
//! The Percolator risk engine calls matcher programs via CPI to execute trades.
//! This crate defines the call/return wire format so that both the risk engine
//! and matcher implementations agree on the binary layout.
//!
//! ## Wire Format
//!
//! ### Call (67 bytes, sent as instruction data)
//! | Offset | Size | Field           | Description                          |
//! |--------|------|-----------------|--------------------------------------|
//! | 0      | 1    | `tag`           | Always `0x00` (matcher call tag)     |
//! | 1      | 8    | `req_id`        | Request nonce (must be echoed back)  |
//! | 9      | 2    | `lp_idx`        | LP index in the slab                 |
//! | 11     | 8    | `lp_account_id` | Unique LP account ID (never recycled)|
//! | 19     | 8    | `oracle_price`  | Oracle price × 10⁶ (must be echoed)  |
//! | 27     | 16   | `req_size`      | Requested size (i128, signed)        |
//! | 43     | 24   | `reserved`      | Must be zero                         |
//!
//! ### Return (64-byte prefix written to matcher context account)
//! | Offset | Size | Field           | Description                          |
//! |--------|------|-----------------|--------------------------------------|
//! | 0      | 4    | `abi_version`   | Must equal `ABI_VERSION` (1)         |
//! | 4      | 4    | `flags`         | Bitfield: VALID, PARTIAL_OK, REJECTED|
//! | 8      | 8    | `exec_price`    | Execution price × 10⁶               |
//! | 16     | 16   | `exec_size`     | Actual fill size (i128, signed)      |
//! | 32     | 8    | `req_id`        | Echoed request nonce                 |
//! | 40     | 8    | `lp_account_id` | Echoed LP account ID                 |
//! | 48     | 8    | `oracle_price`  | Echoed oracle price × 10⁶           |
//! | 56     | 8    | `reserved`      | Must be zero                         |

#![no_std]

// ── Constants ────────────────────────────────────────────────────────────────

/// Current ABI version. Risk engine rejects mismatches.
pub const ABI_VERSION: u32 = 1;

/// Matcher call tag byte (instruction data byte 0).
pub const CALL_TAG: u8 = 0;

/// Total length of a matcher call instruction data.
pub const CALL_LEN: usize = 67;

/// Minimum length of the matcher context account (return data prefix).
pub const RETURN_PREFIX_LEN: usize = 64;

/// Required minimum context account length.
pub const CONTEXT_LEN: usize = 320;

// ── Call ABI offsets ─────────────────────────────────────────────────────────

pub const CALL_OFF_TAG: usize = 0;
pub const CALL_OFF_REQ_ID: usize = 1;
pub const CALL_OFF_LP_IDX: usize = 9;
pub const CALL_OFF_LP_ACCOUNT_ID: usize = 11;
pub const CALL_OFF_ORACLE_PRICE: usize = 19;
pub const CALL_OFF_REQ_SIZE: usize = 27;
pub const CALL_OFF_PADDING: usize = 43;

// ── Return ABI offsets ───────────────────────────────────────────────────────

pub const RET_OFF_ABI_VERSION: usize = 0;
pub const RET_OFF_FLAGS: usize = 4;
pub const RET_OFF_EXEC_PRICE: usize = 8;
pub const RET_OFF_EXEC_SIZE: usize = 16;
pub const RET_OFF_REQ_ID: usize = 32;
pub const RET_OFF_LP_ACCOUNT_ID: usize = 40;
pub const RET_OFF_ORACLE_PRICE: usize = 48;
pub const RET_OFF_RESERVED: usize = 56;

// ── Flags ────────────────────────────────────────────────────────────────────

/// Bit 0: response is valid (must always be set for acceptance).
pub const FLAG_VALID: u32 = 1;
/// Bit 1: partial fill (including zero-fill) is allowed.
pub const FLAG_PARTIAL_OK: u32 = 2;
/// Bit 2: trade rejected by matcher.
pub const FLAG_REJECTED: u32 = 4;

// ── Request (decoded call) ───────────────────────────────────────────────────

/// Decoded matcher call request. Parsed from CPI instruction data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatcherRequest {
    pub req_id: u64,
    pub lp_idx: u16,
    pub lp_account_id: u64,
    pub oracle_price_e6: u64,
    pub req_size: i128,
}

impl MatcherRequest {
    /// Parse a matcher call from raw instruction data.
    ///
    /// Returns `None` if the data is too short or the tag byte is wrong.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < CALL_LEN {
            return None;
        }
        if data[CALL_OFF_TAG] != CALL_TAG {
            return None;
        }

        let req_id =
            u64::from_le_bytes(data[CALL_OFF_REQ_ID..CALL_OFF_REQ_ID + 8].try_into().ok()?);
        let lp_idx =
            u16::from_le_bytes(data[CALL_OFF_LP_IDX..CALL_OFF_LP_IDX + 2].try_into().ok()?);
        let lp_account_id = u64::from_le_bytes(
            data[CALL_OFF_LP_ACCOUNT_ID..CALL_OFF_LP_ACCOUNT_ID + 8]
                .try_into()
                .ok()?,
        );
        let oracle_price_e6 = u64::from_le_bytes(
            data[CALL_OFF_ORACLE_PRICE..CALL_OFF_ORACLE_PRICE + 8]
                .try_into()
                .ok()?,
        );
        let req_size = i128::from_le_bytes(
            data[CALL_OFF_REQ_SIZE..CALL_OFF_REQ_SIZE + 16]
                .try_into()
                .ok()?,
        );

        // Check padding is zero
        for &b in &data[CALL_OFF_PADDING..CALL_LEN] {
            if b != 0 {
                return None;
            }
        }

        Some(Self {
            req_id,
            lp_idx,
            lp_account_id,
            oracle_price_e6,
            req_size,
        })
    }
}

// ── Response ─────────────────────────────────────────────────────────────────

/// Matcher return response. Written to the matcher context account.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatcherReturn {
    pub abi_version: u32,
    pub flags: u32,
    pub exec_price_e6: u64,
    pub exec_size: i128,
    pub req_id: u64,
    pub lp_account_id: u64,
    pub oracle_price_e6: u64,
    pub reserved: u64,
}

impl MatcherReturn {
    /// Read a matcher return from the first 64 bytes of an account's data.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < RETURN_PREFIX_LEN {
            return None;
        }
        Some(Self {
            abi_version: u32::from_le_bytes(
                data[RET_OFF_ABI_VERSION..RET_OFF_ABI_VERSION + 4]
                    .try_into()
                    .ok()?,
            ),
            flags: u32::from_le_bytes(data[RET_OFF_FLAGS..RET_OFF_FLAGS + 4].try_into().ok()?),
            exec_price_e6: u64::from_le_bytes(
                data[RET_OFF_EXEC_PRICE..RET_OFF_EXEC_PRICE + 8]
                    .try_into()
                    .ok()?,
            ),
            exec_size: i128::from_le_bytes(
                data[RET_OFF_EXEC_SIZE..RET_OFF_EXEC_SIZE + 16]
                    .try_into()
                    .ok()?,
            ),
            req_id: u64::from_le_bytes(data[RET_OFF_REQ_ID..RET_OFF_REQ_ID + 8].try_into().ok()?),
            lp_account_id: u64::from_le_bytes(
                data[RET_OFF_LP_ACCOUNT_ID..RET_OFF_LP_ACCOUNT_ID + 8]
                    .try_into()
                    .ok()?,
            ),
            oracle_price_e6: u64::from_le_bytes(
                data[RET_OFF_ORACLE_PRICE..RET_OFF_ORACLE_PRICE + 8]
                    .try_into()
                    .ok()?,
            ),
            reserved: u64::from_le_bytes(
                data[RET_OFF_RESERVED..RET_OFF_RESERVED + 8]
                    .try_into()
                    .ok()?,
            ),
        })
    }

    /// Serialize this return into the first 64 bytes of a mutable buffer.
    ///
    /// Returns `false` if the buffer is too small.
    pub fn write_to(&self, buf: &mut [u8]) -> bool {
        if buf.len() < RETURN_PREFIX_LEN {
            return false;
        }
        buf[RET_OFF_ABI_VERSION..RET_OFF_ABI_VERSION + 4]
            .copy_from_slice(&self.abi_version.to_le_bytes());
        buf[RET_OFF_FLAGS..RET_OFF_FLAGS + 4].copy_from_slice(&self.flags.to_le_bytes());
        buf[RET_OFF_EXEC_PRICE..RET_OFF_EXEC_PRICE + 8]
            .copy_from_slice(&self.exec_price_e6.to_le_bytes());
        buf[RET_OFF_EXEC_SIZE..RET_OFF_EXEC_SIZE + 16]
            .copy_from_slice(&self.exec_size.to_le_bytes());
        buf[RET_OFF_REQ_ID..RET_OFF_REQ_ID + 8].copy_from_slice(&self.req_id.to_le_bytes());
        buf[RET_OFF_LP_ACCOUNT_ID..RET_OFF_LP_ACCOUNT_ID + 8]
            .copy_from_slice(&self.lp_account_id.to_le_bytes());
        buf[RET_OFF_ORACLE_PRICE..RET_OFF_ORACLE_PRICE + 8]
            .copy_from_slice(&self.oracle_price_e6.to_le_bytes());
        buf[RET_OFF_RESERVED..RET_OFF_RESERVED + 8].copy_from_slice(&self.reserved.to_le_bytes());
        true
    }

    /// Create a valid acceptance response that echoes request fields and fills at oracle price.
    pub fn accept(req: &MatcherRequest, exec_price_e6: u64, exec_size: i128) -> Self {
        Self {
            abi_version: ABI_VERSION,
            flags: FLAG_VALID,
            exec_price_e6,
            exec_size,
            req_id: req.req_id,
            lp_account_id: req.lp_account_id,
            oracle_price_e6: req.oracle_price_e6,
            reserved: 0,
        }
    }

    /// Create a valid partial-fill (or zero-fill) response.
    pub fn partial(req: &MatcherRequest, exec_price_e6: u64, exec_size: i128) -> Self {
        Self {
            abi_version: ABI_VERSION,
            flags: FLAG_VALID | FLAG_PARTIAL_OK,
            exec_price_e6,
            exec_size,
            req_id: req.req_id,
            lp_account_id: req.lp_account_id,
            oracle_price_e6: req.oracle_price_e6,
            reserved: 0,
        }
    }

    /// Create a rejection response.
    pub fn reject(req: &MatcherRequest) -> Self {
        Self {
            abi_version: ABI_VERSION,
            flags: FLAG_REJECTED,
            exec_price_e6: 0,
            exec_size: 0,
            req_id: req.req_id,
            lp_account_id: req.lp_account_id,
            oracle_price_e6: req.oracle_price_e6,
            reserved: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_return() {
        let ret = MatcherReturn {
            abi_version: ABI_VERSION,
            flags: FLAG_VALID | FLAG_PARTIAL_OK,
            exec_price_e6: 50_000_000,
            exec_size: -100_000,
            req_id: 42,
            lp_account_id: 7,
            oracle_price_e6: 50_100_000,
            reserved: 0,
        };
        let mut buf = [0u8; 320];
        assert!(ret.write_to(&mut buf));
        let decoded = MatcherReturn::from_bytes(&buf).unwrap();
        assert_eq!(ret, decoded);
    }

    #[test]
    fn test_roundtrip_request() {
        let req = MatcherRequest {
            req_id: 123,
            lp_idx: 5,
            lp_account_id: 42,
            oracle_price_e6: 60_000_000,
            req_size: 500_000,
        };
        // Build raw call bytes
        let mut data = [0u8; CALL_LEN];
        data[CALL_OFF_TAG] = CALL_TAG;
        data[CALL_OFF_REQ_ID..CALL_OFF_REQ_ID + 8].copy_from_slice(&req.req_id.to_le_bytes());
        data[CALL_OFF_LP_IDX..CALL_OFF_LP_IDX + 2].copy_from_slice(&req.lp_idx.to_le_bytes());
        data[CALL_OFF_LP_ACCOUNT_ID..CALL_OFF_LP_ACCOUNT_ID + 8]
            .copy_from_slice(&req.lp_account_id.to_le_bytes());
        data[CALL_OFF_ORACLE_PRICE..CALL_OFF_ORACLE_PRICE + 8]
            .copy_from_slice(&req.oracle_price_e6.to_le_bytes());
        data[CALL_OFF_REQ_SIZE..CALL_OFF_REQ_SIZE + 16]
            .copy_from_slice(&req.req_size.to_le_bytes());
        let decoded = MatcherRequest::from_bytes(&data).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn test_request_rejects_wrong_tag() {
        let mut data = [0u8; CALL_LEN];
        data[0] = 1; // wrong tag
        assert!(MatcherRequest::from_bytes(&data).is_none());
    }

    #[test]
    fn test_request_rejects_short_data() {
        let data = [0u8; 10];
        assert!(MatcherRequest::from_bytes(&data).is_none());
    }

    #[test]
    fn test_return_rejects_short_buffer() {
        let data = [0u8; 32];
        assert!(MatcherReturn::from_bytes(&data).is_none());
    }

    #[test]
    fn test_write_rejects_short_buffer() {
        let ret = MatcherReturn::accept(
            &MatcherRequest {
                req_id: 1,
                lp_idx: 0,
                lp_account_id: 1,
                oracle_price_e6: 1_000_000,
                req_size: 100,
            },
            1_000_000,
            100,
        );
        let mut buf = [0u8; 32];
        assert!(!ret.write_to(&mut buf));
    }

    #[test]
    fn test_accept_response() {
        let req = MatcherRequest {
            req_id: 99,
            lp_idx: 3,
            lp_account_id: 55,
            oracle_price_e6: 45_000_000,
            req_size: 1_000_000,
        };
        let ret = MatcherReturn::accept(&req, 45_050_000, 1_000_000);
        assert_eq!(ret.abi_version, ABI_VERSION);
        assert_eq!(ret.flags, FLAG_VALID);
        assert_eq!(ret.exec_price_e6, 45_050_000);
        assert_eq!(ret.exec_size, 1_000_000);
        assert_eq!(ret.req_id, 99);
        assert_eq!(ret.lp_account_id, 55);
        assert_eq!(ret.oracle_price_e6, 45_000_000);
        assert_eq!(ret.reserved, 0);
    }

    #[test]
    fn test_partial_response() {
        let req = MatcherRequest {
            req_id: 1,
            lp_idx: 0,
            lp_account_id: 1,
            oracle_price_e6: 1_000_000,
            req_size: 1_000,
        };
        let ret = MatcherReturn::partial(&req, 1_000_000, 500);
        assert_eq!(ret.flags, FLAG_VALID | FLAG_PARTIAL_OK);
        assert_eq!(ret.exec_size, 500);
    }

    #[test]
    fn test_reject_response() {
        let req = MatcherRequest {
            req_id: 1,
            lp_idx: 0,
            lp_account_id: 1,
            oracle_price_e6: 1_000_000,
            req_size: 1_000,
        };
        let ret = MatcherReturn::reject(&req);
        assert_eq!(ret.flags, FLAG_REJECTED);
        assert_eq!(ret.exec_price_e6, 0);
        assert_eq!(ret.exec_size, 0);
    }

    #[test]
    fn test_request_rejects_nonzero_padding() {
        let mut data = [0u8; CALL_LEN];
        data[CALL_OFF_TAG] = CALL_TAG;
        data[CALL_OFF_PADDING + 5] = 0xFF; // non-zero padding
        assert!(MatcherRequest::from_bytes(&data).is_none());
    }
}
