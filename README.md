# Percolator Matcher

Reference implementation of a matcher program for the [Percolator](https://github.com/dcccrypto/percolator-launch) prediction market protocol on Solana.

## What is a Matcher?

The Percolator risk engine delegates trade pricing to external **matcher programs** via Solana CPI. Each LP (liquidity provider) registers a matcher program when they join a market. When a user submits a trade, the risk engine:

1. Calls the LP's matcher program via CPI with the trade request
2. Reads back the execution price and fill size from the matcher context account
3. Validates the response against the ABI spec
4. Applies the trade to the risk engine state (solvency checks, position updates)

This architecture allows LP operators to use any pricing strategy — AMMs, CLOBs, RFQ, or custom algorithms — while the risk engine handles all safety invariants.

## Repository Structure

```
percolator-matcher/
├── matcher-abi/           # Shared CPI ABI types (no_std, no dependencies)
│   └── src/lib.rs         # MatcherRequest, MatcherReturn, wire format
├── matcher-program/       # Reference AMM matcher (Solana BPF program)
│   └── src/
│       ├── lib.rs         # Entrypoint + account validation
│       └── amm.rs         # Reference pricing engine
├── Cargo.toml             # Workspace root
└── README.md
```

## CPI Wire Format

### Call (67 bytes — instruction data)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 1 | `tag` | Always `0x00` |
| 1 | 8 | `req_id` | Request nonce (must echo back) |
| 9 | 2 | `lp_idx` | LP index in the slab |
| 11 | 8 | `lp_account_id` | Unique LP account ID |
| 19 | 8 | `oracle_price_e6` | Oracle price × 10⁶ |
| 27 | 16 | `req_size` | Requested size (i128) |
| 43 | 24 | `reserved` | Must be zero |

### Return (64-byte prefix in matcher context account)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `abi_version` | Must equal `1` |
| 4 | 4 | `flags` | `VALID`, `PARTIAL_OK`, `REJECTED` |
| 8 | 8 | `exec_price_e6` | Execution price × 10⁶ |
| 16 | 16 | `exec_size` | Fill size (i128) |
| 32 | 8 | `req_id` | Echoed nonce |
| 40 | 8 | `lp_account_id` | Echoed LP ID |
| 48 | 8 | `oracle_price_e6` | Echoed oracle price |
| 56 | 8 | `reserved` | Must be zero |

## Reference AMM Strategy

The included reference matcher implements a simple oracle-anchored AMM:

- **Longs** pay `oracle × (1 + spread_bps / 10000)`
- **Shorts** receive `oracle × (1 - spread_bps / 10000)`
- Default spread: 30 bps (0.30%)
- Configurable via the matcher context account's user-data region (bytes 64+)

### LP Configuration (matcher context bytes 64-67)

| Offset | Size | Field | Default |
|--------|------|-------|---------|
| 64 | 2 | `spread_bps` | 30 (0.30%) |
| 66 | 2 | `max_fill_pct` | 10000 (100%) |

## Building

```bash
# Build both crates
cargo build

# Build for Solana BPF deployment
cargo build-sbf --manifest-path matcher-program/Cargo.toml

# Run tests
cargo test
```

## Building Your Own Matcher

1. Fork this repository
2. Replace `matcher-program/src/amm.rs` with your pricing logic
3. The `price_trade()` function receives a `MatcherRequest` and must return a `MatcherReturn`
4. Deploy to Solana and register with an LP on a Percolator market

Key constraints enforced by the risk engine:
- `abi_version` must equal `1`
- `req_id`, `lp_account_id`, and `oracle_price_e6` must be echoed back unchanged
- `exec_size` must not exceed `req_size` in absolute value
- `exec_size` must have the same sign as `req_size`
- `exec_price_e6` must be non-zero for accepted trades
- `reserved` must be zero
- `FLAG_VALID` must be set for acceptance
- Zero-fill requires `FLAG_PARTIAL_OK`

## License

Apache 2.0 — see [LICENSE](LICENSE).
