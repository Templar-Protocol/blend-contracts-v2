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

`user_rules.conf` — job `4040469/1b57051dc7054db7a57e271ed98d6b13`, 2026-10-04,
prover_version `master`, status SUCCEEDED. All 12 user-integrity rules
verified:

```
add_collateral_increases_b_supply                SUCCESS
add_collateral_increases_position_collateral     SUCCESS
add_liabilities_increases_dsupply                SUCCESS
add_liabilities_increases_liabilities            SUCCESS
add_supply_increases_b_supply                    SUCCESS
add_supply_increases_position_supply             SUCCESS
remove_collateral_decreases_b_supply             SUCCESS
remove_collateral_decreases_position_collateral  SUCCESS
remove_liabilities_decreases_dsupply             SUCCESS
remove_liabilities_decreases_position_collateral SUCCESS
remove_liabilities_decreases_liabilities         SUCCESS
remove_supply_decreases_b_supply                 SUCCESS
```

This reproduces the auditor's twelve Verified results against this fork's
`main` rather than against `996e09e`, on a current prover.

The remaining five confs have not been run. Per-rule verdicts come from
`output/<userId>/<jobId>/output.json`; `jobData?attr=rules` stays empty, and
the `rule_sanity` sub-results appear only in the web report's tree view, so a
job reporting SUCCESS does not by itself confirm the basic sanity checks
passed.

`confs/health.conf` carries `"server": "prover"` where the other five use
`"server": "production"`. The CLI accepts both, so it is left as the auditor
had it; whether the backend honours `prover` is untested. If a run rejects it,
`production` is the value the rest of the suite uses.

To check the harness compiles without a key:

```sh
cargo check -p pool --features certora
cd pool && just build
```

## Rules

31 rules, of which the 26 below are the ones the June 2025 report records as
Verified. All 31 are present in the built wasm.

**User health** (`spec/health_rules.rs`, `confs/health.conf`) — after
`execute_submit` and `execute_submit_with_flash_loan`, either the health check
ran or the user's positions moved in a provably safe direction, skolemized over
the reserve index. With soundness rules for the `build_actions_from_request`,
`handle_transfers`, and `handle_transfer_with_allowance` summaries.

**Pool status** (`spec/pool_status_rules.rs`, `confs/pool_status{1..4}.conf`) —
the nine state-machine rules: status 6 and 4 always panic; 2 goes to 5 at
q4w >= 75%; 0 goes to 3 when the threshold is unmet or q4w >= 50% and stays at
0 otherwise; other statuses go to 5 at q4w >= 60%, to 3 at q4w >= 30% or unmet
threshold, else to 1; and the result is always in {0,1,2,3,5}.

**User integrity** (`spec/user_rules.rs`, `confs/user_rules.conf`) — twelve
rules over `User::{add,remove}_{liabilities,collateral,supply}`, each checking
both the position delta and that the paired supply moves while the other is
untouched.

**Beyond the report** — four `cvlr_satisfy!` vacuity rules, plus
`target_util_should_be_less_than_0_9500000`, which the auditor wrote as a
counter-example demonstration: `calc_accrual` takes the wrong branch when
`config.util > 0.95`. On this fork `require_valid_reserve_metadata` bounds
`util > 0_9000000`, so it is unreachable through validated config and the rule
stands as a regression guard. Every conf also sets `rule_sanity: "basic"`.

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
