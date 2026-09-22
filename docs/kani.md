# Lending-kernel Kani verification

## Scope and rules

This branch extends security-fork commit `54afdae01bb8e484514940229e06460aa3fa6a1f`. Kani only: no whole-market solvency, Soroban host, authorization, oracle, transaction rollback, TTL, or custody proof is implied. The public contract ABI remains unchanged; the rebased pool Wasm hash differs because the proof-facing helper extraction is no longer byte-identical on the newer `debt_setoff` event path. Existing native/Wasm/differential checks cover separate integration obligations.

Production uses Rust 1.81, SDK 22.0.7, host 22.1.3, fixed-point math 1.3.0, and Stellar 22.6.0. Preserve overflow checks and the lockfile. Kani 0.67.0 uses its bundled compiler and CBMC 6.8.0, independently of the production compiler.

A proof must call actual production logic. Extracted helpers must be production-delegated, behavior preserving, and optimized-Wasm/ABI byte neutral. Arithmetic boundary functions must retain production SorobanFixedPoint and I256 fallback. Harness arithmetic may use the dependency's real FixedPoint implementation only on an explicitly product-fit domain; this does not prove host fallback. No copied business model, undocumented rate bound, universal one-unit dust bound, or disabled safety checks.

## Preregistered inventory

All rows initially **not attempted** unless explicitly stated. Expected result is successful verification on the eventual declared domain, not success over unrestricted protocol states. Exact harness bounds must be recorded before each first run; restricted domains must not be represented as protocol-enforced constraints. All entry points are loop-free scalar kernels unless otherwise recorded. Kani assertion reachability and default safety checks remain enabled.

| ID | Actual source / production callsite | Independent obligation | Domain / boundary | Initial status |
|---|---|---|---|---|
| A1 | `pool/config.rs::is_disable_only`, reserve queue/execute checks | Independent arbitrary configurations accepted iff enabled-to-disabled and every other field preserved | All field-width values, no metadata validity assumption | Existing narrower mutation harness recalibrated: 254 checks, zero failures; stronger theorem pending |
| B0 | `backstop/pool.rs::PoolBalance` conversions | Empty balances and exact full redemption follow specified branches | Nonnegative i128 tokens, positive i128 shares | Temporary direct-source calibration: 115 checks, zero failures; four branch-unreachable checks reported |
| B1 | `PoolBalance` conversions and `non_queued_tokens` | Round trips do not create underlying value; available tokens stay in bounds | Nonnegative balances, queued shares <= total shares; product-fit arithmetic, bounded symbolic subdomain to be recorded | Not attempted |
| B2 | `PoolBalance::deposit`, queue/dequeue/withdraw | Valid scalar transitions conserve changed balances and queued limits | Nonnegative valid caller inputs; representable additions; host error branches excluded explicitly where necessary | Not attempted |
| B3 | `pool/status.rs::calc_pool_backstop_threshold`, backstop `is_pool_above_threshold` | Threshold predicates agree including floor and saturation | Nonnegative underlying balances; actual functions, no third implementation | Not attempted |
| C1 | `pool/bad_debt.rs::check_and_handle_user_bad_debt` setoff | Committed burn <= claim, repaid <= debt, debt = repaid + residual; zero repayment commits no burn | Nonnegative claims/debt/b-rate; positive d-rate; product-fit, zero/partial/full branches required | Not attempted; core |
| C2 | `pool/user.rs::default_liabilities` loss-rate calculation | Rate never increases or becomes negative; no suppliers preserves rate without division | Nonnegative rate and post-setoff supplier count, positive default; product-fit | Not attempted; core |
| C3 | Bad-debt residual decision and `execute_gulp`/`remove_supply` | No default retains collateral; gulp burn delta matches supply delta and pays no underlying | Scalar decision only unless storage can be analyzed directly; events, maps and emissions remain host boundaries | Conditional; not attempted |
| D1 | `pool/reserve.rs` b/d conversion functions | Exact floor/ceil inequalities and conservative round trips | Nonnegative amounts, positive divisor rates; product-fit and recorded symbolic bounds | Not attempted; core |
| D2 | `pool/actions.rs` withdraw/collateral/repay calculations | Burn <= position, withdrawal <= computed claim, input = retained repayment + refund | Successful arithmetic domain; no full request/auth claim | Not attempted |
| D3 | Reserve utilization and supply/health comparisons | Capped range; zero-liability branch; actual strictness at max/100% and caps | Nonnegative computed totals; positive divisor only on division branch; no oracle/routing claim | Not attempted |
| E1 | `pool/interest.rs::calc_accrual` scalar kernel | Valid divisors, clamped modifier, accrual >= unity | Metadata-valid configuration; positive elapsed time; product-fit; no invented r_three bound | Not attempted |
| E2 | `Reserve::load` debt accrual and `Reserve::accrue` | Nonnegative debt interest; correct supplier/credit split; zero take-rate leaves credit unchanged | Recorded bounded domain; nested floor retained; no floor cancellation or fixed one-unit asset bound | Not attempted |
| F1 | `pool/auctions/auction.rs::scale_auction` timing kernel | Modifier bounds, monotone lot/bid progression and maturity boundaries | Nonnegative elapsed block count from chronological ledgers | Not attempted |
| F2 | Auction scalar quote partition | Filled base + remaining base = original; time-discounted values separately bounded | Valid fill percentage; nonnegative amounts, product-fit | Not attempted |
| F3 | Pool/reserve action decisions and reserve queue timing | Allowed/rejected action table; exact delay boundaries | Full-width status/action values where accepted by actual function; queue authorization/storage excluded | Conditional; not attempted |

