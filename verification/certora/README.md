# Certora Sunbeam prover run report

Results of running the ported Certora Sunbeam harness against this fork's
`main`. Setup, provenance and scope limits are in
[`docs/formal-verification.md`](../../docs/formal-verification.md); this file
is the run evidence.

Each `*.json` beside this file is the unmodified `output.json` fetched from
`prover.certora.com/output/<userId>/<jobId>/output.json`.

## Summary

**24 of the 26 rules** the June 2025 Certora report records as Verified are
reproduced on this fork. Two are undecided. No rule was violated and no
counter-example was produced.

| Verdict | Count |
|---|---|
| SUCCESS | 24 |
| UNKNOWN | 2 |
| VIOLATED | 0 |

What that does **not** mean is covered under [Limits](#limits). In particular,
`user_health_execute_submit` is SUCCESS only modulo a summary whose own
soundness rule is one of the two UNKNOWN results.

## Jobs

All jobs ran under `prover_version: master` against this fork's `main`, not
against upstream `996e09e` with the 2025 prover. User id `4040469`.

| Date | Conf | Job | Budget | Outcome |
|---|---|---|---|---|
| 2026-10-04 | `user_rules.conf` | `1b57051d…` | default | 12/12 SUCCESS |
| 2026-10-04 | `pool_status1.conf` | `638ceeac…` | default | 3/3 SUCCESS |
| 2026-10-04 | `pool_status3.conf` | `32f65ab6…` | default | 3/3 SUCCESS |
| 2026-10-05 | `pool_status2.conf` | `bf0cc6db…` | 1800s | 2/2 SUCCESS |
| 2026-10-05 | `pool_status4.conf` | `b51c0b2d…` | 1800s | 1/1 SUCCESS |
| 2026-10-05 | `health.conf` | `184c3940…` | 1800s | 3 SUCCESS, 1 TIMEOUT, 1 UNKNOWN |
| 2026-10-06 | `health_unresolved.conf` | `c5934d7b…` | 7200s | 2 UNKNOWN (~100 min) |
| 2026-10-07 | `health_unresolved.conf` | `68449b7e…` | 7200s | 2 UNKNOWN (~22 min) |

An earlier `health.conf` attempt (`a1fa23b8…`) was cancelled after 75 minutes
before the solver cost was reduced; it produced no report and is not counted.

## Verdicts by rule

Where a rule was run more than once, the latest verdict is shown.

### User integrity — 12/12 SUCCESS

Position and supply bookkeeping on `User::{add,remove}_{liabilities,collateral,supply}`.
Each rule checks both the position delta and that the paired supply moves while
the other is untouched.

```
SUCCESS  add_liabilities_increases_liabilities
SUCCESS  add_liabilities_increases_dsupply
SUCCESS  remove_liabilities_decreases_liabilities
SUCCESS  remove_liabilities_decreases_dsupply
SUCCESS  add_collateral_increases_position_collateral
SUCCESS  add_collateral_increases_b_supply
SUCCESS  remove_collateral_decreases_position_collateral
SUCCESS  remove_collateral_decreases_b_supply
SUCCESS  add_supply_increases_position_supply
SUCCESS  add_supply_increases_b_supply
SUCCESS  remove_supply_decreases_position_collateral
SUCCESS  remove_supply_decreases_b_supply
```

### Pool status state machine — 9/9 SUCCESS

The full state machine from the Blend documentation, over
`execute_update_pool_status`.

```
SUCCESS  verify_update_status_6        status 6 (setup) always panics
SUCCESS  verify_update_status_4        status 4 (admin frozen) always panics
SUCCESS  verify_update_status_2        status 2 -> 5 when q4w >= 75%
SUCCESS  verify_update_status_0_a      status 0 -> 3 when threshold unmet or q4w >= 50%
SUCCESS  verify_update_status_0_b      status 0 stays 0 otherwise
SUCCESS  verify_update_status_other_a  other -> 5 when q4w >= 60%
SUCCESS  verify_update_status_other_b  other -> 3 when q4w >= 30% or threshold unmet
SUCCESS  verify_update_status_other_c  other -> 1 otherwise
SUCCESS  verify_status_update          result always in {0,1,2,3,5}
```

Two of these bear directly on ADR 0008, which hoisted the status-4 guard above
the backstop read and out of the `match`:

- `verify_update_status_4` holds, so the hoisted guard preserves the observable
  "status 4 always panics" contract.
- `verify_update_status_other_{a,b,c}` hold even though the `_` arm they
  describe now covers statuses {1, 3, 5} rather than {1, 3, 4, 5}, status 4
  no longer reaching the match at all.

### User health — 3/5

```
SUCCESS  user_health_execute_submit
SUCCESS  handle_transfers_summary_ok
SUCCESS  handle_transfer_with_allowance_summary_ok
UNKNOWN  user_health_execute_submit_with_flash_loan
UNKNOWN  build_actions_from_request
```

The two transfer rules are summary-soundness checks: they establish that the
ported summaries over-approximate this fork's behaviour, which is what makes
the drift reconciliation (`Actions` gaining `check_max_util`, the realigned
ADR 0011 event no-ops, `ReserveConfig.supply_cap`) trustworthy rather than
assumed.

## The two UNKNOWN rules

UNKNOWN is **not** a violation. No counter-example was produced and neither
property was shown false. They are undecided on this fork's code under the
auditor's summary boundary.

Three attempts, and the pattern is the point:

| Job | Budget | `apply_*` summaries wired | Result | Elapsed |
|---|---|---|---|---|
| `184c3940…` | 1800s | no | TIMEOUT, UNKNOWN | ~20 min |
| `c5934d7b…` | 7200s | no | UNKNOWN, UNKNOWN | ~100 min |
| `68449b7e…` | 7200s | yes, all six | UNKNOWN, UNKNOWN | ~22 min |

Quadrupling the budget converted TIMEOUT into UNKNOWN, not into SUCCESS.
Wiring the six `apply_*` summaries the auditor also hooks was a real fix to
the port and cut time-to-verdict roughly fivefold, but did not change the
verdict. Reaching UNKNOWN *faster* with more summarization indicates a wall
the solver cannot pass on these two rules rather than resource exhaustion, so
more prover time is not the remedy.

Untried, in rough order of cost: split the two rules into separate jobs so
they stop competing for one job's resources; `loop_iter: 1` to test whether
the difficulty is loop-driven; a different solver via `prover_args`; and
failing those, treat this fork's larger `build_actions_from_request` as having
outgrown the auditor's postcondition and re-abstract the summary. The last is
spec work, not configuration.

## Limits

These are the limits of the inherited rule set, and they are the auditor's as
much as ours.

**The health property is weaker than its name suggests.** The report is
explicit: "Ideally, we would prove that a given user's health factor is always
greater than some nominal value. However, due to the complexity of the
arithmetic involved, this direct property is inherently difficult for
automatic provers. Instead, we specify a weaker property: if a user's
positions change, then either (i) it is in a way that obviously preserves the
health factor, or (ii) a designated function is called to check that the user
is still 'healthy.'" So `user_health_execute_submit` establishes that a check
was invoked. It is not a health-factor bound and not monotonicity.

**`user_health_execute_submit` is conditional.** It holds modulo the
`build_actions_from_request` summary, and that summary's soundness rule is
UNKNOWN. The auditor's chain has both links Verified; this fork has the
property without the link beneath it.

**Pool only.** No backstop, pool-factory or emitter rules exist. The backstop
was Code4rena contest scope and is excluded from the June 2025 report; the
auditor's repository contains no backstop specs.

**No tokens move.** `handle_transfers` and `handle_transfer_with_allowance`
are summarized to empty bodies and `spec/token.rs` is a mock client whose
`balance` is unconstrained. Nothing here concerns actual token movement, and
no solvency or value-conservation property is expressible without first adding
a balance model.

**Loops unrolled at most twice**, with `optimistic_loop: true`, against
`MAX_RESERVES: 30`. Any property quantified over a portfolio is established
only for two reserves.

**Summaries are unproven except where stated.** `events` (no-ops),
`emissions`, `auction::{fill, delete}` and the `actions` family are modeled.
Only `handle_transfers` and `handle_transfer_with_allowance` have passing
soundness rules; `build_actions_from_request`'s is UNKNOWN. This fork's ADR
0011 bad-debt and trap changes touch the summarized auction region.

**Five defined rules were never submitted** and so have no verdict:
`build_actions_from_request_sanity_1`, `build_actions_from_request_sanity_2`,
`user_health_sanity`, `user_health_flash_loan_sanity`, and
`target_util_should_be_less_than_0_9500000`. The first four are the explicit
vacuity checks, so **vacuity is unconfirmed**. Every conf sets
`rule_sanity: "basic"`, but those sub-results render only in the web report's
tree view, which is not captured here.

**Not addressed at all:** solvency, health-factor monotonicity, interest
accrual safety, liquidation correctness, oracle circuit breaker, borrow caps,
and value conservation. No rule in the harness speaks to any of them.

Authorization, the Soroban host and wasm runtime, ledger atomicity and
deployment behaviour are all outside the Prover's scope by construction.

## Reproducing

```sh
pip3 install certora-cli          # verified against 8.19.2
export CERTORAKEY=<key>
cd pool                           # build_script resolves against the cwd
certoraSorobanProver confs/user_rules.conf
```

Per-rule verdicts live at `output/<userId>/<jobId>/output.json`.
`jobData?attr=rules` returns `{}` even after a job succeeds, and a job whose
status is SUCCEEDED can still contain TIMEOUT and UNKNOWN rules, so the job
status is never a substitute for reading `output.json`.

Monitoring a job needs the `anonymousKey` that the CLI strips from the URL it
prints; it is recoverable from `pool/.certora_internal/`, and the data
endpoints return 403 without it.
