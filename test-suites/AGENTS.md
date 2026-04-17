# TEST SUITES KNOWLEDGE BASE

## OVERVIEW
`test-suites/` is the integration and migration test hub for Blend V2. It provides reusable fixtures, storage readers, snapshot-backed migration tests, and a sibling fuzz workspace.

## STRUCTURE
```text
test-suites/
├── src/test_fixture.rs   # canonical full-environment harness
├── src/setup.rs          # populated fixture helper used by many tests
├── src/snapshot.rs       # mainnet snapshot constants + loader
├── src/assertions.rs     # approximate fixed-point assertions
├── tests/                # integration scenarios named `test_*.rs`
└── fuzz/                 # cargo-fuzz workspace and invariants target
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Full protocol harness | `src/test_fixture.rs` | deploys tokens, oracle, emitter, backstop, pool factory, pools |
| Pre-seeded scenario fixture | `src/setup.rs` | `create_fixture_with_data(false|true)` is the usual starting point |
| Approximate math assertions | `src/assertions.rs` | use these instead of ad hoc epsilon logic |
| Migration / snapshot testing | `src/snapshot.rs`, `src/mainnet-55261759-snapshot.json` | partial mainnet ledger snapshot |
| Integration scenarios | `tests/test_*.rs` | central cross-crate behavior coverage |
| Fuzz invariants | `fuzz/fuzz_targets/fuzz_pool_general.rs` | command sequences + `assert_invariants()` |

## CONVENTIONS
- `TestFixture::create()` builds the whole protocol surface, not just one contract.
- `TokenIndex` is the canonical token selector; avoid magic integer indexing in new tests.
- Use `jump()` when only timestamp matters and `jump_with_sequence()` when ledger sequence progression matters.
- Read helpers like `read_pool_config`, `read_reserve_data`, and `read_reserve_emissions` are the preferred way to inspect storage-backed state.
- Integration tests live in `tests/`; crate-local unit tests mostly stay in contract crates.

## ANTI-PATTERNS
- Do not create bespoke fixture bootstrapping if `create_fixture_with_data()` or `TestFixture::create()` already gives the needed environment.
- Do not use plain equality for fixed-point behavior when protocol rounding is expected; use the local approximate helpers.
- Do not treat fuzz panics as normal contract failures; the fuzz harness explicitly flags unexpected `panic!` paths and expects `panic_with_error!` semantics.

## NOTES
- `src/lib.rs` intentionally has `#![allow(clippy::all)]`; keep child test code readable, but do not cargo-cult that into production crates.
- `fuzz/` is a separate `cargo-fuzz` workspace on nightly and currently focuses on pool command-sequence invariants.
- Snapshot tests depend on a partial mainnet ledger file and are the non-obvious place to look for v1-to-v2 migration behavior.