## Acceptance

Before each first proof, record harness names, source linkage, all quantified bounds and premises, predicted result and risk. Retain terminal verdicts and exact argv/tool/source hashes; distinguish success, counterexample, timeout, unsupported, and not attempted. Zero selected harnesses is failure. Infeasible assumptions or incomplete unwind are not success.

Check boundary witnesses and temporary local semantic mutations. Mutation compile errors do not establish theorem sensitivity. Never prune counterexamples by assuming the conclusion. Review theorem domains independently before acceptance.

Optimized baseline SHA256:

- pool: `42b00a47ae35471b50401f31467fdf1c0c6acc1c851864a3d282d0db860943da`
- backstop: `6bd13007c020b617c43c718c2ad3d2d13605d4d42fa8f739da2293f8a4e293a0`
- pool factory: `80abaf3c68fd15adade9f82165baa9b50d85800e497c99b45809e1fbd7397de2`

Rebuilt from the exact baseline under pinned tools. All three optimized bytes must remain identical, not merely equivalent ABI. A mismatch blocks integration of that extraction. Run prescribed workspace and explicit differential gates and relevant native/Wasm scenarios. Prior generated verification is separate; a running job is not a passing receipt.

A final local third commit requires material core settlement and rounding coverage, independent assumption/linkage and correctness/Ponytail reviews, and no unresolved required proof. Failed or unsupported core proofs require an explicit scope decision rather than relabelling cheap predicate proofs as complete market verification.

## Execution outcome: blocked, no proof commit created

The preregistration above is preserved as the original target, **not** a list of completed proofs. Implementation and feasibility work stayed in isolated filesystem copies. No production source, Cargo manifest/lockfile, or existing commit in the live checkout was changed. This ledger is the only live-checkout addition from this work.

### Artifact gate

The exact baseline rebuilt successfully with the pinned production tools. The combined extraction candidate also built, but optimized `pool.wasm` changed from 54,368 to 54,374 bytes, SHA256 `5a86ee529754b8fe91e42e75bec5f6ba6e65474516facb35765f04a16743c1f7`. Each of the four production-file extractions changed the pool artifact independently; narrowly scoped helper inlining did not restore equality. A pristine rebuild in the same discriminator directory reproduced the baseline, ruling out the directory itself as the cause.

The first discriminator incorrectly preserved old source mtimes with `copy2`, permitting stale Cargo results after its first case. That attempt is explicitly invalidated. The replacement used fresh writes and a pristine control; only those corrected results support the conclusions.

The pool, backstop, and factory `contractspecv0` payloads remain byte-identical in the combined candidate. This proves ABI payload identity, **not** the required whole-Wasm identity or behavioral equivalence.

The separate `cfg(kani)`-only candidate, with no production extractions, reproduced all three optimized baseline artifacts exactly. It is useful partial work, not a substitute for the required settlement/rounding portfolio.

### Verified direct subset

Kani 0.67.0 / CBMC 6.8.0, with Kissat selected explicitly:

| Harness | Declared domain | Terminal result |
|---|---|---|
| `prove_disable_only_transition` | Independent arbitrary complete configurations; constructed valid/reverse transition witnesses | 277 checks, zero failures |
| `prove_share_zero_and_full_redemption` | Nonnegative full-width i128 tokens, positive shares; empty and positive-share/zero-token branches | 128 checks, zero failures; four unreachable checks |
| `prove_share_round_trip_and_available_tokens` | Amount, tokens, shares, queued each 0..255; positive denominators and queued <= shares | 117 checks, zero failures; four unreachable checks |
| `prove_deposit_and_queue_balances` | Nonnegative full-width i128 values, queued <= shares, representable additions | 88 checks, zero failures |

Every run selected exactly one harness. B1 initially timed out under CaDiCaL, then passed with Kissat without changing its source, domain, or assertions. The corrected B0 includes the positive-shares/zero-tokens branch missed by its initial calibration. The unreachable checks do not establish coverage of the excluded paths.

A private negative-control copy making the disable-only predicate reject every transition compiled successfully and failed the valid-transition assertion. This is a detected semantic mutation, not a compiler failure.

### Candidate proofs that do not satisfy acceptance

