# Formal verification: Certora Sunbeam

Contract-level formal verification of the pool, built on the auditor's own
harness rather than a parallel invention.

## Provenance

| | |
|---|---|
| Report | `audits/Script3 - Certora - Blend v2 - Formal Verification Draft v2 Report - June 2025.pdf` |
| Harness | `Certora/blend-contracts-v2@certora`, tip `f53fb33` |
| Verified against | `blend-capital/blend-contracts-v2@996e09e` |
| Work period | 2025-02-03 to 2025-03-13 |
| `cvlr` | `919957b622d8ebb5e2e0430a6df9ea1e470f00d7` (0.3.2) |
| `cvlr-soroban` | `69a49e15c496b695cd15a01bb06d811294d0f954` (0.3.0) |

`996e09e` is a clean ancestor of this fork's `main`, so the harness ports onto
our tree rather than needing a reimplementation.

Both cvlr revisions are pinned. `cvlr-soroban` 0.3.0 requires `cvlr` 0.3 from
git *without* a rev, so it would otherwise resolve against cvlr's moving
default branch (now 0.6.x) and fail; the pinned revisions are recorded in
`Cargo.lock`. Current `cvlr-soroban` requires soroban-sdk 26, which this
workspace does not use.

## Running the rules

Requires a Certora account and `CERTORAKEY`; Sunbeam has no local mode.

```sh
pip3 install certora-cli
export CERTORAKEY=<personal access key>
cd pool                                        # confs resolve relative to here
certoraSorobanProver confs/user_rules.conf
certoraSorobanProver confs/pool_status1.conf   # ... through pool_status4.conf
certoraSorobanProver confs/health.conf
```

Each conf invokes `pool/certora_build.py`, which runs `just build` (building
pool-factory, backstop, and pool for `wasm32-unknown-unknown` with
`--features certora`) and reports the wasm path to the prover. The working
directory must be `pool/`: `build_script` is resolved with `shutil.which`
against the current directory, not against the conf's own directory.

The confs were adjusted for certora-cli 8.x, verified against 8.19.2:

- `"process": "emv"` removed from all six. It is still a known attribute for
  EVM targets but is absent from `SorobanProverAttributes`, so the auditor's
  value now aborts conf parsing. It was the only invalid key; the other ten
  attributes in use are all still valid.
- `build_script` changed from `../certora_build.py` to `./certora_build.py`.
- `pool_status2.conf` and `pool_status4.conf` had trailing commas in
  `prover_args`; all six are now strict JSON.

Each conf has been run locally to the point of job submission, where an
invalid key is correctly rejected.

### Results on this fork

Verified on this fork's `main` with `prover_version: master`, rather than on
`996e09e` with the 2025 prover. Raw `output.json` for each job is retained
under `verification/certora/`.

| Conf | Job | Rules | Result |
|---|---|---|---|
| `user_rules.conf` | `1b57051dc7054db7a57e271ed98d6b13` | 12 | all SUCCESS |
| `pool_status1.conf` | `638ceeac7cf54f02bdd247c4afd5edc7` | 3 | all SUCCESS |
| `pool_status2.conf` | not run | 2 | |
| `pool_status3.conf` | not run | 3 | |
| `pool_status4.conf` | not run | 1 | |
| `health.conf` | not run | 5 | |

15 of the 26 rules the June 2025 report records as Verified have been
reproduced. `pool_status1` covers `verify_update_status_{2,4,6}`;
`verify_update_status_4` is the first inherited rule to exercise an ADR 0008
change, since that commit hoisted the status-4 guard above the backstop read
and out of the `match`. The rule holds, confirming the guard preserves the
observable contract.

Per-rule verdicts come from `output/<userId>/<jobId>/output.json`.
`jobData?attr=rules` and the other detail attributes return `{}` even after a
job succeeds. A job reporting SUCCEEDED means it ran to completion, not that
its rules passed, and the `rule_sanity` sub-results appear only in the web
report's tree view, so neither job above confirms the basic sanity checks
passed.

Monitoring a submitted job needs the `anonymousKey` that the CLI strips from
the URL it prints; it is recoverable from `pool/.certora_internal/`, and
`jobStatus`/`jobData` return 403 without it. Observed states:
QUEUED, RUNNABLE, SUCCEEDED.

## Production impact

The `certora` feature is off by default and the cvlr crates are `optional`, so
a production build resolves the same dependency graph as upstream Blend.
Verified by `cargo tree -p pool -e normal`, which reports no cvlr crates.

