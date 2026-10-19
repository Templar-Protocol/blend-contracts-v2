# Certora Sunbeam prover run report

Results of running the ported Certora Sunbeam harness against this fork's
`main`. Setup, provenance and scope limits are in
[`docs/formal-verification.md`](../../docs/formal-verification.md); this file
is the run evidence.

Each `*.json` beside this file is the unmodified `output.json` fetched from
`prover.certora.com/output/<userId>/<jobId>/output.json`.

> **Read this before quoting any verdict below.** The headline health rule,
> `user_health_execute_submit`, passes without ever exercising the property it
> names. Its post-state is always equal to its pre-state, so the assertion is
> satisfied by its trivial disjunct on every path. The verdict is SUCCESS and
> the property is untested. This is inherited verbatim from the auditor's
> harness and is therefore also a limitation of the June 2025 report's
> Verified status, not a defect introduced by this port. It is left in place
> deliberately; [Limits](#limits) gives the mechanism and the fix it would
> take.

## Summary

**24 of the 26 configured rules** returned SUCCESS on this fork, covering 24
distinct properties. Two are undecided. No rule was violated and no
counter-example was produced.

`remove_supply_decreases_b_supply` originally called `User::remove_collateral`,
duplicating `remove_collateral_decreases_b_supply`. The body is corrected here
and `user_rules.conf` was rerun as job `3ce7b162…`, which returned 12/12
SUCCESS against the corrected rule. Job `1b57051d…` is retained as the
superseded run.

| Verdict | Count |
|---|---|
| SUCCESS | 24 |
| UNKNOWN | 2 |
| VIOLATED | 0 |

Those counts are verdicts, not assurance. `user_health_execute_submit` is in
the SUCCESS column and is untested, per the note above.

What that does **not** mean is covered under [Limits](#limits). In particular,
`user_health_execute_submit` is SUCCESS only modulo a summary whose own
soundness rule is one of the two UNKNOWN results.

## Jobs

All jobs ran under `prover_version: master` against this fork's `main`, not
against upstream `996e09e` with the 2025 prover. User id `4040469`.

This branch later merged six Guardian audit commits, which added a
`require_min_position` guard and an oracle `load_price` call inside
`apply_supply_collateral`, a `plan_withdraw` overflow guard, and a
`min_position` field on `ReserveConfig`. `health.conf` and `user_rules.conf`
were rerun after that merge and reproduced their verdicts exactly. The four
`pool_status*.conf` jobs predate the merge and were not rerun: they exercise
`execute_update_pool_status`, which the merge left untouched, as it did
`user.rs`, `submit.rs`, `reserve.rs` and `pool.rs`. The changed `apply_*`
bodies sit behind havocking summaries that over-approximate them, and the
merge only adds panics, which such a summary already covers.

| Date | Conf | Job | Budget | Outcome |
|---|---|---|---|---|
| 2026-10-04 | `user_rules.conf` | `1b57051d…` | default | 12/12 SUCCESS (superseded) |
| 2026-10-15 | `user_rules.conf` | `3ce7b162…` | default | 12/12 SUCCESS (pre-merge) |
| 2026-10-16 | `user_rules.conf` | `f684493d…` | default | 12/12 SUCCESS |
| 2026-10-04 | `pool_status1.conf` | `638ceeac…` | default | 3/3 SUCCESS |
| 2026-10-04 | `pool_status3.conf` | `32f65ab6…` | default | 3/3 SUCCESS |
| 2026-10-05 | `pool_status2.conf` | `bf0cc6db…` | 1800s | 2/2 SUCCESS |
| 2026-10-05 | `pool_status4.conf` | `b51c0b2d…` | 1800s | 1/1 SUCCESS |
| 2026-10-05 | `health.conf` | `184c3940…` | 1800s | 3 SUCCESS, 1 TIMEOUT, 1 UNKNOWN (pre-merge) |
| 2026-10-16 | `health.conf` | `09dfece5…` | 1800s | 3 SUCCESS, 1 TIMEOUT, 1 UNKNOWN |
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

The two transfer rules are summary-soundness checks for the transfer
summaries alone: they establish that `handle_transfers` and
`handle_transfer_with_allowance` leave position entries unchanged. They say
nothing about the other summaries, so the rest of the drift reconciliation
(`Actions` gaining `check_max_util`, the realigned ADR 0011 event no-ops,
`ReserveConfig.supply_cap`) stays assumed rather than established.
[Limits](#limits) lists which summaries are unproven.

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

**`user_health_execute_submit` does not exercise its own non-trivial branch.**
The summary takes `from_state: &mut User` and never writes it: it havocs a
separate `new_state`, may `store` that, and assumes the postcondition over the
pair. `execute_submit` then validates `from_state` and ends with
`from_state.store(e)`, so the address under test is written back unchanged.
The rule reloads it, `orig_state` and `new_state` agree, and `should_check`
returns false for equal states, which satisfies the assertion through its
second disjunct on every path. `model::init()` havocs `GHOST_CHECKED`, so the
assertion is reachable and `rule_sanity: basic` does not flag it; the SUCCESS
verdict is therefore consistent with the property never being tested. This is
inherited verbatim: the auditor's summary, rule and `from_state.store(e)` are
byte-identical, so the June 2025 Verified status carries the same limitation.
Closing it needs the summary to bind `new_state` to the submitting address and
apply the modeled positions to `from_state`, then a `health.conf` rerun.

**Four more inherited rules are weaker than their names suggest.** Each is
byte-identical to the auditor's, so the June 2025 report carries these too:

- `positions_hf_under` returns an unconstrained bool. Its result does not
  depend on the positions, pool or health factor it is handed, so a rule that
  reaches it treats an arbitrary value as a passed health check and can accept
  positions the concrete helper would reject with `InvalidHf` or
  `MinCollateralNotMet`.
- `add_liabilities_increases_liabilities` asserts the position grew by *at
  least* `amount`. Production adds exactly `amount`, so over-crediting the
  position while incrementing `d_supply` correctly would still pass.
- `verify_update_status_2` asserts
  `q4w_pct < 0_7500000 || status_after == 5`, which holds on every path where
  `q4w_pct` is below the threshold whatever the status becomes. The status-0
  and other-status rules each have a complementary `_b`/`_c` rule; status 2
  has none.
- `target_util_should_be_less_than_0_9500000` is a counter-example demo
  rather than a property. It assumes `config.util > 0_9500000` and asserts the
  middle interest branch is taken, which those assumptions make impossible.
  The hazard its comment describes is already prevented in production:
  `require_valid_reserve_metadata` panics on `util > 0_9000000` on both the
  queue and execute paths, so the premise is unreachable for any validated
  config. It is one of the five rules never submitted.

**Pool only.** No backstop, pool-factory or emitter rules exist. The backstop
was Code4rena contest scope and is excluded from the June 2025 report; the
auditor's repository contains no backstop specs.

**No tokens move.** `handle_transfers` and `handle_transfer_with_allowance`
are summarized to empty bodies and `spec/token.rs` is a mock client whose
`balance` is unconstrained. Nothing here concerns actual token movement, and
no solvency or value-conservation property is expressible without first adding
a balance model.

**Loops barely unrolled**, with `optimistic_loop: true`, against
`MAX_RESERVES: 30`. Only `health.conf`, `health_unresolved.conf` and the two
supply-cap confs set `loop_iter: 2`; `user_rules.conf`, `pool_status*.conf`,
`oracle.conf` and `utilization.conf` leave it at the default 1. Because
`optimistic_loop` assumes away the paths that would need another iteration
rather than reporting them, a property quantified over a portfolio is
established for at most two reserves, and for one wherever `loop_iter` is
unset.

**Summaries are unproven except where stated.** `events` (no-ops),
`emissions`, `auction::{fill, delete}` and the `actions` family are modeled.
Only `handle_transfers` and `handle_transfer_with_allowance` have passing
soundness rules; `build_actions_from_request`'s is UNKNOWN. This fork's ADR
0011 bad-debt and trap changes touch the summarized auction region.

**Five defined rules were never submitted** and so have no verdict:
`build_actions_from_request_sanity_1`, `build_actions_from_request_sanity_2`,
`user_health_sanity`, `user_health_flash_loan_sanity`, and
`target_util_should_be_less_than_0_9500000`. The first four are the explicit
vacuity checks, so **vacuity is unconfirmed**. Submitting the two
`build_actions_from_request_sanity_*` rules as written would not close it:
both call `pool::actions::build_actions_from_request`, which under `certora`
is the `apply_summary!` wrapper dispatching to the nondeterministic summary,
so they would witness the summary rather than the builder. The original is
nested by the macro as `build_actions_from_request::build_actions_from_request`.
Both rules, and the stale TODO comment admitting this, are inherited verbatim
from the auditor's branch. Every conf sets `rule_sanity: "basic"`, but those
sub-results render only in the web report's tree view, which is not captured
here.

**Not addressed by the auditor's rule set:** solvency, health-factor
monotonicity, interest accrual safety, liquidation correctness, the oracle
circuit breaker, the supply and utilization caps, and value conservation. No
rule among the 26 configured here speaks to any of them.

Some are covered elsewhere. Rules added after the audit establish the oracle
circuit breaker, and the rejection behaviour of both utilization guards over a
bounded reserve domain; see
[`docs/formal-verification.md`](../../docs/formal-verification.md). The
Kani suite covers liquidation, the conversion-rounding obligations and most of
interest accrual; see [`docs/kani.md`](../../docs/kani.md). Solvency,
health-factor monotonicity and value conservation are unaddressed by either
suite — the rounding obligations constrain which way each conversion rounds,
which is a prerequisite for value conservation and not the property itself,
and as stated above no value-conservation property is expressible without a
balance model.

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