- C1: the extracted setoff harness passed 78 checks and all five covers (two unreachable checks). Independent review correctly limits this to positive burn/repayment caps on its u8/hundredth-rate domain. Its residual equality is calculated inside the harness; it does not execute the consuming production subtraction/default decision. It also does not establish the exact accepted burn policy. The extraction fails byte identity and is not integrated.
- C2: the bounded asset-loss helper theorem timed out at the 120-second harness cap with both CaDiCaL and Kissat. Its token-to-asset conversion boundary is not proved. Its extraction also fails byte identity.
- B3: full-width agreement between the two actual threshold implementations compiled, but timed out at the same cap with both solvers. This is unproved, not a disproved theorem.
- F1/F2/F3 scalar candidates passed respectively 33/141/35 checks. Their extractions fail byte identity. F2 additionally proves partition arithmetic with harness-selected callbacks, not the production choice of floor versus ceiling; the passthrough extraction is rejected as insufficient linkage.
- C3 remains unproved. A preliminary lane inventory incorrectly said that no setoff avoids default: for positive debt it instead leaves residual debt and introduces a default. Full repayment, not absence of setoff, avoids a new default for that liability. The original inventory is retained with an explicit correction, not silently rewritten.
- D1/D2/D3/E1/E2 remain unproved at the required production-linked scope. No copied conversion wrappers or interest model were added to manufacture successful results. The direct Env feasibility result is recorded separately below.

The independent review rejected the candidate as meeting the core plan. No integrated regression gate, final pre-commit review, or third commit is claimed: the production-extraction gate failed first. The existing two commits, no-publication boundary, and separate release/custody gates remain intact.

### Direct Env boundary

The real `Env::default()` plus real `SorobanFixedPoint` arithmetic probe compiled with test support and no stubs, but timed out while CBMC recursively expanded Soroban XDR destruction paths. A second isolated probe used safe `core::mem::forget(e)` only to exclude final Env destruction; it still timed out expanding internal XDR drop paths. Neither run disabled safety checks or counted a partial unwind as success. Both used the 120-second per-harness cap; compilation and process teardown made total wall time longer.

This establishes failure to complete these particular probes under the recorded bounds—not that every possible Soroban-host Kani proof is impossible. No full Env, actual Reserve conversion, interest, or I256-fallback proof is claimed.

### Retained evidence and unchanged checkout

All 148 baseline tracked files and HEAD `ffc365aa7f4ec5ca518952313fc9e60ea5f1c65d` were checked unchanged after the experiments. No third commit, push, worktree, deployment, or production source integration occurred.

The source snapshots, predictions, raw proof logs, terminal receipts, independent review/corrections, build discriminators, and six-artifact ABI comparisons are frozen at:

`/home/common/.local/share/templar-proof-evidence/blend-kani-20260914-d_iw7rpr/evidence.tar.gz`

Archive SHA256: `9c64a6333ea74383ccc3c6f30158e29a4c0bb371e74f64abfcee18311161fca8`. The adjacent `manifest.json` binds all 997 members; each member was read back from the archive and its hash verified. The frozen ledger predates only this archive-locator section, avoiding a self-referential archive hash.

The archive intentionally contains rejected, timed-out, and invalidated attempts alongside successful ones. Filenames and disposition records distinguish them; existence in the archive is not acceptance. Working copies remain isolated for an explicit user decision on the blocking artifact constraint.

## Approved amendment: permit byte-changing extractions

The user subsequently selected **Permit byte-changing extractions**. This relaxes only whole optimized-Wasm byte identity. ABI identity, unchanged business rules, pinned dependencies/toolchains, the agreed required proof scope, native/Wasm/differential verification, and the separate local third-commit boundary remain binding. No publication or deployment is authorized.

The first attempt and its failures remain frozen above. The revised candidate uses one two-operation, statically dispatched arithmetic interface (`floor(x*y/z)` and `ceil(x*y/z)`). Its production implementation delegates to the existing SorobanFixedPoint implementation, retaining I256 fallback. The proof-only implementation delegates to the same dependency's checked FixedPoint operations. Shared lending kernels—not caller-selected arithmetic callbacks—choose the operands and rounding directions. This small interface is justified by the observed direct-Env probe failures and the rejected passthrough proof linkage; it introduces neither a runtime dependency nor a hierarchy.

The actual settlement residual calculation must be inside the verified production-delegated kernel. The exact burn policy, not just caps, must be asserted independently. Original deliberately bounded proof domains are retained; changing solvers or removing unrequested extra theorems must not silently narrow required coverage.

## Revised candidate: incomplete proof portfolio

The byte-change amendment was exercised, but **no third commit is accepted or
created**. The final isolated candidate has **22 of 32 required harnesses
verified**, with each successful receipt bound to the complete frozen Rust
source inventory. Every reported cover must also be satisfied. The remaining
obligations stay required:

