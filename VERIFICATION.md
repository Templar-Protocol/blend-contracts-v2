# Formal verification status

All seven safety claims have machine-checked **component evidence**, not
unconditional end-to-end production proofs. The scopes differ: bounded arithmetic,
substituted components and incomplete transition coverage remain explicit gaps.
Solvency does not yet establish the absolute `assets >= liabilities` inequality.
One historical health rule is vacuous; the historical zero-debt accrual vacuity
now has a reachable replacement proof.

The historical campaigns below reported no production counterexample. Timeouts
and UNKNOWN results are not proofs. Intentional negative controls do fail and
are recorded separately from production findings.

This is the entry point; the per-tool detail lives in
[`docs/kani.md`](docs/kani.md),
[`docs/formal-verification.md`](docs/formal-verification.md) and
[`verification/certora/README.md`](verification/certora/README.md). The current
local-only strengthening campaign, accepted inventory and stuck/open outcomes are
in [`docs/verification-local.md`](docs/verification-local.md). It does not close
all seven claims or reclassify historical campaigns as current-tree full runs.

## The seven invariants

| Invariant | In plain terms | Prover | Result | Assumptions |
| --- | --- | --- | --- | --- |
| **Solvency** | The pool never owes depositors more than the assets backing them. | Kani | proven for 3 of 7 ways the book can change | **D**, **S** |
| **No free money** | You cannot extract value by converting between assets and pool tokens, in either direction. Rounding always favours the pool. | Kani | proven | **D**, **S** |
| **Interest accrual safety** | Interest only ever moves forward. Accrual cannot reduce a debt, and cannot pay out more interest than it charged. | Kani | proven | **D** |
| **Health factor monotonicity** | Adding collateral can only improve a position; borrowing more can only worsen it. No action has a perverse effect. | Kani | proven | **D**, **S** |
| **Borrow cap** | Deposits and utilization cannot exceed the limits the pool is configured with. | Kani | proven | **D**, **T** |
| **Liquidation correctness** | Liquidation auctions price, discount and time correctly, and bad debt is handled without creating value. | Kani + Certora | proven | **S** |
| **Oracle circuit breaker** | The pool refuses a price that is missing, zero, negative, future-dated or more than 24 hours stale. | Certora (historical), Kani (local) | historical 13/13 rules; local first-read and decimals rules | **T**, **S** |

The protocol has no borrow cap as such; it has a supply cap and a maximum
utilization, which is what the audit property list calls "borrow caps".

## What the assumption classes mean

A proof is only as strong as what it assumes. Every row above is graded against
the same three gaps, and each row names the ones that apply.

- **D — domain narrowed.** The property is proven exhaustively, but over a
  restricted range of inputs rather than every value the type allows. Typically
  balances from 0 to 255 and interest rates in hundredths up to 2.55×. The
  arithmetic is checked for *every* combination in that range, with no sampling.
  Full-width production arithmetic remains unproved here. New arbitrary-integer
  lemmas do not remove this gap without the production i128/I256/host refinement.
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
| Kani (bounded model checking) | Historical inventory: 110 proof harnesses plus 1,678 generated fixed-parameter instances. Published campaign: 3,197 of 3,197 obligations PASS at commit `753b1ae`. The previously documented property-mapped rerun was 35 SUCCESSFUL, 2 budget-limited, 0 counterexamples, plus the solvency pair; it is not a full rerun of the strengthened tree. |
| Certora Sunbeam | The auditor's rule set replicated: 24 SUCCESS, 2 UNKNOWN, **0 VIOLATED**. Beyond it: oracle circuit breaker 13/13, utilization caps 9/10. |
| Fuzzing | A libFuzzer target drives randomised request sequences against a full pool fixture and asserts the pool's supply exceeds its liabilities after every one. |
| Local strengthening | `make verify-local`: five Kani harnesses, all 21 required covers; nine Verus mathematical proof functions; intended ceil-mint mutation failures detected. Exact production arithmetic extraction/translation succeeded, but no Lean refinement theorem was proved. Stuck initializer, borrow/repay and price-cache probes are excluded from acceptance and documented explicitly. |
| Third-party audit | See [`audits/`](audits/). Formal verification complements an audit; it does not replace one. |

### What a passing verdict does not always mean

Two Certora rules return UNKNOWN and two Kani harnesses exhaust their budget.
Neither is a violation: UNKNOWN means the prover could not decide, and a budget
exhaustion means it ran out of time. In both cases the property's components
verify independently.

More importantly, a SUCCESS verdict can be *vacuous* — satisfied without ever
exercising the property it names. Two such cases are known and are deliberately
left visible rather than quietly counted as wins:

- **`user_health_execute_submit`** (Certora) passes because its post-state always
  equals its pre-state, so a trivial disjunct satisfies the assertion. It is in
  the 24 SUCCESS column and the property is untested. This is inherited verbatim
  from the auditor's harness, so it is equally a limitation of the June 2025
  report's Verified status, not something introduced here.
- **`prove_accrual_conserves_value_debt_000`** (Kani) historically satisfied 0
  of its 2 positive-growth reachability checks. The replacement zero-debt proof
  establishes zero accrued value and unchanged supplier claims/credit, with
  **4/4 covers satisfied** under Kani 0.68.0. It retains the stated grid domain;
  it does not establish full-width arithmetic or the Soroban I256 bridge.
  See [`docs/verification-local.md`](docs/verification-local.md) for the local
  acceptance command and exact scope. The historical campaign is not rewritten.

Every other result quoted above satisfies its reachability checks, which is how
a non-vacuous pass is distinguished from a vacuous one. Vacuity is tracked
because an undetected vacuous proof is worse than no proof: it reads as
assurance and provides none.

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
