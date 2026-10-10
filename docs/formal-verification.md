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
| `pool_status2.conf` | `bf0cc6db0e6d4d4dadc69870bba77438` | 2 | all SUCCESS |
| `pool_status3.conf` | `32f65ab6816849ccb720500e915c9f7e` | 3 | all SUCCESS |
| `pool_status4.conf` | `b51c0b2d5b5646788b07af28e0e4f76c` | 1 | all SUCCESS |
| `health.conf` | `184c39409e0a4924936e4e985f89e142` | 5 | 3 SUCCESS, 1 TIMEOUT, 1 UNKNOWN |

24 of the 26 rules the June 2025 report records as Verified have been
reproduced. `health.conf` resolved three of its five:

```
SUCCESS   user_health_execute_submit
SUCCESS   handle_transfers_summary_ok
SUCCESS   handle_transfer_with_allowance_summary_ok
TIMEOUT   user_health_execute_submit_with_flash_loan
UNKNOWN   build_actions_from_request
```

**`user_health_execute_submit` is weaker here than in the report.** It holds
only modulo the `build_actions_from_request` summary, and that summary's
soundness rule returned UNKNOWN rather than the report's Verified. The
auditor's chain had both links; ours has the property but not the link it
rests on. Do not quote the health property for this fork without that
caveat.

### The two unresolved health rules

`user_health_execute_submit_with_flash_loan` and `build_actions_from_request`
are **not reproduced on this fork**. Three attempts:

| Attempt | Budget | `apply_*` summaries wired | Result | Elapsed |
|---|---|---|---|---|
| `184c3940` | 1800s | no | TIMEOUT, UNKNOWN | ~20 min |
| `c5934d7b` | 7200s | no | UNKNOWN, UNKNOWN | ~100 min |
| `68449b7e` | 7200s | yes (all six) | UNKNOWN, UNKNOWN | ~22 min |

Wiring the six `apply_*` summaries the auditor also hooks was a real fix to
this port -- it cut the time to verdict roughly fivefold -- but it did not
change the verdict. Raising the budget fourfold converted TIMEOUT into
UNKNOWN rather than into SUCCESS. Reaching UNKNOWN *faster* with more
summarization points at a wall the solver cannot get past on these two rules,
not at resource exhaustion, so more prover time is not the remedy.

UNKNOWN is not a violation: no counter-example was produced, and nothing here
says the properties are false. They are undecided on main's code with the
auditor's summary boundary.

**Consequence for `user_health_execute_submit`.** It is SUCCESS, but only
modulo the `build_actions_from_request` summary, whose soundness rule is one
of the two UNKNOWN results. The auditor's report has both links Verified;
this fork has the property without the link it rests on. Any statement of the
user-health property for this fork must carry that caveat.

Untried, in rough order of cost, if this is picked up again: split the two
rules into separate jobs so they stop competing for one job's resources; try
`loop_iter: 1` to test whether the difficulty is loop-driven; try a different
solver via `prover_args`; and failing those, treat main's larger
`build_actions_from_request` as having outgrown the auditor's postcondition
and strengthen or re-abstract the summary. The last is spec work, not
configuration.



Two of these results bear on ADR 0008, which hoisted the status-4 guard above
the backstop read and out of the `match` in `execute_update_pool_status`:

- `verify_update_status_4` (pool_status1) holds, so the hoisted guard
  preserves the observable "status 4 always panics" contract.
- `verify_update_status_other_{a,b,c}` (pool_status3) hold. The `_` arm they
  describe now covers statuses {1, 3, 5} rather than {1, 3, 4, 5}, because
  status 4 no longer reaches the match at all. Each rule is conditioned on
  the status not being 0, 2, 4, or 6, so the narrowing leaves them intact.

Per-rule verdicts come from `output/<userId>/<jobId>/output.json`.
`jobData?attr=rules` and the other detail attributes return `{}` even after a
job succeeds. A job reporting SUCCEEDED means it ran to completion, not that
its rules passed, and the `rule_sanity` sub-results appear only in the web
report's tree view, so neither job above confirms the basic sanity checks
passed.

Monitoring a submitted job needs the `anonymousKey` that the CLI strips from
the URL it prints; it is recoverable from `pool/.certora_internal/`, and
`jobStatus`/`jobData` return 403 without it. Observed states:
QUEUED, STARTING, RUNNABLE, RUNNING, SUCCEEDED, CANCELED (one L).
Per-rule verdicts seen: SUCCESS, TIMEOUT, UNKNOWN. A job whose status is
SUCCEEDED can still contain TIMEOUT and UNKNOWN rules, so the job status
is never a substitute for reading output.json.

### health.conf solver cost

The first `health.conf` attempt ran 75 minutes without producing partial
results and was cancelled. The cloud enforces its own global timeout, which
the CLI refuses to let a user set (`validate_cloud_global_timeout` always
raises); Certora documents that ceiling as 7200s, and billing is per
verification-minute, so an unbounded attempt costs the full cap for nothing.

The likely cause is specific to this fork. `validate_submit` runs inside both
health rules' call graph and does work the auditor's tree never had:
`require_under_max` over the positions, a `has_auction` storage read, and a
loop over `check_max_util` calling `load_reserve` and
`require_utilization_below_max`. The auditor's `Actions` carried no
`check_max_util` field at all, so that loop is new verified surface, unrolled
twice under `loop_iter: 2`, and utilization is a division. That the auditor
gave `smt_timeout` and `-splitParallel` to `pool_status2` and `pool_status4`
but not to `health.conf` suggests health was cheap on his tree.

Mitigations applied:

- The `check_max_util` loop is skipped under `certora`. It touches neither the
  positions nor the health check, so it cannot affect the property, and
  dropping its early panic only admits more executions into the assertion.
- `health.conf` gained `global_timeout: 1800`, `smt_timeout: 1200`, and the
  same `prover_args` splitting the auditor used for his two expensive configs.

`pool_status2.conf` and `pool_status4.conf` also carry `global_timeout: 1800`
as a cost guard. Their `smt_timeout` is left at the auditor's 7200, which the
1800 global now makes unreachable; the value is kept so raising the global
restores his configuration exactly. Note he chose 7200 for precisely these two
configs, so 1800 may well not be enough to verify them. Treat the first run of
each as a cheap probe: a timeout costs 30 minutes instead of 120 and tells us
they need the longer budget.

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