| ID | Unproved obligation |
| --- | --- |
| C1 | Exact same-reserve setoff policy, repayment and residual split |
| C2 | Exact supplier-loss rate, bounds and zero-supplier preservation |
| D1-3 / D1-4 | Inverse b-token and d-token rounding |
| D2-1 / D2-2 | Withdrawal caps and repayment/refund split |
| D3-1 | Utilization range and branches |
| E2-1 / E2-2 | Nonnegative debt accrual and supplier/credit allocation |
| E1-boundaries | Interest-rate boundary values |

The preceding revised-source runs of those ten harnesses timed out under their
300-second harness caps with calibrated Z3 integer blasting. They are not
counterexamples and are not successful proofs of the final snapshot. The final
snapshot is separately frozen after the reviewed oracle simplification and
private accrual forwarding-method removal.

Further C2 diagnostics did not close the gap: an independent i128
quotient/remainder oracle timed out at 300 seconds; exact-oracle, rate-bound and
strict-interior-cover property slices each timed out at 120 seconds; fixed
denominator 1 and 255 probes also timed out at 120 seconds. The partition probes
are diagnostic subdomains, not replacement acceptance domains. No arithmetic
substitution, omitted safety check, narrowed required theorem or failed cover
was promoted into acceptance.

Post-run cleanup found 17 Z3 children orphaned by earlier Kani timeouts. Each
was verified against this campaign's scratch directory, run tag and orphaned
parent state before termination through a retained process handle. Earlier
timeouts remain unproved, but their timings are not controlled,
contention-free performance evidence.

The scratch runner now owns each Kani invocation in a separate process group
and terminates remaining group members when that invocation ends. An actual
two-second Kani timeout smoke left no live solver. A subsequent final-source C2
attempt, with the original domain and 300-second cap, also timed out. Its live
Z3 child was observed during execution; neither Z3 nor CBMC remained afterward.
This closes the observed timeout-cleanup defect, not the loss-rate theorem.

The shared threshold prefix now has a full nonnegative-i128 safety proof with
four satisfied covers, and the signed-universal threshold suffix proof has six.
Prefix equivalence is established by shared production source and pinned scale
constants, not by claiming the earlier timed-out equality miter passed.

### Revised regression and review evidence

- Pinned Rust 1.81.0 / Stellar 22.6.0 production build succeeded.
- All three optimized contract ABIs match the baseline.
- Pool optimized Wasm changes from 54,368 to 54,502 bytes, permitted by the
  amendment. Backstop and pool-factory optimized Wasm remain byte-identical.
- The final candidate's native/Wasm workspace run passed 495 tests.
- Its explicit differential run passed and detected all 11 negative controls.
- The forward b-token floor-to-ceil mutation was detected by the retained Kani
  negative control. Its proof, fixture and arithmetic support were mechanically
  matched to the final candidate; this is a rounding-linkage check, not proof of
  every lending obligation.
- Independent final source and Ponytail review found no blocking issue in the
  reviewed changes. Source approval does not discharge unproved harnesses.
- Additional generated runs are not included in the 495-test or formal-proof
  counts. In particular, the earlier 10,000-case log has no retained terminal
  success and its transient service metadata is no longer available; it remains
  excluded from acceptance.
- The additional revised-source 100-case run (seed 9110008) reached its
  3,600-second outer deadline without a terminal success. Its captured output
  is retained, but neither individual progress messages nor elapsed runtime
  are accepted as completion.

The baseline HEAD remains
`ffc365aa7f4ec5ca518952313fc9e60ea5f1c65d`; all 148 original tracked files remain
byte-identical. Only this untracked execution ledger is in the live checkout.
The production candidate and diagnostics remain isolated. No push, worktree,
deployment or release approval occurred. Completion still requires successful,
source-bound coverage of every required obligation and the documented runnable
proof-suite entry point; neither a partial proof commit nor a scope reduction
is authorized by this record.

### Revised durable checkpoint

The revised checkpoint is frozen separately at:

`/home/common/.local/share/templar-proof-evidence/blend-kani-20260914-d_iw7rpr/evidence.r2.tar.gz`

SHA256: `e49a8bc96ab87762b91d424ddb028fbe523e59f93578c258e7729ac8b6d0a06d`.
The adjacent `manifest.r2.json` binds all 5,314 members, and
`verification.r2.json` records complete archive-member readback, two identical
source-hash passes, and preservation of the original archive and manifest.
No earlier member path is missing from the revised snapshot; the one changed
member version is the scratch runner, whose earlier bytes remain in the
original archive.

This snapshot includes final proof sources, inventories, success and timeout
receipts, reviews, Wasm artifacts, the selected C2 diagnostic model, mutation
evidence, and solver-cleanup evidence. Compiler/verifier caches are excluded
except for explicitly retained artifacts. The r2 ledger stops before this
locator section; later follow-up records are versioned separately.

### Arithmetic-refinement follow-up

CBMC 6.8.0 source inspection identified one distinct representation option:
`--refine-arithmetic` activates its arithmetic-refinement loop with a capable
internal SAT backend. External SAT dispatch takes precedence, so combining
external Kissat with that flag silently runs ordinary BV solving instead.
Those external controls are excluded from refinement evidence.

