# Local verification strengthening

The seven north-star claims are backed by the machine-checked results and stated
scopes in [`VERIFICATION.md`](../VERIFICATION.md). This campaign strengthens that
evidence with universal integer lemmas, reachable zero-debt accrual, checked
contract consumption, and local oracle validation and decimals-cache proofs.
The existing results retain their stated scopes and assumptions; the sections
below distinguish the new verified additions from further strengthening work.

Runtime behavior, ABI, storage, runtime dependencies and rounding are unchanged.
No new hosted Certora jobs were submitted. Published and new results are recorded
with their respective source/tool identities and reproduction commands.

## Acceptance and prerequisites

From the repository root:

```sh
make verify-local
```

The local acceptance inventory rechecks the verified additions below. The broader
Kani and Certora proof portfolio remains documented in `VERIFICATION.md`.

| Verified addition | Passing result | Proved scope |
| --- | --- | --- |
| Zero-debt accrual | One Kani harness, 4/4 covers | Actual `grow_debt` and `accrue` kernels on the documented checked-i128 grid |
| Supply-chain contract and checked caller | Two Kani harnesses, 3/3 and 2/2 covers | Existing supply-chain arithmetic transcription and grid, with a verified callee dependency |
| First-read price guard | One Kani harness, 8/8 covers | Unchanged production `Pool` guard with modeled host interfaces |
| Decimals validation, cache and invocation reset | One Kani harness, 4/4 covers | Unchanged production `Pool` validation/cache code with modeled host interfaces |
| Integer supply/withdrawal mathematics | Nine Verus proof functions, zero errors | Three unbounded-integer theorems, four quotient helpers and two witness functions |
| Floor-to-ceil mint mutation control | Both intended assertion failures detected | Demonstrates rejection of the incorrect mint rounding in a sandbox copy |

`verification/local-proofs.json` fixes exact Kani selectors, deadlines, cover counts
and dependency order. A caller cannot be accepted without its verified callee
preceding it. Kani must finish exactly one selected harness with `SUCCESSFUL` and
all required covers; process exit zero is insufficient. The authoritative parser
is `scripts/kani_release.py::parse_log`. Verus must report the exact nine successful
function names, full-crate verification, zero errors and the pinned verifier commit.
The mutation must fail in `supply_delta` and `ceil_mint_counterexample`, not merely
exit nonzero on a syntax/tool error. Both runs use `--no-cheating`, one solver thread
and a 300-second deadline.

Requires Linux, Python 3.11+, the repository Rust/build prerequisites, Kani
**0.68.0** (CBMC 6.11.0), and a user systemd manager enforcing cgroup v2. Existing
optimized Wasm imports must be present; `make build` prepares them. Acceptance
runs the three Python regression modules, the five Kani harnesses and Verus. It is
**not** the full workspace suite, every historical harness, a deployed-Wasm proof
or a release gate. Logs are under `target/verification-local/`.

The Verus bundle defaults to `/tmp/blend-verification-tools/verus`; `VERUS_HOME`
may select the same directory layout elsewhere. Immutable downloads, commits,
archive/binary/frontend-library hashes and isolated Rust setup commands are in
[`verification/verus/toolchain.json`](../verification/verus/toolchain.json):

- Verus `0.2026.10.23.dccae1d`, commit `dccae1df5fcbe663628af781c8b211123ad9d928`;
- bundled Z3 4.16.0; bundled `vstd` and the four loaded frontend libraries;
- isolated Rust 1.99.1, commit `58cf8f9c35b355199727627d07ae34a4e2846d67`.

Provision the hash-checked release archive and pinned Rust components explicitly
before acceptance. Keep `RUSTUP_HOME` and `CARGO_HOME` in that sandbox and perform
installation under the same resource limits. The runner checks hashes and compiler
identity; it never installs missing tools. Rustup automatic installation is disabled
in the runner and workers (`RUSTUP_AUTO_INSTALL=0`). The extraction driver also
disables Charon's fallback installer. Tools installed for this campaign stayed in
the approved sandbox; no system-wide prover installation was performed.

### Containment and failure controls

Every prover/build subprocess is serialized under the shared lock and checks its
actual cgroup: 4 GiB memory, zero swap, 256 tasks, CPU at most 200%, group OOM
killing. Systemd enforces deadlines and `KillMode=control-group`; interruption
stops the unit, and inability to establish cleanup fails closed. No uncontained
fallback is allowed. These are operational controls, not contract theorems.