Production source changes are limited to:

- `#[cfg(feature = "certora")]` branch pairs in `pool/status.rs` (backstop read
  and the `met_threshold` ghost write), `pool/user.rs` (emissions dispatch),
  and `submit.rs` (the flash-loan transfer and receiver call);
- `nondet_expr!` wrappers, which expand to the original expression without the
  feature;
- `apply_summary!` wrappers, which emit the unmodified function without the
  feature;
- gated `Nondet` impls for `Positions`, `User`, `PoolConfig`, `ReserveConfig`,
  `ReserveData`, `ReserveEmissionData`, `Reserve`, `Request`, `FlashLoan`,
  `Actions`, and `AuctionData`;
- a gated `PoolEvents` impl in `events.rs`, replaced by no-op summaries;
- visibility widening: `pool::{actions, submit}` to `pub(crate)`, the three
  `auctions` submodules to `pub`, `set_user_emissions` to `pub`.

### Deviations from the auditor's harness

Each one reduces production impact or tracks drift since `996e09e`.

- **Optional dependencies.** The auditor's tree declares the cvlr crates
  unconditionally. Making them optional required replacing `cfg!()` in
  `nondet_expr!` with `#[cfg]` on `let` bindings, and adding a fallback
  `apply_summary!` in `spec/mod.rs` for builds where the macro crate is absent.
  The fallback emits exactly what the upstream macro emits with the feature
  off, so production bodies are never duplicated.
- **`nondet_expr!` and unit values.** The `#[cfg]` form cannot infer `()`, so
  the two void calls in `execute_submit_with_flash_loan` use plain `#[cfg]`
  statements. Skipping a unit-returning call and havocking it are equivalent.
- **`positions_hf_under` hook point.** `main` centralized the health check in
  `validate_submit`, which both entrypoints reach, so the summary needs one
  hook where the auditor's base tree needed two. The extracted function also
  carries the `MinCollateralNotMet` rejection, which is therefore elided along
  with the health check when summarized; the summary over-approximates, which
  is the sound direction for this safety property.
- **`emissions` re-export.** The auditor's tree replaces the
  `update_emissions` re-export with `set_user_emissions`. Both are exported
  here so production callers are unaffected.
- **Struct drift.** `ReserveConfig.collateral_cap` is `supply_cap` on `main`;
  `PoolBackstopData` gained `shares` and `token_spot_price`; `Actions` gained
  `check_max_util`. The summaries and `Nondet` impls were updated to match,
  with field sets checked against the definitions.
- **Event drift.** `main` renamed `delete_liquidation_auction` to
  `delete_auction`, added `collateral_orphaned`, `debt_setoff`, and
  `orphan_settled` (ADR 0011), and `gulp` takes three arguments rather than
  four. The no-op summaries were aligned.
- **`update_b_emissions` supply argument.** The auditor's tree passes
  `d_supply` for both emission updates; `main` correctly passes `b_supply` for
  the b-token side, and that is preserved.

## Scope and limits

What these rules do **not** establish:

- **Pool only.** No backstop, pool-factory, or emitter rules. The backstop was
  Code4rena contest scope and is excluded from the June 2025 report; the
  auditor's repository contains no backstop specs.
- **Summaries are unproven over-approximations.** The report states this
  directly. `events` (no-ops), `emissions`, `auction::{fill, delete}`, and the
  `actions` family are modeled, and only `build_actions_from_request`,
  `handle_transfers`, and `handle_transfer_with_allowance` have soundness
  rules. Our ADR 0011 bad-debt and trap changes touch the summarized auction
  region.
- **Loops unrolled at most twice**, with `optimistic_loop: true`. Nothing is
  established for more than two reserves or requests in a single call.
- Authorization, the Soroban host and wasm runtime, ledger atomicity, and
  deployment behaviour are all out of scope.

## Fork-delta rules still to write

The inherited rules cover upstream Blend's behaviour. These fork changes have
no rules yet:

- `execute_initialize` rejecting a nonzero `bstop_rate` (ADR 0008).
- The reserve retirement and disable-only transitions in `execute_queue_set_reserve`
  and `execute_set_reserve`, including the status-6 versus live-pool timelock split.
- `execute_set_pool_status(4)` taking an early return that bypasses the
  backstop read, and the hoisted status-4 guard in `execute_update_pool_status`.
- The ADR 0011 bad-debt set-off and backstop trap paths.
