# Formal verification status

All seven of the safety invariants this protocol commits to are backed by
machine-checked proof, and across the whole campaign — two independent provers
and a fuzzer — **no counterexample has ever been produced and no rule has ever
been violated.** Where a prover did not finish it either exhausted its budget
or returned undecided, and those are different things; in neither case did it
find a way to break a property.

What each proof covers differs, and this document is explicit about it rather
than leaving it to the footnotes. Every invariant is proven over a stated scope
and under stated assumptions. Two are proven against a faithful model rather
than the live component. Solvency is the partial one: it covers three of the
seven ways the reserve book can change, and it bounds how those three move the
pool's position rather than establishing the `assets >= liabilities` inequality
outright. Two vacuous results are documented below: the Certora user-health
execution rule remains vacuous; the formerly vacuous Kani zero-debt accrual
harness now has a verified, reachable zero-growth proof.

This is the entry point; the per-tool detail lives in
[`docs/kani.md`](docs/kani.md),
[`docs/formal-verification.md`](docs/formal-verification.md) and
[`verification/certora/README.md`](verification/certora/README.md).
Additional local proof results, their scopes and the remaining strengthening
work are documented in [`docs/verification-local.md`](docs/verification-local.md).

## The seven invariants

| Invariant | In plain terms | Prover | Result | Assumptions |
| --- | --- | --- | --- | --- |
| **Solvency** | The pool never owes depositors more than the assets backing them. | Kani | proven for 3 of 7 ways the book can change | **D**, **S** |
| **No free money** | You cannot extract value by converting between assets and pool tokens, in either direction. Rounding always favours the pool. | Kani | proven | **D**, **S** |
| **Interest accrual safety** | Interest only ever moves forward. Accrual cannot reduce a debt, and cannot pay out more interest than it charged. | Kani | proven | **D** |
| **Health factor monotonicity** | Adding collateral can only improve a position; borrowing more can only worsen it. No action has a perverse effect. | Kani | proven | **D**, **S** |
| **Borrow cap** | Deposits and utilization cannot exceed the limits the pool is configured with. | Kani | proven | **D**, **T** |
| **Liquidation correctness** | Liquidation auctions price, discount and time correctly, and bad debt is handled without creating value. | Kani + Certora | proven | **S** |
| **Oracle circuit breaker** | The pool refuses a price that is missing, zero, negative, future-dated or more than 24 hours stale. | Certora | proven, 13/13 rules | **T** |

The protocol has no borrow cap as such; it has a supply cap and a maximum
utilization, which is what the audit property list calls "borrow caps".

## What the assumption classes mean

A proof is only as strong as what it assumes. Every row above is graded against
the same three gaps, and each row names the ones that apply.

- **D — domain narrowed.** The property is proven exhaustively, but over a
  restricted range of inputs rather than every value the type allows. Typically
  balances from 0 to 255 and interest rates in hundredths up to 2.55×. The
  arithmetic is checked for *every* combination in that range, with no sampling.
  New Verus theorems establish supply/withdrawal bounds over unbounded mathematical
  integers. Applying those wider theorems to production Rust requires the
  checked-i128/I256/host refinement; the existing production proof domains remain
  as stated.
- **T — target substituted.** The property is proven about a faithful model of a
  component rather than the component itself. The oracle rules, for instance, are
  proven against a mock price feed that can return any value, which is the right
  way to test the pool's *reaction* to a bad price, but is not the live feed.
- **S — scope narrowed.** A component of the named property is proven rather than
  the whole property end to end. Liquidation correctness proves the auction
  arithmetic and decisions, not a full liquidation executed against a live pool.

No row is free of all three, so none is claimed as unconditional. That is the
honest shape of formal verification on a system this size, and naming the gap is
what makes each claim checkable.

## Where each row's gap lies

- **Solvency** is the partial row and the most important, so it is stated
  precisely. The reserve book changes in seven ways; three are proven — deposit,
  withdrawal (on its uncapped path) and interest accrual. Borrowing, repayment,
  a default absorbed into the exchange rate, and the backstop-credit payout are
  not covered. The pool's token balance is not a modelled quantity and no
  initially-solvent starting state is established, so what these proofs give is
  that the three covered transitions cannot degrade the pool's position — each
  bounds the change in what the pool owes by the amount of value moved. They do
  not establish the absolute `assets >= liabilities` inequality, which would
  need both the balance modelled and a base case.
- **No free money** and **health factor monotonicity** are proven about the
  conversion and ratio arithmetic that every action routes through, not about the
  handlers that call it.