The actual Kani calibration used `--solver minisat --cbmc-args
--refine-arithmetic`, with all existing checks retained. Both logs confirm
active `BV-Refinement`. The rounding mutation was detected (one of 68 checks
failed), but the positive checked-arithmetic control timed out at its
120-second cap. Intermediate UNSAT messages from the refinement loop are not
terminal proof verdicts.

This calibration is **NO-GO** for a lending attempt: no final C2 refinement
run was launched, no theorem domain changed, and formal coverage remains
**22 of 32 required harnesses**. No CBMC, Kissat or Z3 process remained after
the controls ended.

The 14-file follow-up is retained additively in
`/home/common/.local/share/templar-proof-evidence/blend-kani-20260914-d_iw7rpr/evidence.r3-supplement.tar.gz`,
SHA256 `cadbd615defc78bd1bd9796f1783d9621a84d72a5ab94b7d0b6480c0adb0171b`.
Its adjacent `manifest.r3.json` and `verification.r3.json` bind every member,
record complete readback and stable source hashes, and bind the unchanged r2
parent. The original archive is also unchanged. The supplemental ledger stops
before this locator paragraph.

### Longer-timeout follow-up: all ten required obligations

At the user's direction, all ten remaining harnesses were rerun sequentially
on the frozen final candidate with a **1,800-second per-harness timeout**
(previously 300 seconds). The calibrated Z3 integer-blasting configuration,
theorem domains and safety checks were unchanged. The batch completed all ten
attempts and exited 1; every attempt ended with `CBMC timed out`, not a
successful proof or a terminal semantic counterexample.

| ID | Obligation | 1,800-second result |
| --- | --- | --- |
| C1 | Exact setoff policy and debt split | Timed out |
| C2 | Supplier-loss rate and preservation | Timed out |
| D1-3 | Inverse b-token rounding | Timed out |
| D1-4 | Inverse d-token rounding | Timed out |
| D2-1 | Withdrawal caps | Timed out |
| D2-2 | Repayment/refund split | Timed out |
| D3-1 | Utilization range and branches | Timed out |
| E2-1 | Nonnegative debt growth | Timed out |
| E2-2 | Supplier/credit accrual allocation | Timed out |
| E1-boundaries | Interest-rate boundary values | Timed out |

The summed invocation time, including compilation and runner overhead, was
18,058.45 seconds. All 102 frozen candidate Rust files and all 148 baseline
tracked checkout files remained byte-identical. No CBMC, Z3, Kissat or
cargo-kani process remained after exit. Formal coverage remains **22/32**;
the proof commit remains blocked.

The additive durable run directory is
`/home/common/.local/share/templar-proof-evidence/blend-kani-20260914-d_iw7rpr/long1800-20260914T220343Z/`.
It retains preregistration, the exact ten-theorem inventory, runner and solver
wrapper, launch configuration, all ten complete logs, incremental result
receipts, and `summary.json`. The run's `manifest.json` binds its evidence
files; `verification.json` records their hash readback. Prior archives were
not rewritten.

## 2026-09-15: enumeration and composition closures

Four of the ten timed-out obligations were closed by two techniques: complete
finite-domain enumeration of the inverse-conversion domain, and guarded
parametric-caller composition over an unchanged production kernel.

- **D1-3 / D1-4 (inverse b/d rounding)**: the original closure record reports
  complete coverage of 255 rates (hundredths 1..255) × 256 amounts (u8),
  using symbolic rate partitions and finite enumeration, with a separate
  b/d inverse-equivalence proof. The original receipts remain under
  `/tmp/blend-kani-20260914-d_iw7rpr/recovery-20260915T080802Z/`.
  Their per-rate evidence, not a command count, determines closure.
  Negative controls: an insufficient-unwind run fails explicitly on the
  unwinding assertion; a ceiling-degraded mutation fails the exactness check.
- **D2-2 (repayment/refund split)**: closed as *parametric-caller composition*.
  Prerequisite real-unit proofs establish totality and nonnegativity of the two
  conversions on the caller domain. The composition harness runs the unchanged
  `plan_repay` kernel over a guarded `FixedMath` whose operand triples are
  asserted (not assumed) and whose stable results are arbitrary subject to
  `q >= 0 && u >= 0` only. PASS 144 checks, 3/3 covers (bundled Kissat;
  Z3 integer-blasting times out on this shape). Operand mutation
  (`amount + 1` in the burn triple) fails one semantic check.
- **D2-1 (withdrawal caps)**: same composition pattern for the unchanged
  `plan_withdraw` kernel. Premise set is exactly `q >= 0`, `y >= 0`, and the
  implication `q <= C ⇒ y >= A` on the uncapped branch only (backed by the
  separately passing source-linked round-trip-up theorem); a global `y >= A`
  is forbidden because capped requests legitimately exceed their claim. PASS
  113 checks, 2/2 covers (Kissat) in the original receipt. Both an unsound-premise run and a
  conjunction-form run are retained as invalid. Dropping the `min(q, C)` cap
  operand fails one semantic check.