Exercised controls include:

- An impossible fifth cover: Kani exited zero and reported SUCCESSFUL with 4/5
  covers; acceptance rejected it. The mutation was removed. The parser regression
  distinguishes missing witnesses from a semantic failure.
- Worker invocation outside the required cgroup: rejected before its command.
- A missing toolchain in a fresh sandbox failed immediately with `not installed`;
  no toolchain directory was created. No download/install fallback ran.
- A one-second deadline killed a 30-second sleep. SIGTERM stopped a real worker
  and sleeping descendant; neither remained running.
- Invalid selectors, deadlines, row types and dependency ordering: rejected before
  proof launch. Altered verifier/compiler components and stale/conflicting LLBC
  receipts are covered by fail-closed regression fixtures.

### Observed checkpoint checks

- `make verify-local`: PASS; 13 Python regression tests, five Kani harnesses with
  all 21 covers, nine Verus functions, and both required mutation failures.
- Reviewed Charon and Aeneas reproduction: both exit zero, matching Wasm target and
  source hashes; extraction/translation only, not a Lean theorem.
- `python3 scripts/verify_local.py --seconds 900 -- env CARGO_BUILD_JOBS=1 cargo test -p pool math::tests::rounding_keeps_i256_fallback -- --exact`:
  one test passed, exercising the real production `Env` dispatch on an
  overflowing-i128 product with representable floor=2 and ceil=3.
- `cargo fmt --all --check`: PASS. Selected proof/experiment/local-host files also
  passed `rustfmt --edition 2021 --config skip_children=true --check`.

