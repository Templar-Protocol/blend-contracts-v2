# Local verification strengthening

This campaign strengthens the seven north-star claims without changing runtime
behavior, ABI, storage, dependencies or rounding. New verdicts must be reproducible
locally; no new hosted Certora jobs are submitted. Existing historical campaigns
are retained, not relabeled as current-tree full runs.

## Acceptance

From the repository root:

```sh
make verify-local
```

Requires the repository Rust/Wasm build prerequisites and Kani 0.68.0 (CBMC
6.11.0), plus a working user systemd manager with delegated cgroup v2. The current
inventory is `verification/local-proofs.json`. Each entry names one exact harness
and a required cover count. A successful process exit alone is insufficient:
`VERIFICATION:- SUCCESSFUL`, exactly one completed harness, and every expected
cover must agree. The existing `scripts/kani_release.py::parse_log` is the verdict
authority. Proof logs are written under `target/verification-local/`.

The runner serializes attempts and checks the worker's actual cgroup limits:
4 GiB memory, zero swap, 256 tasks, CPU quota at most 200%, and group OOM killing.
Systemd applies a bounded runtime and `KillMode=control-group`; the parent stops
the unit on interruption and rejects a surviving active service. Missing
containment fails closed. This safety boundary is not part of the contract proof.

The acceptance command checks the claimed inventory, **not** every historical
Kani harness or the full workspace test suite. Existing optimized Wasm imports
must be present (`make build` prepares them). No tool auto-install occurs during
acceptance.

### Enforcement controls exercised

- A temporary impossible fifth cover made Kani report SUCCESSFUL with 4/5
  covers and exit zero; `make verify-local` rejected it. The mutation was removed.
- Direct invocation of the worker outside the required cgroup was rejected
  before executing the requested command.
- A one-second service deadline terminated a 30-second sleep and returned
  failure. SIGTERM interruption also stopped a real worker and its sleeping
  descendant; neither was running afterward.
- Eleven malformed inventory/selector/deadline inputs were rejected before a
  proof launch. The verdict-parser regression distinguishes missing witnesses
  (validation failure) from an actual failed proof (semantic failure).


## Verified improvements

### Zero-debt accrual: reachable zero-growth behavior

`pool::reserve::verification::prove_accrual_conserves_value_debt_000` now calls the
actual `ReserveData::grow_debt` and `accrue` kernels and proves:

- accrued debt value is zero and aggregate debt value remains zero;
- supplier claims, supplier shares/rate, debt shares and backstop credit do not
  change;
- the debt rate may still increase without creating aggregate debt value.

Four required reachable cases: empty supplier total; positive supplier total with
zero supplier rate; positive supplier claims; and increased debt rate. The last
case distinguishes a zero-value theorem from an incorrect whole-state no-op
claim.

Domain: debt tokens zero; supplier tokens 0..255; prior credit 0..255; supplier
rate in hundredths 0..2.55 Q; debt rate and growth factor in hundredths 1..2.55 Q;
Q=1e12. The backstop take argument is arbitrary u32 because the zero-accrual
branch does not consume it. Arithmetic uses the existing checked-i128 proof
implementation. This is **not** the full-width production I256 bridge, an actual
initializer proof, or an end-to-end solvency theorem.

Observed under the contained `make verify-local` invocation: SUCCESSFUL, 4/4
covers, 30.20 s solver time, 75.28 s service runtime and 1.6 GiB peak memory with
zero swap. Historical 0/2 positive-growth-cover results remain in `docs/kani.md`.

## Attempt ledger and stop policy

At most 20 distinct proof attempts overall, at most three approaches per
obligation. After 60 minutes without verified progress on an obligation, mark it
stuck and move on. Unsupported source extraction, compilation failure, timeout,
missing covers and unresolved summary dependencies are not proofs. Reproduction
of an unchanged successful attempt is an acceptance check, not another approach.
Tool preparation and infrastructure smoke checks are not proof attempts.

| Attempt | Obligation / approach | Outcome |
| --- | --- | --- |
| 1 | Separate real zero-debt accrual kernel path and meaningful covers | PASS, 4/4 covers; grid domain retained |

## Open obligations

- Actual reserve initializer/storage refinement and nonnegative cash base case.
- Nonvacuous health execution, original-builder summary soundness and numeric
  health-predicate refinement.
- Unsummarized supply admission through cache/persistence and the actual
  maximum-utilization loop; no permanent `total_supply <= cap` invariant.
- Arbitrary per-call/per-asset oracle responses and price/decimals cache
  provenance/reset, without market-price accuracy assumptions.
- Arbitrary-integer supply/withdrawal lemmas; checked contract consumption;
  source-connected production fast/wide arithmetic and rejected paths.
- Cash and pending transfers, global supply/debt position totals, actual
  transition/caller frames, partial/full liquidation, residual default, orphan
  settlement, interest, health and unbounded request/reserve sequence induction.
- Public rejection of disabled credit-payout paths, rather than treating private
  dormant helpers as live entrypoints.

No open obligation is silently removed from its north-star claim. Compiler,
translation/prover soundness, Soroban host semantics and atomicity, ledger time,
and explicit token-interface contracts remain trust boundaries unless separately
proved. No production refactor or trusted target stub is authorized to bypass
these gates.