### Correction: erroneous receipt-loss diagnosis

The earlier continuation searched `/tmp/blend-kani-20260914_d_iw7rpr`
instead of the actual `/tmp/blend-kani-20260914-d_iw7rpr`. It incorrectly
inferred a tmp cleanup and receipt loss from that path typo. The original
recovery sources, receipts, and closure records still exist. No evidence
supports the claimed wipe or its attributed disk-pressure cause.

The resulting reconstruction and duplicate proofs under
`/tmp/blend-kani-20260915-recovery` are supplemental work, not replacement
custody. Its full enumeration was cancelled after discovering the mistake.
The `incident-tmp-wipe-20260915.json` narrative is superseded by this
correction; retained supplemental logs are not additional portfolio closures.
Check-count differences alone do not establish equivalent assertion sets.

The original reported portfolio is **26/32** (22 baseline + D1-3, D1-4, D2-2,
D2-1). Remaining open: C1, C2, D3-1, E2-1, E2-2, E1-boundaries. The proof
commit remains blocked until they resolve or the user narrows scope.

The original evidence/source freeze is retained at
`/home/common/.local/share/templar-proof-evidence/blend-kani-20260914-d_iw7rpr/original-recovery-20260915-astra/`.
`manifest.json` binds 7,905 files; `verification.json` records successful
archive-byte verification. Extract `evidence.tar.gz`, then
`referenced-bytes.tar.gz`, then `compile-imports.tar.gz`: the supplements
materialize 11 historical receipt symlink targets and 87 compile-time
`contractimport!` WASM inputs at their original paths. The latter have a
separate `compile-imports-manifest.json`; `verification.r2.json` supersedes
the original restore instructions. Disposable build caches are excluded,
but the required import bytes are retained. Original scratch sources remain.

### Unchanged-source Kissat follow-up

All six remaining original `final-candidate` harnesses were tried with bundled
Kissat and an explicit 120-second per-harness cap. Each timed out
(122–127 seconds wall time); none produced an accepted proof or counterexample.
Source maps remained unchanged and all six retained log hashes were verified.
The durable freeze directory above also holds
`proof-results-astra-remaining-kissat-20260915.json` and
`astra-remaining-kissat-classification.json`. Coverage remains **26/32**.
Direct assertion/branch splits and reviewed arithmetic prerequisites are the
next experiments; their preparation does not close an obligation.

### Direct split results and arithmetic prerequisites

The source-preserving direct split wave completed 16 harnesses: **8 passed,
8 timed out** at the same 120-second solver cap. Successful subsets are
utilization zero/saturation, debt-rate monotonicity, zero accrual, positive
backstop-credit allocation, and interest upper/lower clamps and zero
reactivity. Each passed its declared reachability covers. Remaining interior,
delta, rate/accounting, and interest-rate boundary equalities are not closed.
`direct-splits-summary.json`, both result arrays, logs, and changed proof-source
files are retained in the durable freeze directory above. All log hashes and
run-time source pins were checked; no original obligation is promoted from a
partial conjunction. Coverage remains **26/32**.

The C2 forward-debt-to-original-u64 prerequisite passed 125 checks and both
covers; its four real-kernel branch witnesses passed 149 checks. The second
numeric prerequisite (loss ceiling versus the original u64 oracle) timed out.
The guarded supplier-loss caller therefore remains unexecuted and unaccepted.
Fixed-divisor/supply calibration is exploratory until every required finite
partition passes; canaries alone never close a full-domain obligation.

### Fixed-parameter calibration and unresolved dependencies

The first fixed-supply wave passed **11/12** canaries. Exact debt delta at
supply 255 timed out; the other two exact-delta canaries, three nonnegative
debt-delta canaries, and six supplier-rate/accounting canaries passed their
single local reachability covers. These are finite cells, not full-domain
proofs.

C1's six numeric prerequisites produced three passes: both forward u64
bridges and the low-amount debt inverse (131 checks, two covers). Both
collateral-inverse slices and the high-amount debt inverse timed out. Its
guarded caller remains blocked.

C2's fixed-supplier ceiling canaries at 1, 3, and 255 each passed 99 checks
and two endpoint covers. Full recovery requires all 255 supplier values,
followed by the guarded caller and its acceptance gates. The utilization
interior helper still timed out after both forward identities passed;
its composed caller remains blocked as well.

The durable freeze directory retains the setoff, fixed-supply, and
fixed-supplier result arrays, logs, and explicit classification records.
`astra-calibration-sources/manifest.json` binds the corresponding retained
calibration snapshots (including the utilization lane); compile-time WASM
inputs are the previously frozen identical bytes. Coverage remains **26/32**.

### Complete partition execution and selector correction