- **Borrow cap** is proven at the action level over the restricted domain; the
  same bound at production width is undecided by both provers, across thirteen
  measurements.
- **Liquidation correctness** and the **oracle circuit breaker** are proven more
  narrowly than their names suggest — see the **S** and **T** notes above.

## Evidence

| | |
| --- | --- |
| Kani (bounded model checking) | Published inventory: 110 proof harnesses plus 1,678 generated fixed-parameter instances. The campaign at `753b1ae` passed 3,197/3,197 obligations. The Kani 0.68.0 property-mapped rerun at `886e26f` recorded 35 SUCCESSFUL, 2 budget-limited and 0 counterexamples; the separate supply/withdrawal solvency pair also passed. See [`docs/kani.md`](docs/kani.md) for the source-scoped records. |
| Certora Sunbeam | The auditor's rule set replicated: 24 SUCCESS, 2 UNKNOWN, **0 VIOLATED**. Beyond it: oracle circuit breaker 13/13, utilization caps 9/10. |
| Fuzzing | A libFuzzer target exercises request sequences against a full pool fixture. When all totals are evaluable, the sum of floor-rounded price valuations of `(supplier claims + credit)` must strictly exceed the sum of ceil-rounded price valuations of debt. Unevaluable totals are logged and skipped. |
| Local strengthening | `make verify-local` passed five Kani harnesses with all 21 required covers, plus nine Verus proof functions: three unbounded-integer supply/withdrawal theorems, four helpers and two witnesses. The local set includes the repaired zero-debt proof and checked contract consumption, and verifies the production oracle guards under modeled host interfaces. The ceil-mint negative control detected both intended failures. |
| Production arithmetic extraction | Charon/Aeneas successfully extracted and translated the actual reserve conversions, dispatch and checked-i128/I256 branches: 23 transparent functions and 22 opaque functions. Proving the resulting SDK/arithmetic refinement obligations is the next step; this translation result is recorded separately from theorem verdicts. |
| Third-party audit | See [`audits/`](audits/). Formal verification complements an audit; it does not replace one. |

The local acceptance set strengthens and rechecks selected existing results; it is
not a disjoint count to add to the published inventory. Detailed verified scopes
and further strengthening goals are recorded in
[`docs/verification-local.md`](docs/verification-local.md). Intentional failures
of the mutated arithmetic model are negative controls, not protocol
counterexamples.

### What a passing verdict does not always mean

Two Certora rules return UNKNOWN and two Kani harnesses exhaust their budget.
Neither is a violation: UNKNOWN means the prover could not decide, and a budget
exhaustion means it ran out of time. In both cases the property's components
verify independently.

More importantly, a SUCCESS verdict can be *vacuous* — satisfied without ever
exercising the property it names. Two such results were identified; the zero-debt
case has now been repaired. Both original outcomes remain visible below:

- **`user_health_execute_submit`** (Certora) passes because its post-state always
  equals its pre-state, so a trivial disjunct satisfies the assertion. It is in
  the 24 SUCCESS column, but that rule's user-health obligation is unexercised.
  This is distinct from the Kani health-monotonicity proofs in the table above.
  The vacuous rule is inherited verbatim from the auditor's harness and is also
  a limitation of the June 2025 report's Verified status.
- **`prove_accrual_conserves_value_debt_000`** (Kani) historically satisfied 0
  of its 2 positive-growth reachability checks. The replacement zero-debt proof
  establishes zero accrued value and unchanged supplier claims/credit, with
  **4/4 covers satisfied** under Kani 0.68.0. This is an exhaustive checked-i128
  kernel result on the documented grid; production I256 refinement is a separate
  strengthening goal. See [`docs/verification-local.md`](docs/verification-local.md)
  for the local acceptance command and exact domain.

Proof verdicts and reachability outcomes are recorded together so meaningful
passes remain distinguishable from vacuous ones. The local acceptance set enforces
its required covers and mathematical witnesses; a bare SUCCESS verdict alone is
insufficient.

## Continuous enforcement

Verification is not a one-off report. Every pull request must pass three jobs
before it can merge: one that compiles every proof harness, one that re-verifies
the fast harness set, and one that builds and exercises the fuzz target. The
expensive harnesses re-run weekly. Before these existed, a proof could silently
stop compiling — which happened, and is why they exist.

## Reproducing this

All of it runs from a clean checkout; nothing here depends on a hosted service
except the Certora runs, which carry job IDs and archived result JSON in
[`verification/certora/`](verification/certora/). `docs/kani.md` gives the exact
command form, the toolchain versions, and how to read a verdict — including why
a `FAILED` line must be distinguished from a timeout and from a prover crash
before it is called a finding.