These local checks were executed at checkpoint `2d9e3e0`; they did not rerun the
full workspace suite/build or submit hosted proofs. Existing pool compilation
warnings were retained, not suppressed. Subsequent PR CI results are tracked
separately on [PR #39](https://github.com/Templar-Protocol/precept/pull/39).


## Verified additions

### Reachable zero-debt accrual

`pool::reserve::verification::prove_accrual_conserves_value_debt_000` proves zero
accrued debt and unchanged supplier claims, supplier shares/rate, debt shares and
backstop credit. The debt rate may increase without creating aggregate debt value.
Four covers distinguish empty supply, positive supply at zero supplier rate,
positive claims and increased debt rate. This replaces the impossible
positive-growth witnesses; their historical 0/2 outcome remains in `docs/kani.md`.

Domain: debt shares zero; supplier shares and previous credit 0..255; supplier rate
0..2.55 Q; debt rate/growth factor 1..2.55 Q, in hundredths; Q=1e12. The unused
zero-accrual take argument is arbitrary u32. Initial verified run: 30.20 seconds
solver time, 75.28 seconds service runtime, 1.6 GiB peak, zero swap. This establishes
the zero-debt kernel result on that domain. Actual storage initialization and
production I256 refinement are separate strengthening goals.

### Checked local contract consumption

The proof-only `local-contracts` feature gives the existing `supply_chain`
transcription a checked contract. Its callee proof covers positive operations below,
at and above unity. The existing supply-cap caller consumes that verified contract
with `-Z function-contracts -Z stubbing`; both caller branches have witnesses.
Balances/amounts remain 0..255 and rates remain the original hundredths grid.
Both the callee contract and its use by the cap-proof caller are verified on
that domain. Applying the result through production supply admission, persistence
and utilization is a separate source-refinement step. The cap is an admission
check; interest may legitimately raise valued supply above it later.

### Source-relative oracle guards

`verification/local-host/lib.rs` includes the original
`pool/src/pool/pool.rs`; the guard/cache algorithm is not copied. The host model
supplies invocation-constant u64 time, arbitrary i128/u64 price data, arbitrary u32
decimals, feed failure and exact key equality (address kind plus full 256-bit
payload). Host aborts/external-call divergence terminate the path; required covers
ensure the rejection and success cases are reachable.

The first-read rule covers feed failure, absence, zero, negative, future, stale,
exactly 86,400 seconds old and positive-now responses. The decimals rule rejects
values other than seven, proves that a cached seven ignores a later failing feed,
and requires a fresh `Pool` to validate again. These local results add to the
existing Certora breaker rules. A separate price-key cache/reset extension timed
out at 180 seconds and is excluded from the local acceptance set.

The two-slot map model covers the stated local footprint. Arbitrary cache-history
induction is further work; the proofs use explicit SDK/host, address-equality and
transaction-time contracts rather than establishing host implementation or
market-price accuracy.

### Universal supply/withdrawal mathematics

`verification/verus/supply_delta.rs` proves, for arbitrary nonnegative integer
balances/amounts and positive integer rates:

- floor-minted supply increases aggregate floor-valued claims by between zero and
  the assets received;
- an uncapped ceil-burn withdrawal releases at least the assets paid, given enough
  shares;
- a capped floor-valued payout is bounded by aggregate claims released.

No magnitude limit or hundredths grid is imposed. Concrete inhabitants include
below/at/above-unity and non-grid rates, zero amounts, empty supply and nonzero
changes. The negative control changes floor minting to ceil: B=1, R=1.5 Q, A=1
increases claims by two for one asset received. Both the general supply bound and
that intended false concrete bound must fail. Unrelated diagnostic witness code is
removed only from the sandbox mutant so an earlier computation failure cannot
masquerade as this negative control.

These three theorems establish the rounded-division bounds without the previous
grid restriction. Connecting them to production Rust's checked-i128/I256
implementation is the next arithmetic refinement step.

### Production arithmetic extraction

`verification/extraction/run.py` extracts the real six `ReserveData` conversions,
their `Reserve` wrappers, actual `Env` dispatch and locked
`soroban-fixed-point-math` 1.5.0 checked-i128/I256 fallback branches. Production
source/dependencies are not rewritten. Charon and Aeneas successfully generated
LLBC and Lean definitions: 23 transparent functions and 22 opaque functions.
Removing Aeneas's crashing unknown-name **reporting** flag enabled translation;
no target semantic check was disabled.

The completed result here is source extraction and translation. A Lean-checked
arithmetic refinement theorem remains the next step: generated external templates
expose SDK I256/Env, unwrap, checked-remainder and Option/control-flow obligations.
Those templates are not accepted axioms or committed implementations, and no Lean
proof/cache restoration was run. Charon/Aeneas/compiler/Lean pins and diagnostics are in
[`verification/extraction/evaluation.json`](../verification/extraction/evaluation.json).
Creusot/Why3 was not installed: extraction succeeded, so its approved fallback
condition was not met.

The reviewed driver validates tool/compiler identity without implicit installation.
Aeneas requires a successful Charon receipt with matching source/LLBC hashes and
actual machine target, inherits that target unless explicitly matching it, and
consumes an immutable snapshot. Existing output directories and stale/conflicting
receipts are rejected. Every invocation records source hashes before/after, tool
and dependency hashes, generated hashes and exact command. Reproduce feasibility
separately; it is intentionally outside the accepted theorem inventory:

```sh
python3 verification/extraction/run.py charon --output /tmp/blend-verification-tools/extraction/results/wasm-seam-new
python3 verification/extraction/run.py aeneas --llbc /tmp/blend-verification-tools/extraction/results/wasm-seam-new/pool.llbc --output /tmp/blend-verification-tools/extraction/results/wasm-lean-new
```

## Attempt ledger and stop outcome

The campaign reached its **20-invocation cap**; no further approaches were tried.
The limit is conservative: compilation/witness repairs and negative controls count
in the ledger. Unchanged successful acceptance reruns and infrastructure checks do
not add approaches. At most three approaches per obligation were allowed; stop
an obligation after 60 minutes without verified progress. No timeout increase,
numeric narrowing, trusted target stub or production rewrite was used to turn an
undecided result into a pass.

| Attempt | Target / approach | Observed outcome |
| --- | --- | --- |
| 1 | Real zero-debt kernels and meaningful witnesses | PASS, 4/4 covers |
| 2 | Verus integer lemmas and concrete inhabitants | Main lemmas passed; witness preconditions failed |
| 3 | Same mathematics, explicit burn computations | Witness postconditions failed |
| 4 | Same mathematics, expanded concrete computations | Non-grid witness postconditions failed |
| 5 | Computed witness context, unchanged symbolic domains | PASS, 9/9 proof functions |
| 6 | Source-including oracle model | Compile failure: missing `crate::errors` import |
| 7 | Import repaired, unwind four | Fixed-byte comparison unwinding assertion failed |
| 8 | Required unwind 34, initial oracle prototype | PASS, price 7/7, cache 1/1, decimals 3/3; historical prototype, not final cache claim |
| 9 | Ceil-mint negative control | Earlier diagnostic computation failed, not accepted negative evidence |
| 10 | Select concrete mutant obligation | Same unrelated computation failure |
| 11 | Remove diagnostic-only witness from mutant | Intended general and concrete assertion failures detected |
| 12 | Exact production arithmetic seam, Wasm release | Charon PASS; LLBC `has_errors=false` |
| 13 | Translate with unknown-external-name report | Aeneas crashed formatting tuple names |
| 14 | Actual initializer/storage base case with testutils | Kani compiler ICE before verification; no base case established |
| 15 | Remove only crashing reporting flag | Aeneas translation PASS; source bridge still unproved |
| 16 | Checked local function-contract pilot | Compile rejection: explicit `-Z stubbing` required |
| 17 | Enable checked-contract/stubbing mode | Callee PASS 3/3, caller PASS 2/2 |
| 18 | Actual borrow/repay arithmetic prerequisites | Both timed out at 600 seconds; covers undetermined |
| 19 | Full address kinds, feed failure, fresh decimals | Price PASS 8/8, decimals PASS 4/4; expanded cache timed out at 180 seconds |
| 20 | Equivalent full address equality as two words plus kind | Price PASS 8/8, decimals PASS 4/4; cache still timed out at 180 seconds |

## Established results and further strengthening

The established proofs remain valid within their documented domains, targets and
scopes. The final column lists additional coverage sought by this campaign;
compiler failures and timeouts describe those pilots, not failures of the
established results.

| North-star claim | Established results and verified additions | Further strengthening / pilot outcome |
| --- | --- | --- |
| Solvency | Reserve-kernel preservation bounds for supply, uncapped withdrawal and accrual; the zero-debt case now has reachable witnesses. | Add cash, pending transfers, global position totals and the remaining transitions to the induction. Actual initializer: compiler ICE; borrow/repay prerequisites: 600-second timeouts. |
| No free money | Existing conversion/round-trip proofs, plus unbounded supply and uncapped/capped withdrawal lemmas. | Refine production SDK/I256 arithmetic and compose actual handlers/net transfers. Extraction succeeded; the refinement theorem remains to be proved. |
| Interest accrual | Existing monotonicity and conservation results, plus repaired zero-debt conservation. | Extend the remaining arithmetic domains and compose actual caller/storage frames with cash-backed accounting. |
| Health monotonicity | Existing Kani proofs establish ratio/health arithmetic monotonicity on their stated domains. | Strengthen the separate health-execution rule and its builder/health summaries, then compose position valuation and collection induction. These extensions were not reached before the cap. |
| Supply/utilization caps | Existing cap/utilization results on their stated scopes, plus the checked supply-contract/caller pilot. | Extend through actual admission, cache/persistence and the utilization loop. Admission is distinct from a permanent cap on interest-grown supply. |
| Liquidation | Existing auction pricing/timing and bad-debt policy proofs. | Compose partial/full fills, account updates, transfers, residual default and orphan settlement. These extensions were not reached before the cap. |
| Oracle circuit breaker | Existing Certora 13/13 breaker rules, plus local first-read and decimals validation/cache/reset proofs. | Extend price-cache key/provenance/reset and arbitrary-history coverage. The expanded two-key price-cache pilot timed out at 180 seconds. |

Reproducible unproved probes remain explicitly segregated in
`verification/experiments/`. `initializer.rs` selects
`pool::config::verification::initializer_experiment::prove_actual_initializer_base_case`
with `--features testutils`; the observed Kani ICE was
`kani-compiler/src/intrinsics.rs:243:17`, expecting an i32 output type. The
`accounting` module under `pool::actions::verification` contains
`prove_borrow_aggregate_debt_delta` and `prove_repay_aggregate_delta_and_refund`.
Each had three required covers but neither reached a verdict within 600 seconds.
The oracle cache probe lives in the standalone local-host crate and has two
required covers. These unfinished pilots are recorded separately from the passing
acceptance set.

All probes can be launched with `python3 scripts/verify_local.py --seconds N --`
followed by the exact `cargo kani` selector, using the same containment. Existing
logs preserve failures/timeouts; do not infer a counterexample from a timeout.
Compiler/prover/extractor soundness, explicit host/token contracts, ledger time and
transaction atomicity remain explicit trust premises. Release and deployment
readiness require their own validation beyond the proof results documented here.
