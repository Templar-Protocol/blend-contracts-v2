# POOL CONTRACT KNOWLEDGE BASE

## OVERVIEW
`pool/` contains the core lending market contract: reserve state, user positions, submit flows, liquidation auctions, emissions, and pool status transitions.

## STRUCTURE
```text
pool/
├── src/contract.rs       # public contract API and auth/orchestration layer
├── src/pool/            # core reserve, user, config, status, submit logic
├── src/auctions/        # liquidation, bad debt, and backstop-interest auctions
├── src/emissions/       # reserve emission distribution / management
├── src/dependencies/    # backstop client boundary
├── src/storage.rs       # persistent keys + stored domain types
└── src/testutils.rs     # unit-test-only contract helpers
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Public callable methods | `src/contract.rs` | trait plus `#[contractimpl]` blocks |
| Request execution flow | `src/pool/submit.rs`, `src/pool/actions.rs` | central request dispatch and side effects |
| Reserve configuration/init | `src/pool/config.rs` | queue/set/cancel reserve paths |
| Health / underwater logic | `src/pool/health_factor.rs`, `src/pool/bad_debt.rs` | borrow safety and bad-debt handling |
| Pool status rules | `src/pool/status.rs` | backstop-driven and admin-driven transitions |
| Auction logic | `src/auctions/*.rs` | biggest complexity hotspot in this crate |
| Emissions | `src/emissions/*.rs` | config, distribution, reserve emission state |
| Pool unit-test helpers | `src/testutils.rs` | creates mock oracle, tokens, backstop, flashloan receiver |

## CONVENTIONS
- `src/contract.rs` is mostly auth + event orchestration; substantive logic lives under `src/pool/`, `src/auctions/`, and `src/emissions/`.
- `src/pool/mod.rs` is the best crate-local map of exported internal responsibilities.
- Error handling uses `panic_with_error!` consistently for contract-visible failures.
- Storage-backed domain types are re-exported from `lib.rs`; grep exports before introducing duplicate structs.

## ANTI-PATTERNS
- Do not put new business logic directly into `contract.rs` when a module under `src/pool/`, `src/auctions/`, or `src/emissions/` is the real ownership boundary.
- Do not bypass reserve/status/auction validation helpers with ad hoc state mutation.
- Do not ignore fixed-point scalar handling; `SCALAR_7` and `SCALAR_12` are embedded in reserve math and test helpers.

## NOTES
- The largest files in the repo are concentrated here, especially `src/auctions/*.rs` and `src/pool/submit.rs`; expect high coupling when changing request or liquidation behavior.
- `src/testutils.rs` imports `../comet.wasm` and multiple mock/test contracts; unit tests here assume constructor defaults may need resetting.
