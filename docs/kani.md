# Kani lending-kernel verification

This PR keeps the code that is reviewed and merged: production arithmetic seams and their `#[cfg(kani)]` proofs. Campaign inventories, runners, receipts, and generated closure data are published as release evidence instead of living in the merge diff.

## Toolchain

**Kani 0.67.0 is required.** Not a preference — the suite does not build under
0.68.0.

| | ethnum 1.5.0 | ethnum 1.5.3 |
|---|---|---|
| Kani 0.67.0 | **builds** | — |
| Kani 0.68.0 | `E0512` in `ethnum` | builds |
| Production on Rust 1.81 | builds | `from_str_radix` not const-stable |

`ethnum` reaches the build through `soroban-env-common` →
`soroban-sdk-macros`. Kani 0.68.0 bundles a newer rustc that rejects
`ethnum` 1.5.0's `mem::transmute(())` with `E0512`, so every harness fails
before compiling. Bumping to `ethnum` 1.5.3 fixes that but needs a const
`from_str_radix`, which is not stable on the `rust-toolchain.toml` pin of
1.81, so it breaks the production build instead. There is no version of
`ethnum` that satisfies both, and `devenv.nix` already records the same
`E0512` problem for building `stellar-cli`.

Pinning Kani to 0.67.0 resolves it without touching the lockfile or the
production toolchain, and it is the version the published campaign ran under
(`integrated-execution-receipt.json`: "kissat (Kani 0.67.0)").

```sh
# Run from OUTSIDE this repository, or with a newer cargo: kani-verifier
# requires edition2024, which the repo's pinned cargo 1.81 cannot parse.
cd /tmp && cargo install --locked kani-verifier --version 0.67.0 --force
cargo-kani setup   # only if ~/.kani/kani-0.67.0 is absent

cargo kani --version   # expect cargo-kani 0.67.0
```

Verified on this tree: `cargo kani -p pool --lib` compiles the crate,
`ethnum` included, in about 28 seconds.

## Tracked proof surface

- Runtime code remains in the ordinary `pool/src` and `backstop/src` modules.
- Proof bodies live under each crate's `src/proofs/{module}.rs` behind `#[cfg(kani)]` (or the existing `#[cfg(all(kani, test))]` status gate).
- `pool/src/proofs/inverse_partitions.rs` is proof-only exhaustive partition code.

The split preserves module paths such as `pool::reserve::verification::*`; the released inventory selectors therefore still address the current proofs. Proof modules remain children of their production modules and require no broader production visibility.

## Runtime boundary

Kani calls the same helper logic used by production. The runtime-facing changes are limited to:

- the `FixedMath` seam and `Env` implementation;
- reserve, interest, action, auction, user, and bad-debt arithmetic kernels;
- the shared saturating backstop-threshold calculation; and
- module wiring and Kani-only re-exports needed for cross-crate proofs.

These are real production changes. The original description of the work as wholly verification-only and byte-identical was incorrect.

## Published campaign

