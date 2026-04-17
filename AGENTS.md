# PROJECT KNOWLEDGE BASE

**Generated:** 2026-04-10
**Commit:** ba22b48
**Branch:** main

## OVERVIEW
Blend Protocol V2 is a Soroban smart-contract workspace for permissionless lending pools on Stellar. The core deployable contracts are `pool`, `backstop`, and `pool-factory`; integration and fuzz coverage live under `test-suites`.

## STRUCTURE
```text
blend-contracts-v2/
├── backstop/      # backstop liquidity + reward-zone + emissions contract
├── pool/          # lending pool contract, reserve logic, auctions, emissions
├── pool-factory/  # pool deployment contract
├── mocks/         # contract test doubles and modified ERC3156 example
├── test-suites/   # integration fixtures, assertions, migration snapshot, fuzzing
├── audits/        # PDF audit artifacts
├── Makefile       # canonical build/test/fmt entrypoints
└── rust-toolchain.toml
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Contract entrypoints | `pool/src/contract.rs`, `backstop/src/contract.rs`, `pool-factory/src/pool_factory.rs` | `#[contract]` + `#[contractimpl]` surfaces |
| Workspace members | `Cargo.toml` | Root workspace and shared dependency versions |
| Build and CI flow | `Makefile`, `.github/workflows/pull_request.yml`, `.github/workflows/release.yml` | CI runs `make test`; releases are tag-driven |
| Pool-specific logic | `pool/AGENTS.md` | Auctions, reserve state, status, emissions |
| Backstop-specific logic | `backstop/AGENTS.md` | Deposits, withdrawals, reward zone, emissions |
| Integration and fuzz tests | `test-suites/AGENTS.md` | Fixture setup, snapshot tests, fuzz invariants |
| Mocks and flashloan example | `mocks/*` | Support crates; root guidance is enough |

## CODE MAP
| Symbol | Type | Location | Role |
|--------|------|----------|------|
| `PoolContract` | contract | `pool/src/contract.rs` | lending pool entrypoint |
| `BackstopContract` | contract | `backstop/src/contract.rs` | backstop entrypoint |
| `PoolFactoryContract` | contract | `pool-factory/src/pool_factory.rs` | pool deployment entrypoint |
| `create_fixture_with_data` | test helper | `test-suites/src/setup.rs` | canonical populated integration fixture |
| `TestFixture` | test harness | `test-suites/src/test_fixture.rs` | deploys tokens, oracle, emitter, backstop, pools |

## CONVENTIONS
- Rust toolchain is pinned in `rust-toolchain.toml` to `1.81` with `wasm32-unknown-unknown`, `rustfmt`, `clippy`, and `rust-src`.
- Build artifacts are optimized Soroban WASM binaries produced via `cargo rustc ... --release` followed by `stellar contract optimize`.
- Workspace release profile is intentionally small and safety-oriented: `opt-level = "z"`, `overflow-checks = true`, `panic = "abort"`, `strip = "symbols"`, `lto = true`.
- Contract crates are `cdylib` + `rlib`; tests depend heavily on `feature = "testutils"` and inline `#[cfg(test)]` modules.
- CI only checks formatting plus `make test`; it does **not** run every inline unit test path automatically.

## ANTI-PATTERNS (THIS PROJECT)
- Do not remove `overflow-checks = true` from the root release profile. Both `Cargo.toml` and `README.md` call this out as a vulnerability risk.
- In contract/fuzz code, do not use plain `panic!` for expected failures; the fuzz harness explicitly treats `InvalidAction` panics as suspicious and expects `panic_with_error!` instead.
- Do not assume `emitter/` is an active workspace crate. In this repo it is only a README reference for a v1 emitter release path.

## UNIQUE STYLES
- Contract entry files are intentionally thin orchestration layers over crate-internal modules.
- Cross-contract calls are wrapped through `dependencies/` modules instead of ad hoc client construction.
- Numeric behavior is fixed-point heavy; scalars like `SCALAR_7` / `SCALAR_12` are part of the domain model, not incidental constants.
- Large scenario coverage lives in `test-suites/` rather than per-crate integration test directories.

## COMMANDS
```bash
make
make test
cargo fmt --all
cargo rustc --manifest-path=pool/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release
cargo rustc --manifest-path=backstop/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release
cargo rustc --manifest-path=pool-factory/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release
```

## NOTES
- Release workflow publishes `backstop`, `pool`, `pool-factory`, and an `emitter` package via reusable Stellar Expert workflows.
- `test-suites/fuzz` is its own mini-workspace with a nightly toolchain; treat it separately from the pinned stable workspace toolchain.
- A prebuilt `comet.wasm` exists at repo root and is used by test helpers.