The utilization divisor canaries at 2, 3, and 650 passed 46 checks and two
covers each. Its full helper domain is the union of every divisor 2..650.
The supplier-loss helper requires suppliers 1..255; reserve nonnegativity
requires debt supplies 0..255; the two positive-accrual conjuncts require
supplier counts 1..255 each. Complete-union runs are in progress; no
original obligation is promoted from partial results.

The utilization and supplier-loss ceil-to-floor oracle controls each failed
a semantic equality as intended. The three reserve assertion/oracle
controls also failed their intended properties. These are proof-oracle
sensitivity controls, not production mutations.

C1's nine fixed-rate canaries passed 131 checks and two covers each. Its
three unresolved inverse slices still require all 255 rates apiece.
The debt numerator literal passed 74 checks and four covers; the universal
product bridge timed out, while fixed-x canaries at 0, 1, and 255 passed
69 checks and two covers each. The bridge still requires every x in 0..255
before the exact-delta composed caller may execute.

An execution-selector defect was found before supplier-union acceptance:
Kani's default substring filter selected `supplier_4` and `supplier_40`
through `supplier_49` together. The retained runner rejected that
eleven-harness result as a single-cell receipt. Those runs were stopped
and superseded, not counted. A separate runner variant changes only the
selector to the fully qualified name with `--exact`; its supplier-4 smoke
passed 99 checks, both covers, and exactly one harness. Subsequent
unpadded-name partition runs use this exact selector.

`astra-reviewed-union-snapshots/manifest.json` in the durable freeze binds
the source overlays to the frozen candidate; retained result arrays and
logs preserve passes, timeouts, and superseded selector runs separately.
Coverage remains **26/32** until complete dependency chains and guarded
callers pass.

### Utilization helper completion and remaining caller work

The complete utilization helper union passed **649/649** divisor cells
(`P = 2..650`), each with 46 checks and two local endpoint covers. Its
source-bound receipts and logs are retained in `astra-util-helper-closure.json`
in the durable freeze. This closes the helper domain, **not D3**:
the guarded caller retaining real forward conversions timed out at the
120-second solver limit. Its operand-mutation control failed one semantic
assertion as intended. A fully parametric successor must still prove the
actual call sequence and retain concrete real-arithmetic reachability witnesses.

For interest boundaries, the rate-identity prerequisite passed 35 checks and
both endpoint covers. Universal cancellation and scaled-base prerequisites
timed out; neither is accepted. The cancellation successor covers the exact
original caller image `a = 100000 * K`, `K = 1..94`, with asserted applicability.
It does not claim the wider universal cancellation theorem. Scaled-base
canaries at modifiers 1, 40, and 100 passed 57 checks and both local endpoint
covers; all 100 modifiers remain required. A direct nonexact arithmetic witness
passed 60 checks and its cover. The isolated equality mutation failed one
semantic assertion as intended.

Review rejected misplaced cancellation wrappers and a debt witness whose
constant predicates were disconnected from its arithmetic inputs. Those
unexecuted candidates were corrected before acceptance. The corrected debt
witness executes both real ceiling calls and passed 75 checks and one cover.
Historical timeouts and superseded source reports remain distinct from accepted
receipts. Coverage remains **26/32**; no proof commit has been packaged.

### Accepted C1 and D3 composition closures

The independent composition reviewer accepted **C1** and **D3-1**, bringing
the unique original-obligation tally to **28/32**. Remaining: C2, E2-1,
E2-2, and E1-boundaries. Dependencies are not additional portfolio obligations.

- **C1:** all 765 fixed-rate arithmetic cells plus the other source-linked
  arithmetic prerequisites passed. The original exact-policy guarded caller
  passed 163 checks and all five branch covers. Five concrete real-arithmetic
  witnesses passed 249 checks. The isolated first-operand mutation failed one
  semantic assertion (164 checks), leaving both bypass covers reachable.
  `astra-setoff-closure.json` retains the full dependency chain.
- **D3-1:** the real zero-liability and saturation branches, both forward
  conversion bounds, all 649 interior divisor cells, and three concrete
  original-input witnesses support the final caller: 166 checks, three covers.
  Its isolated operand mutation failed one semantic assertion (167 checks).
  `astra-utilization-closure.json` records **two-stage output-parametric
  composition**: the caller forwards a stable guarded quotient; the separately
  proved real helper supplies equality to the original literal ceiling oracle.
  This is not a standalone original-numeric-oracle proof. The old
  `astra_utilization_ceil_assets` timeout is explicitly replaced by the complete
  helper union, never counted as passed.

Both closures retain the unchanged original bounded domains and checked-i128
product-fit scope; neither proves the Soroban host or I256 fallback. The durable
`original-recovery-20260915-astra/` freeze retains both closure records and logs.
Historical standalone timeouts and superseded numeric-stage pending markers
remain contextualized rather than overwritten. No integrated-tree proof run
or proof-commit acceptance is claimed.

### Accepted C2 and E1 boundary closures

Independent review accepted **C2** and **E1-boundaries**, bringing the unique
original-obligation tally to **30/32**. E2-1 and E2-2 remain open.

