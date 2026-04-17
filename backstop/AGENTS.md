# BACKSTOP CONTRACT KNOWLEDGE BASE

## OVERVIEW
`backstop/` implements the protocol backstop: pool deposits, queued withdrawals, fund management, reward-zone membership, and BLND emissions distribution.

## STRUCTURE
```text
backstop/
├── src/contract.rs        # public contract API and authorization layer
├── src/backstop/         # deposit, withdrawal, pool/user accounting, fund management
├── src/emissions/        # reward-zone and emissions logic
├── src/dependencies/     # pool, pool-factory, comet, emitter clients
├── src/storage.rs        # persistent keys and stored state
└── src/testutils.rs      # test-only helpers
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Public methods | `src/contract.rs` | core, emissions, and fund-management surfaces |
| Deposit / queue / withdraw | `src/backstop/deposit.rs`, `src/backstop/withdrawal.rs` | share accounting and lock behavior |
| Pool/user balances | `src/backstop/pool.rs`, `src/backstop/user.rs` | threshold checks and per-user Q4W state |
| Draw / donate flows | `src/backstop/fund_management.rs` | pool-only fund movement |
| Reward zone + emissions | `src/emissions/manager.rs`, `src/emissions/distributor.rs`, `src/emissions/claim.rs` | backfill, zone membership, claiming |
| External contract boundaries | `src/dependencies/mod.rs` | pool-factory, pool, comet, emitter clients |

## CONVENTIONS
- `contract.rs` is intentionally thin; execution helpers under `src/backstop/` and `src/emissions/` own most state transitions.
- Dependency wrappers are explicit and centralized in `src/dependencies/`; use them instead of recreating clients inline.
- Reward-zone behavior and emissions logic are tightly coupled; read both manager and distributor paths before changing either.

## ANTI-PATTERNS
- Do not treat backstop deposits as generic token transfers; queue and withdrawal semantics are part of the protocol contract.
- Do not bypass pool-factory validation when adding or checking pools.
- Do not hand-wave reward-zone thresholds or bad-debt guards; the crate uses targeted errors for these invariants.

## NOTES
- `BackstopContract::__constructor` enforces a capped drop allocation using `MAX_BACKFILLED_EMISSIONS`; constructor math is part of safety logic.
- This crate depends on an emitter client from `blend_contract_sdk` plus local pool/pool-factory/comet client wrappers.