The complete historical run is published as [`kani-run-753b1ae-2026-09-15`](https://github.com/Templar-Protocol/blend-contracts-v2/releases/tag/kani-run-753b1ae-2026-09-15):

- source commit: `753b1ae57cd60bf8bfd1ca595d0e1d71d4d1b1fb`
- source tree: `3a5d23a1d7bc7ac55eeb475a46c3d1ce9ea431c8`
- archive SHA-256: `6d516a7248aa220d6b86543c6f3e5aa9459f01f57ca0b623023d747eed00449c`
- inventory SHA-256: `927473cc6ad2777ff21c87b018aa3311363264e49727aa74d4945fefc0fa0541`
- member-manifest SHA-256: `df2210b461c0299766b991f2f3a895f0c4b7c6bcceb375d16f47ee007309e7ee`

All 116 recorded Rust/manifest digests across the 3,199 retained executions match that commit exactly. The accepted set contains 3,197 unique PASS targets; two superseded timeout attempts for the long inverse-equivalence proof remain visible beside its accepted `probe-long` PASS.

The release archive contains the exact runner, inventory, obligation map, closure/provenance records, campaign receipt, and all raw shard/retry/long-probe results and logs.

## Downloading and verifying the run

```sh
rm -rf /tmp/kani-release
mkdir -p /tmp/kani-release
gh release download kani-run-753b1ae-2026-09-15 \
  --repo Templar-Protocol/blend-contracts-v2 \
  --pattern 'kani-run-753b1ae-2026-09-15.tar.zst*' \
  --dir /tmp/kani-release
cd /tmp/kani-release
sha256sum --check kani-run-753b1ae-2026-09-15.tar.zst.sha256
mkdir readback
tar --zstd -xf kani-run-753b1ae-2026-09-15.tar.zst -C readback
cd readback/kani-run-753b1ae-2026-09-15
sha256sum --check SHA256SUMS
```

## Running the released inventory against this checkout

Build the Wasm files consumed by `contractimport!`, then point the released runner and inventory at the current source tree:

```sh
cd /path/to/blend-contracts-v2
devenv shell -- make build
devenv shell -- python3 \
  /tmp/kani-release/readback/kani-run-753b1ae-2026-09-15/proof-packaging/kani.py \
  --source . \
  --inventory /tmp/kani-release/readback/kani-run-753b1ae-2026-09-15/proof-packaging/inventory.json \
  --output target/kani-run \
  --target-dir target/kani \
  --solver kissat \
  --timeout 3600s
```

The runner refuses to overwrite an existing output directory, checks exact inventory selectors and expected covers, records source digests before and after each harness, and stops on the first non-PASS verdict. A new run binds its receipts to the current checkout rather than inheriting the historical result.

## Publishing a future run

`kani-release` expects a proof package containing `kani.py`, `inventory.json`, `original-obligations.json`, and `integrated-execution-receipt.json`, plus the runner output directory containing its nested `results.json` files and referenced logs:

```sh
make kani-release \
  KANI_RELEASE_COMMIT="$(git rev-parse HEAD)" \
  KANI_RELEASE_PACKAGE_DIR=/path/to/proof-packaging \
  KANI_RELEASE_RUN_DIR=/path/to/integrated-run
```

The target derives `kani-run-{commit}-{date}` from the exact commit and earliest retained execution. It independently reconciles inventory fields, execution modes, verdicts, covers, source maps, campaign totals, and every referenced log before accepting the run.

Inputs must be owned by the authenticated local operator and not group- or world-writable. The publisher snapshots them into a private directory, rejects symlinks and special files, builds a deterministic `.tar.zst`, verifies a local readback, and installs outputs without overwriting existing paths.

Publishing atomically creates the `kani-run-*` tag at the full commit SHA before it creates the draft GitHub release. The target verifies the tag while the release is still private, verifies both remote asset digests through a fresh download, and only then publishes the draft; final metadata and tag checks run after publication, with a return to draft on failure. Set `KANI_RELEASE_DRY_RUN=1` to stop after local build and verification; override `KANI_RELEASE_REPO` or `KANI_RELEASE_OUTPUT_DIR` only when needed.

This is a custody/integrity tool for a trusted operator with `gh` publication authority, not a cryptographic execution attestation. Run it with exclusive authority over `kani-run-*` tags, or protect that tag pattern against force updates; a concurrent writer can otherwise move any ordinary Git ref. Use an external signed attestation if proof authenticity must survive a malicious publishing operator.

## Property-mapped reruns on this checkout

Kani 0.67.0, `--solver kissat`, against this tree rather than the published
campaign commit. These harnesses discharge the arithmetic obligations behind the
audit property list; they are a scoped subset, not the full inventory.

Select harnesses with `--exact` and the fully qualified name
(`pool::reserve::verification::<harness>`). A bare `--harness <name>` is a
substring filter: `astra_grow_debt_nonnegative_delta` alone matches 257
harnesses — itself plus the 256 `proof_case!` fixed-supply cases
`_supply_000` through `_supply_255` — so a run reports many verdicts and the
first one does not necessarily belong to the named harness. Prefer
`-Z unstable-options --harness-timeout <n>s` over an external `timeout`, which
yields a signal rather than a verdict; on 0.67.0 the flag is unstable and
exits 2 without the `-Z`.

One harness, by fully qualified name:

```sh
cargo kani -p pool --lib \
  --exact --harness pool::reserve::verification::astra_grow_debt_nonnegative_delta \
  --solver kissat \
  -Z unstable-options --harness-timeout 1800s
```

The module segment follows the harness's production parent, so
`prove_supply_cap_strictness` is `pool::actions::verification::…`. To recover
the exact selector for any harness, `cargo kani -p pool --lib list --format json`
writes every fully qualified name to `kani-list.json` (gitignored); `--format
pretty` is the default and prints instead.

Read the verdict from the run rather than the exit status, and expect one
`VERIFICATION:-` line per matched harness:

```sh
grep -cE 'VERIFICATION:-' run.log          # 1 confirms the filter matched once
grep -oE 'VERIFICATION:- [A-Z]+' run.log
grep -F 'CBMC timed out' run.log           # a FAILED that is only a budget
```

`VERIFICATION:- FAILED` has three meanings and the verdict line and exit
status separate none of them:

| Meaning | What distinguishes it |
| --- | --- |
| the property is refuted | a counterexample trace is printed |
| the budget ran out | `CBMC timed out` appears in the log |
| CBMC itself aborted | `invariant violation` and `failed with status 134` |

The third is not hypothetical. `--incremental-smt2-solver` aborted CBMC on a
128-bit `signedbv` while verifying the supply-cap bound, and the run reported
only `VERIFICATION:- FAILED` with no timeout message — which reads exactly
like a refutation. Check for all three before calling a FAILED a finding:

```sh
grep -cE "Trace for|Counterexample" run.log   # refuted
grep -cF "CBMC timed out" run.log             # budget
grep -cE "invariant violation|failed with status" run.log  # crashed
```

### Liquidation and bad debt

Run after the `cfg(kani)` surface was repaired; these were unrunnable while
`pool/src/proofs` failed to compile.

| Harness | Verdict | Time (s) |
| --- | --- | --- |
| `prove_auction_timing_modifiers` | SUCCESSFUL | 0.35 |
| `prove_auction_quote_partition` | SUCCESSFUL | 5.16 |
| `prove_auction_discount_bounds` | SUCCESSFUL | 2.77 |
| `prove_orphan_collateral_decision` | SUCCESSFUL | 0.03 |
| `prove_setoff_guarded_parametric` | SUCCESSFUL | 1.13 |
| `prove_setoff_guarded_witnesses` | SUCCESSFUL | 0.50 |
| `prove_setoff_exact_policy_split_and_conditions` | SUCCESSFUL | 2322.10 |

The last one needs a budget above 1800 s. At 1800 s it reports
`VERIFICATION:- FAILED` with `CBMC timed out`, which is a resource limit and
not a refutation; given 7200 s it verifies in 2322 s.

### Arithmetic kernels

| Property | Harness | Verdict | Time (s) |
| --- | --- | --- | --- |
| No-free-money | `prove_b_token_forward_floor` | SUCCESSFUL | 2.18 |
| No-free-money | `prove_d_token_forward_ceil` | SUCCESSFUL | 6.14 |
| No-free-money | `prove_b_token_inverse_rounding` | SUCCESSFUL | 261.00 |
| No-free-money | `prove_d_token_inverse_rounding` | SUCCESSFUL | 247.58 |
| No-free-money | `prove_b_round_trip_down_conservative` | SUCCESSFUL | 42.07 |
| No-free-money | `prove_b_round_trip_up_conservative` | SUCCESSFUL | 59.01 |
| No-free-money | `prove_d_round_trip_down_conservative` | SUCCESSFUL | 6.23 |
| No-free-money | `prove_d_round_trip_up_conservative` | SUCCESSFUL | 44.50 |
| Cap predicate | `prove_supply_cap_strictness` | SUCCESSFUL | 0.06 |
| Utilization | `prove_utilization_and_action_predicates` | SUCCESSFUL | 0.18 |
| Utilization | `prove_utilization_range_and_branches` | SUCCESSFUL | 768.12 |
| Accrual safety | `astra_grow_debt_rate_monotonic` | SUCCESSFUL | 14.44 |
| Accrual safety | `astra_grow_debt_nonnegative_delta` | SUCCESSFUL | 76.34 |
| Accrual safety | `prove_accrue_split_and_zero_take_rate` | SUCCESSFUL | 747.55 |
| Accrual safety | `astra_grow_debt_exact_delta_composed` | SUCCESSFUL | 529.08 |
| Accrual safety | `astra_grow_debt_exact_delta_supply_000` | SUCCESSFUL | 0.95 |
| Accrual safety | `astra_grow_debt_exact_delta_supply_001` | SUCCESSFUL | 1.88 |
| Accrual safety | `astra_grow_debt_exact_delta_supply_255` | SUCCESSFUL | 211.03 |
| Accrual safety | `astra_grow_debt_exact_delta` | TIMEOUT | >3500 |
| Accrual safety | `prove_grow_debt_nonnegative_interest` | TIMEOUT | >3500 |

`prove_supply_cap_strictness` is listed as the cap *predicate* because that is
all it establishes: `supply_within_cap(a, b) == (a <= b)`, the definition of a
one-line comparison. The supply cap itself is in its own section below.

Three results need reading together. `prove_grow_debt_nonnegative_interest`
conjoins rate monotonicity, the exact accrued-interest delta, and delta
nonnegativity, and times out. Monotonicity and nonnegativity each verify
standalone. The exact delta verifies over symbolic supply against the
instrumented `AstraDebtDeltaMath` seam, and against the production `()` seam
only at supplies 0, 1 and 255; fully symbolic over `()` it times out past
3500 s. Cost grows steeply with supply magnitude (0.95 s, 1.9 s, 211 s), so the
symbolic case is intractable here rather than merely slow. Exact-delta
accounting over the whole symbolic domain on the production seam is therefore
not established; the accrual safety properties are.

`prove_utilization_range_and_branches` establishes `0 <= util <= SCALAR_7` plus
the zero-liability, cap and interior branch equalities. The same property is
configured as `utilization_in_range` in the Certora Sunbeam suite, where it
times out, so Kani reaches a range claim the prover does not — but over a
different domain, and neither contains the other. This harness takes `u8`
supplies (0 to 255) and `hundredths` rates (0 to 2.55x `SCALAR_12`, `b_rate`
included, which is what reaches the saturation branch). Certora's
`bounded_reserve` spans supplies to 1e18 and rates `SCALAR_12` to 1e15,
excluding `b_rate` below `SCALAR_12`. Neither establishes the range for
production-scale supplies at post-default rates.

### Supply cap

The audit property list calls this "borrow caps". The protocol has no borrow
cap; it has `supply_cap` and `max_util`. This is the supply cap.

**Status: partial, and deliberately so.** The action-level property is
established over `u8` supplies at three fixed rates. The same bound at
production `i128` width is undecided — thirteen measurements across six
harness shapes, eight CBMC backends and one Certora job, every one budget
exhausted or tool failure, tabulated below. The general argument is sound;
nothing here machine-checks it at full width. Treat the supply cap as
partially verified and read the boundary below before relying on it.

**`total_supply <= supply_cap` is not an invariant of this system**, and no
proof can make it one. One production path raises `total_supply` with no cap
check anywhere near it: `ReserveData::accrue` recomputes `b_rate` upward as
interest accrues, and `total_supply` is
`floor(b_supply * b_rate / SCALAR_12)`, so the total grows with time alone and
passes the cap if the cap is close. `test_accrual_carries_total_supply_past_cap`
exercises it.

The cap itself cannot move under a live pool, which is what confines the
exclusion to accrual. `execute_set_reserve` admits a metadata change on a
non-setup pool only when `is_disable_only` holds, and that requires
`current.supply_cap == candidate.supply_cap`; `execute_set_pool_status` accepts
only 0, 2, 3 and 4, so status 6 is unreachable once the pool leaves setup.
`supply_cap` is therefore fixed from the moment a pool goes live, which
`test_queue_set_reserve_rejects_other_field_drift_on_existing` already
exercises: it drifts `supply_cap` by one on an existing reserve and the queue
call rejects it.

What the code enforces is the action-level property, and that is what is
established here: **no supply action can raise a reserve's accounted supply
above its cap.** It holds in three parts.

**The two capped handlers.** `apply_supply` and `apply_supply_collateral` each
end by testing `supply_within_cap(reserve.total_supply(e),
reserve.config.supply_cap)` and panicking on failure, with only
`pool.cache_reserve(reserve)` between the test and the return. Neither the test
nor the cache write touches `b_supply`.

**The uncapped increase paths cancel.** Exactly two sites raise `b_supply` in
production — `User::add_collateral` and `User::add_supply`, both
`reserve.data.b_supply += amount`. Their production callers are:

| Caller | Why the cap is not evaded |
| --- | --- |
| `apply_supply` | the guard above |
| `apply_supply_collateral` | the guard above |
| `bad_debt.rs`, orphan collateral | `remove_collateral(amount)` on the same reserve immediately precedes it, and that calls `burn_supply(amount)` |
| `add_positions` from `fill_bad_debt_auction` | passes `map![e]` as the collateral map, so `add_collateral` is never reached |
| `add_positions` from `fill_user_liq_auction` | `rm_positions` with the identical `lot` map precedes it, removing the same collateral per asset |

The last two are position transfers between users, not new supply. They are
safe only because an equal decrease precedes the increase on the same reserve,
which `prove_burn_then_add_is_supply_neutral` pins.

**The arithmetic cannot evade the check.** The guard is tested on
`to_asset_from_b_token(b_supply)` after `to_b_token_down(amount)` has been
minted and added. Both conversions floor, and flooring is not additive, so a
deposit credited more underlying than it delivered would let the check pass
while the true total sat above the cap.

| Harness | Verdict | Time (s) |
| --- | --- | --- |
| `prove_burn_then_add_is_supply_neutral` | SUCCESSFUL | 0.09 |
| `prove_supply_credit_monotonic` | SUCCESSFUL | 27.95 |
| `prove_supply_credit_no_inflation_h001` | SUCCESSFUL | 1.49 |
| `prove_supply_credit_no_inflation_h255` | SUCCESSFUL | 4.87 |
| `prove_supply_credit_no_inflation_h100` | SUCCESSFUL | 6.21 |
| `prove_supply_cap_decides_on_post_state` | SUCCESSFUL | 25.49 |

`prove_supply_credit_monotonic` and `prove_supply_cap_decides_on_post_state`
hold over a symbolic rate. The upper bound does not: it puts a quotient inside
the third conversion's dividend, and with a symbolic divisor there the solver
does not finish, so it is split across three fixed rates. `h001` is far below
the initial `SCALAR_12`, the region a reserve enters after `default_b_rate`
subtracts a loss; `h100` is the initial rate exactly; `h255` tops the
`hundredths` range. Supplies and amounts are `u8`, as elsewhere in this
module, and that is the ceiling rather than a convention. Four shapes of the
bound were attempted and every one exhausted its budget:

| Attempt | Domain | Budget |
| --- | --- | --- |
| bound and monotonicity together, symbolic rate | `u8` | 900 s |
| same, direct division | `i128` to 1e18 | 3600 s |
| restated division-free, quotients named and constrained | `i128` to 1e18 | 1800 s |
| same, division-free | `u32` | 1200 s |
| restated in product space, no multiplication at all | `i128`, products to `i128::MAX/4` | 1200 s |

The division-free restatement replaces all three divisions with
multiplication and the product-space restatement removes multiplication too,
leaving division by the constant `SCALAR_12`, one modulo and comparisons. Both
still exhausted their budgets, so the cost is neither division nor
multiplication specifically: a 128-bit arithmetic circuit over symbolic
operands is too large for a SAT backend whatever its shape. Writing the
divisions as exact remainder equalities gives the same products and so buys
nothing either. A conformance check tying the real conversions to the
floor inequalities runs in 2.18 s at `u8`
(`prove_b_token_forward_floor`) and exhausts 1200 s at `u32`, so the link
between the arithmetic and the implementation is itself only machine-checked
at `u8` width.

The bound is therefore established exhaustively over `u8` supplies at the
three rates above, and the general argument is sound but unchecked at
production scale, by either tool.

`confs/supply_credit.conf` states the bound over `bounded_reserve` with no
storage, no handler and no loops, on the reasoning that Certora works over
integer theories and that the three HALTed supply-cap jobs failed on the
handler call graph rather than on this arithmetic. Job
`93cecf326b8243d7bdd4751a9f6187e5`, evidence in
[`../verification/certora/supply_credit-93cecf32.json`](../verification/certora/supply_credit-93cecf32.json):

```
SUCCESS  sanity_supply_credit_mints
TIMEOUT  supply_credits_no_more_than_supplied
```

The sanity rule passing is what makes the TIMEOUT meaningful: the setup is
reachable, so the prover ran out of budget on the property rather than on a
broken fixture. At `global_timeout: 1800` the bound is undecided there too.

The obligation was then held fixed and the backend varied instead, on the
product-space harness:

| Backend | Result |
| --- | --- |
| `kissat` (SAT) | budget exhausted |
| `cadical` (SAT, CBMC default) | budget exhausted |
| `cvc5` 1.4.2 (SMT) | budget exhausted |
| `z3` 5.3.1 (SMT) | budget exhausted |
| `cvc5 --bv-abstraction` via `--incremental-smt2-solver` | CBMC aborted, no verdict |
| `--refine-arithmetic` | budget exhausted |
| `--refine-arithmetic --max-node-refinement 50` | budget exhausted |
| `--refine` | budget exhausted |

The reason the operator matters is documented: `bvmul`, `bvudiv` and `bvurem`
admit only large SAT encodings, and no statement of this bound avoids all
three, which is why the harness shapes above fail alike. The published
mitigation is abstraction refinement over exactly those operators, and both
implementations of it were tried — cvc5 1.4.0 added `--bv-abstraction`, CBMC
has `--refine-arithmetic`. Neither discharged it, and the cvc5 route aborted
CBMC rather than returning a verdict. Certora, which reasons over integers
rather than bit-vectors, also timed out while its sanity rule passed. Thirteen
measurements say the bound is not machine-checkable at full width with this
tooling; the one option left untried is this conf at a higher
`global_timeout`.

`supply_chain` transcribes the sequence `apply_supply` runs rather than
driving the handler, which needs an `Env`. `add_supply` adds exactly its
argument to `b_supply` and `apply_supply` passes exactly
`to_b_token_down(amount)`, both checked by reading, so the transcription is
faithful today and nothing keeps it faithful if the handler is reordered.

**The handler decides where the harnesses say it does.** The harnesses above
transcribe the arithmetic rather than driving `apply_supply`, so two tests
close that gap by execution:
`test_apply_supply_admits_exactly_up_to_cap` supplies exactly to the cap and
asserts the post-state equals it, and
`test_apply_supply_rejects_one_over_cap` supplies one stroop more and expects
`ExceededSupplyCap`.

The writer enumeration is a reading of the code, so
`test_b_supply_writers_are_enumerated` pins it. The test walks every `.rs`
file under `pool/src`, skipping the verification-only `proofs/` and `spec/`
trees, excises each `#[cfg(test)]` module by brace matching, and reports per
file the number of `b_supply` writes in three forms: compound addition,
compound subtraction, and plain assignment, the last covering
`b_supply = b_supply + amount`. The expected report is asserted exactly, so a
new writer anywhere in the tree fails the build rather than silently joining
the argument, including one in a module that did not exist when this was
written.

Three limits are worth naming. The test matches an operator immediately
following the field, so a write through an alias would not be seen
(`let s = &mut reserve.data.b_supply; *s += amount`); no production write
takes that form today. Brace matching does not skip string literals, so an
unbalanced brace inside one ends a module's excision early — that retains
code rather than hiding it, and over-counts into a visible failure. And the
test cannot classify a site it finds: placing a new one in the cap-guard or
paired-decrease case above still needs a reader.

## Claim boundary

The released 3,197/3,197 result applies exactly to commit `753b1ae`. It is not a full-inventory result for the rebased or file-separated PR tree; current validation remains the native suite plus the property-mapped reruns recorded above.

- Do not weaken a proof obligation, expected cover count, or production behavior to obtain a green verdict.
- Do not substitute a different contract, tolerance, or historical waiver.
- Treat any source digest change as a new proof candidate; historical receipts remain historical.
- Keep failures and unreachable properties visible rather than relabeling them.