- **C2:** the complete 255-supplier arithmetic partition, forward exactness,
  real arithmetic witnesses, and guarded caller establish exact saturated
  loss, rate bounds, and zero-supplier preservation. The caller passed 163
  checks and all four covers; its operand control failed one of 164 checks.
  `astra-loss-closure.json` binds the dependency chain. An initial launch used
  a descriptive object instead of the executable array inventory; it failed
  before Kani and is retained as an inventory failure, not a proof result.
- **E1-boundaries:** all six original assertions are covered: three guarded
  interest-boundary callers (290, 292, and 290 checks; one cover each) and a
  real-arithmetic proof of the original upper clamp, lower clamp, and
  zero-reactivity assertions (187 checks; one cover). Dependencies include all
  94 cancellation-image cells, the rate-identity theorem, and all 100 modifier
  cells. The caller operand control failed one of 291 checks.
  `astra-interest-closure.json` records the complete six-conjunct mapping.

These remain source-linked, bounded checked-i128 composition closures, not
Soroban-host or I256 theorems. Historical standalone and numeric-stage timeouts
are retained. Reversible macro compaction removed 11,438 repetitive lines from
the isolated integration candidate without changing the expanded proof
bodies; that candidate still requires fresh execution and acceptance gates.
No third commit, publication, or deployment is claimed.

### Accepted E2 closures and integrated-tree execution

Independent review accepted **E2-1** and **E2-2**, bringing the unique
original-obligation tally to **32/32**.

- **E2-1 (nonnegative debt growth):** two-stage output-parametric composition.
  The caller `astra_grow_debt_output_parametric` passed 84 checks with all
  four covers, four real boundary witnesses passed, and the operand control
  failed one of 203 checks. Rate monotonicity transfers directly from the
  full-domain symbolic PASS `astra_grow_debt_rate_monotonic` (4/4 covers) over
  the identical original domain; nonnegativity holds in every one of the
  256/256 fixed-supply cells on the real `grow_debt`. `astra-debt-closure.json`
  binds the chain with explicit transfer statements.
- **E2-2 (supplier/credit accrual allocation):** source-linked finite-domain
  partition, same law as D1-3. The zero-accrual branch (2/2 covers) and the
  exact credit split (4/4 covers, floor formula, zero-take branch,
  `expected_credit <= accrued`) pass fully symbolically; the exact `b_rate`
  update formula and the no-value-minted accounting bound
  (`gap * SCALAR_12 < b_supply + SCALAR_12`) are proven in all 255+255
  fixed-divisor cells with every other input symbolic. `astra-accrue-closure.json`
  records the closure.

### Integrated-tree fresh execution and packaging

The integrated candidate (`astra-integrated-ready`, including the restored
`c2_s0_d_forward_exact_u64` body and the restored enumerative
`inverse_partitions.rs`) was freshly executed against the full portable
inventory: **3197/3197 harnesses PASS** under Kissat with source digests
unchanged around every harness and every expected cover satisfied. One
harness, `independent_probe_b_d_inverse_equivalence`, exceeded the 120s
campaign default (macro-compaction symex blow-up: 13.6s on its source lane,
~915s integrated) and passed under a documented 3600s budget. Post-repair
integration gates passed on the same tree: 495 native workspace tests, ABI
equality on all three contracts against the frozen candidate rebuild, and the
exact differential with every negative control detected.
`proof-packaging/integrated-execution-receipt.json` is the execution receipt;
`proof-packaging/inventory.json` (3197 entries), the provenance and
obligation maps, and the accepted closure records are committed alongside.
Raw shard logs and results dumps remain outside the repository.

Scope remains bounded checked-i128 composition over unchanged original
domains; no Soroban-host or I256 theorem is claimed, and the historical
1800s standalone timeouts are retained as history.

## 2026-09-22: rebase onto the current security fork

The Kani-specific commit was replayed onto
`54afdae01bb8e484514940229e06460aa3fa6a1f`. The sole conflict retained the
current fork's `debt_setoff` event and routed its burn/repaid/default decisions
through the proof-facing `supply_setoff` result.

Validation on the rebased tree:

- `devenv shell -- make test`: 380 passed, one ignored.
- The three conflict-local portable inventory harnesses passed with every
  expected cover satisfied and unchanged Rust source digests.
- `prove_setoff_exact_policy_split_and_conditions`: 124 checks passed, two
  unreachable properties, and 5/5 covers satisfied under Kissat.
- `cargo fmt --all -- --check`: passed.
- Pool contract interface: byte-identical to the current fork interface
  (`7f7a946a…`); optimized pool Wasm changed from `2db04d55…` to `0594a17e…`.
  Backstop and factory optimized Wasms remain byte-identical.

`proof-packaging/rebase-validation.json` binds the commands, source digests,
artifact hashes, and external transcript hashes. The earlier 3197/3197 result
remains historical evidence for the pre-rebase tree; this rebase reran the
conflict-local proof surface rather than relabeling that full result.
