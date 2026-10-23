# Kani lending-kernel verification

This PR keeps the code that is reviewed and merged: production arithmetic seams and their `#[cfg(kani)]` proofs. Campaign inventories, runners, receipts, and generated closure data are published as release evidence instead of living in the merge diff.

## Toolchain

**Kani 0.68.0** is required on this tree. It bundles rustc 1.100.0-nightly,
which clears the workspace's `rust-version = "1.98"` floor;
`cargo kani -p pool --only-codegen` compiles every harness with 0 errors
(about 7 minutes cold). Kani 0.67.0 bundles rustc 1.93.0-nightly and refuses
to plan the graph:

```text
$ cargo kani -p pool --only-codegen
error: rustc 1.93.0-nightly is not supported by the following packages:
  moderc3156@0.1.0 requires rustc 1.98
  pool@2.0.0 requires rustc 1.98
```

The published campaign ran under Kani 0.67.0 on the pre-upgrade tree
(soroban-sdk 22, Rust 1.81; `integrated-execution-receipt.json`:
"kissat (Kani 0.67.0)"). 0.68.0 was unusable there: its rustc rejected
`ethnum` 1.5.0 (`E0512`), and `ethnum` 1.5.3 needed a const `from_str_radix`
that Rust 1.81 lacked. The soroban-sdk 25.3.0 lock carries `ethnum` 1.5.3,
which removes that bind. The proofs have since been re-run under 0.68.0 on
this tree; the results are recorded in "Kani 0.68.0 rerun (2026-10-22)" below
and supersede the 0.67.0 tables in this document as the current evidence.
Three CI jobs cover the proof surface, all required by the `complete` gate in
`.github/workflows/pull_request.yml`: `kani-codegen` compiles every harness
(`--only-codegen`), which is what catches a harness that has stopped building;
`kani-proofs` verifies the set in `verification/kani-ci-harnesses.txt`, every
harness the 0.68.0 tables record under 70 s; and `fuzz-build` builds and
briefly runs the libFuzzer target, which lives in its own workspace and so is
compiled by nothing else. The expensive harnesses run weekly from
`.github/workflows/verification-full.yml`, listed in
`verification/kani-full-harnesses.txt`. Both jobs read verdicts from the log
rather than the exit status, for the reason given under "Property-mapped
reruns" below.

```sh
cd /tmp && cargo install --locked kani-verifier --version 0.68.0 --force
cargo-kani setup   # only if ~/.kani/kani-0.68.0 is absent

cargo kani --version   # expect cargo-kani 0.68.0
```

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

These ran on the pre-upgrade tree under Kani 0.67.0. Every row below has been
re-run with Kani 0.68.0 on the current tree; that campaign is recorded in
"Kani 0.68.0 rerun (2026-10-22)" and its tables supersede the ones here. The
commands are otherwise unchanged.

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
`prove_supply_cap_strictness` is `pool::actions::verification::…`. The frame
harnesses are the one exception: `proofs/reserve_frames.rs` is attached from
`proofs/reserve.rs` so it can reuse `sym_data` and `hundredths`, which puts
them at `pool::reserve::verification::frames::…`. A selector built from the
rule above matches nothing for them, silently. To recover
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

### Accrual value conservation

`Reserve::load` is the only place a reserve's accounted value moves without a
token transfer: it calls `grow_debt` and feeds the return straight into
`accrue`. Every accrual harness above drives one half of that pair with the
other half's book pinned to zero -- `grow_debt` at `b_supply == 0`, `accrue` at
`d_supply == 0` and `d_rate == 0` -- so the two halves had never met on one
reserve. These run both live.

| Property | Harness | Verdict | Covers | Time (s) |
| --- | --- | --- | --- | --- |
| No-free-money | `prove_accrual_conserves_value_debt_000` | SUCCESSFUL | 0 of 2 | 33.82 |
| No-free-money | `prove_accrual_conserves_value_debt_001` | SUCCESSFUL | 2 of 2 | 434.52 |
| No-free-money | `prove_accrual_conserves_value_debt_002` | SUCCESSFUL | 2 of 2 | 688.85 |
| No-free-money | `prove_accrual_conserves_value_debt_255` | SUCCESSFUL | 2 of 2 | 7334.77 |

The claim is `(held_after - held_before) + (credit_after - credit_before) <=
accrued` together with `accrued == owed_after - owed_before`: the supplier and
backstop claims rise by no more than the borrower obligation that funds them.
`d_supply` is pinned. Every other input is symbolic over a `u8`-derived grid
rather than the full `i128` range: `b_supply` spans 1 to 255 and the prior
backstop credit 0 to 255; `b_rate`, `d_rate` and `loan_accrual` are hundredths
of unity, so `b_rate` covers 0 to 2.55x and the two growth factors 1.00x to
2.55x in 0.01 steps; the take rate is whole percentage points, 0% to 99%.
Intermediate rates, rates above 2.55x unity, and take rates off a percentage
point are therefore not covered.

**Historical result:** `debt_000` above is a vacuous pass, with neither
positive-growth cover satisfied. The three positive-debt rows satisfy both.
Those campaign rows remain unchanged.

**Local repair (2026-10-23):** the zero-debt harness now proves zero accrued
value and unchanged supplier claims/credit, rather than demanding growth.
Its four covers witness empty supply, positive supply at zero b_rate, positive
claims, and an increased d_rate despite zero accrued debt value. Kani 0.68.0
returned SUCCESSFUL with 4/4 covers (30.20 s solver time). `make verify-local`
enforces the exact selector and cover count. See
[`verification-local.md`](verification-local.md) for scope and reproduction.
This remains grid-domain checked-i128 evidence, not an I256/host proof.

`d_supply` is pinned because it is the multiplicand in the debt conversion and
the established cost driver -- `astra_grow_debt_exact_delta` leaves it symbolic
and times out past 3500 s. Pinning `b_supply` instead does not help: at
`b_supply == 1`, where the `accrue` divisor is 1, the harness still reached
1800 s with no verdict. `debt_255` needs a budget above 7200 s; at 1800 s it
reports no verdict, which is a resource limit and not a refutation.

Scope: three debt levels, 1, 2 and 255, at `u8` width. The property is not
established for symbolic `d_supply`, and **solvency proper is not addressed** --
assets-cover-liabilities needs the pool's token balance, which lives outside
`ReserveData` and is not reachable from Kani.


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
exercises it, and `prove_accrue_moves_only_rate_and_credit` establishes the
other half: accrual moves `b_rate` and `backstop_credit` and no other field,
so it raises the accounted total without issuing shares and is excluded from
the cap guard rather than counted as a writer of `b_supply`.

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

**The uncapped increase paths cancel.** One production function raises
`b_supply`: `ReserveData::mint_supply`, the mirror of `burn_supply`. It is
`pub(super)`, so rustc confines its callers to `pool/src/pool`, and
`User::add_collateral` and `User::add_supply` are the only two that call it.
That bounds the callers of the method, not the writers of the field: under
`#[contracttype]` `b_supply` stays `pub`, so a bare field write elsewhere in
`pool/src/pool` would still compile, and it is
`test_b_supply_writers_are_enumerated` rather than the compiler that pins its
absence. Their own production callers are:

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

**Each mutator moves only the field it names.** `impl ReserveData` has exactly
four `&mut self` methods, so these four harnesses are the whole mutator
surface rather than a sample. Each runs over a reserve whose every field is
symbolic, and the field list is recovered by exhaustive destructuring, so a
new `ReserveData` field fails to compile here instead of dropping silently
out of all four frames.

The Certora suite already covers part of this at the handler level and more
strongly: `add_supply_increases_b_supply`,
`add_collateral_increases_b_supply`, `remove_supply_decreases_b_supply` and
`remove_collateral_decreases_b_supply` drive the real handlers over a nondet
`Reserve`, a nondet `User` and a symbolic `Env`, and each asserts the
`b_supply` delta and that `d_supply` is untouched. What is new here is the
other five fields, and the frames on `accrue` and `grow_debt`, which neither
suite covered.

`Reserve::load` writes `ir_mod` and `last_time` itself, outside these four
methods and behind an `Env`, so it is not framed here.

The accrue frame carries two `cover!`s. `bstop_rate` is a percentage in
`SCALAR_7` points, so a `u8` domain for it floors every credit to zero:
`floor(255 * 255 / SCALAR_7) == 0`, and the `backstop_credit` half of the frame
would hold without the branch that moves it ever running. Both report
SATISFIED today. An unsatisfied cover does **not** fail the harness, though:
`kani` exits 0 with one present, verified by planting
`kani::cover!(amount > 1000)` against a `u8` amount. The covers are a
diagnostic in the harness output; what enforces them is
`scripts/kani_release.py`, which compares `N of M cover properties
satisfied` against each inventory entry's `expected_covers`. These two are
not in that inventory, so nothing currently fails if the domain regresses.

| Harness | Verdict | Time (s) |
| --- | --- | --- |
| `prove_mint_supply_frame` | SUCCESSFUL | 0.14 |
| `prove_burn_supply_frame` | SUCCESSFUL | 0.14 |
| `prove_accrue_moves_only_rate_and_credit` | SUCCESSFUL | 1.55 |
| `prove_grow_debt_moves_only_d_rate` | SUCCESSFUL | 0.50 |

`mint_supply` and `burn_supply` each move `b_supply` by exactly their
argument. `burn_supply`'s harness leaves `amount` unbounded by `b_supply`
because the method makes no such check: `remove_collateral` and
`remove_supply` validate the user's position, not the reserve total, so the
missing premise is the global invariant that positions sum to `b_supply`,
which nothing here proves. `accrue` divides by `b_supply` to recompute
`b_rate` without checking it is non-zero, so its harness assumes
`b_supply > 0`.

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
hold over a symbolic rate. The upper bound is split across three fixed rates
because it puts a quotient inside the third conversion's dividend, which is
expensive with a symbolic divisor. That split is a budget artefact and not a
limit: `prove_supply_claims_bounded_by_tokens_in` states both bounds together
at a symbolic rate over the same `u8` domain and verifies, at a budget well
above the 900 s tried here -- see "Solvency, as an induction over the reserve
kernel" below. The three fixed-rate cases are subsumed at `u8` width and kept
as the cheaper check. `h001` is far below the initial `SCALAR_12`, the region a
reserve enters after `default_b_rate` subtracts a loss; `h100` is the initial
rate exactly; `h255` tops the `hundredths` range. Supplies and amounts are
`u8`, as elsewhere in this module, and that is the ceiling rather than a
convention. Five shapes of the bound were attempted; each exhausted the budget
it was given, and only the first has since been discharged at a larger one:

| Attempt | Domain | Budget |
| --- | --- | --- |
| bound and monotonicity together, symbolic rate | `u8` | 900 s (since discharged above that budget) |
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

The bound is therefore established over `u8` supplies at a symbolic rate, and
the general argument is sound but unchecked at production scale, by either
tool. Every measurement in the table above is at `i128` or `u32` width except
the first, and nothing here bears on those: full width remains undecided.

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
The rule asserts three things at once — that the mint is non-negative, the
upper bound, and monotonicity — and Certora checks a rule's assertions as
one conjunction unless `multi_assert_check` is set. So the TIMEOUT is a
verdict on all three together, and 1800 s of prover time returned nothing
about the two cheap ones; splitting them into their own rule would let
those land while the bound keeps timing out. The non-negativity claim is
in any case implied by the assumes, since `amount >= 0` and `b_rate` is at
least `SCALAR_12`.

The conf is also weaker than its sibling in one respect: `supply_cap.conf` passes
`-splitParallel true` and `-dontStopAtFirstSplitTimeout true`, and
`supply_credit.conf` passes neither, so the TIMEOUT is a verdict on this
configuration rather than on everything the prover could have been asked to
try.

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

Every measurement above predates the overflow fix to this harness, which
removed an `i128` addition that could overflow inside a `kani::assume` and so
carried a property unrelated to the bound. The harness was re-measured after
the fix and killed at 1300 s with no verdict line, so the bound is
unverified on the fixed harness too. A killed run carries no solver
evidence, so it says nothing about whether the spurious property was
contributing; that question is open. A later pass removed a symbolic
`i128` from the same harness that turned out to constrain nothing, and a
third run was killed at 1300 s as well, so the variable count was not the
binding constraint either.

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
trees, excises each `#[cfg(test)] mod` block, and reports per
file the number of `b_supply` writes in three forms: compound addition,
compound subtraction, and plain assignment, the last covering
`b_supply = b_supply + amount`. The expected report is asserted exactly, so a
new direct write fails the build rather than silently joining the argument,
including one in a module that did not exist when this was written.
Enumeration is syntactic and covers direct writes only; the limits below
say what that excludes.

Excision covers `#[cfg(test)] mod` and nothing else; any other shape panics
with the offending text. An item's extent depends on its shape, and guessing
wrong excises production code and hides the writes in it: a field, variant or
match arm ends at a comma or the enclosing brace, a `use` at a semicolon.
Four earlier forms each guessed and each hid a write while passing. Naive
brace matching was the first. Counting braces inside string literals was the
second, where a test module containing `"{"` ran the excision to end of file.
Scanning for the next `{` was the third, where a brace-less `use` sent it into
the following production body. The fourth survived the whitelist: `mod x;`
declares a module out of line, so the scan for `{` reached the next
production body, and the proof modules in this tree are declared exactly that
way. `mod x { .. }` and `mod x;` are the only two forms, so both are handled
rather than one guessed. All eighteen `#[cfg(test)]` items under
`pool/src` are `mod tests`, so the whitelist costs nothing and converts every
unanticipated shape into a failure rather than a silent omission.

Brace matching inside an accepted module still skips strings, raw strings,
char literals and comments. Two tests pin both halves against fixtures rather
than by hand: `test_production_rejects_unsupported_cfg_test_shapes` over the
field, variant, match-arm, `use`, `extern crate` and `impl` shapes, and
`test_production_keeps_writes_after_test_modules` over the brace-in-literal
cases. Disabling either the whitelist or the literal skipping makes one of
them fail, which is how they were checked.

Two limits remain. The test matches an operator immediately following the
field, so a write through an alias would not be seen
(`let s = &mut reserve.data.b_supply; *s += amount`); no production write
takes that form today. Struct-literal initialization is not counted either:
`initialize_reserve` in `config.rs` constructs `ReserveData { b_supply: 0,
.. }`, which is a production write of the field and is the base case the
argument above assumes rather than a case it has to exclude. A future
literal setting a non-zero `b_supply` would escape the guard. Counting
`b_supply:` would catch that, at the price of also counting the field
declaration in `storage.rs`, the `certora` nondet impl beside it and two
`testutils.rs` fixtures — four counts that are not writes, against the one
that is, and any edit to the struct or the fixtures would then fail the
test spuriously. The exclusion is recorded instead.

`production()` skips literals and comments but `writes()`
does not, so `b_supply +=` inside a production comment or string would count;
that over-counts into a visible failure rather than hiding anything. The walk
treats everything outside `proofs/` and `spec/` as production, which includes
the test-only `testutils.rs` and the `#[cfg(all(kani, test))]` module gates —
again over-counting, not under. And the test cannot classify a site it finds:
placing
a new one in the cap-guard or paired-decrease case above still needs a
reader.

## Kani 0.68.0 rerun (2026-10-22)

Every harness in the four tables above was re-run on this checkout with Kani
0.68.0 (bundled rustc 1.100.0-nightly, CBMC 6.11.0) on commit `886e26f`,
using the documented command form with `--solver kissat` and
`-Z unstable-options --harness-timeout <n>s`; two harnesses ran concurrently
against separate `--target-dir` slots. Wall times include per-invocation
driver overhead and were measured on a shared host, so they are not comparable
with the 0.67.0 numbers (some are faster, some slower); verdicts, not timings, are the claim. No counterexample trace
and no CBMC crash appeared anywhere: the only non-PASS verdicts are the two
budget cases below.

| Harness | Verdict | Time (s) |
| --- | --- | --- |
| `prove_auction_timing_modifiers` | SUCCESSFUL | 7.42 |
| `prove_auction_quote_partition` | SUCCESSFUL | 12.50 |
| `prove_auction_discount_bounds` | SUCCESSFUL | 13.49 |
| `prove_orphan_collateral_decision` | SUCCESSFUL | 3.43 |
| `prove_setoff_guarded_parametric` | SUCCESSFUL | 8.41 |
| `prove_setoff_guarded_witnesses` | SUCCESSFUL | 7.28 |
| `prove_setoff_exact_policy_split_and_conditions` | SUCCESSFUL | 2271.20 |

| Property | Harness | Verdict | Time (s) |
| --- | --- | --- | --- |
| No-free-money | `prove_b_token_forward_floor` | SUCCESSFUL | 9.94 |
| No-free-money | `prove_d_token_forward_ceil` | SUCCESSFUL | 12.82 |
| No-free-money | `prove_b_token_inverse_rounding` | SUCCESSFUL | 384.63 |
| No-free-money | `prove_d_token_inverse_rounding` | SUCCESSFUL | 382.33 |
| No-free-money | `prove_b_round_trip_down_conservative` | SUCCESSFUL | 12.95 |
| No-free-money | `prove_b_round_trip_up_conservative` | SUCCESSFUL | 65.24 |
| No-free-money | `prove_d_round_trip_down_conservative` | SUCCESSFUL | 13.69 |
| No-free-money | `prove_d_round_trip_up_conservative` | SUCCESSFUL | 70.77 |
| Cap predicate | `prove_supply_cap_strictness` | SUCCESSFUL | 5.12 |
| Utilization | `prove_utilization_and_action_predicates` | SUCCESSFUL | 4.47 |
| Utilization | `prove_utilization_range_and_branches` | SUCCESSFUL | 932.95 |
| Accrual safety | `astra_grow_debt_rate_monotonic` | SUCCESSFUL | 16.01 |
| Accrual safety | `astra_grow_debt_nonnegative_delta` | SUCCESSFUL | 105.64 |
| Accrual safety | `prove_accrue_split_and_zero_take_rate` | SUCCESSFUL | 1109.20 |
| Accrual safety | `astra_grow_debt_exact_delta_composed` | SUCCESSFUL | 399.71 |
| Accrual safety | `astra_grow_debt_exact_delta_supply_000` | SUCCESSFUL | 7.62 |
| Accrual safety | `astra_grow_debt_exact_delta_supply_001` | SUCCESSFUL | 12.31 |
| Accrual safety | `astra_grow_debt_exact_delta_supply_255` | SUCCESSFUL | 378.72 |
| Accrual safety | `astra_grow_debt_exact_delta` | TIMEOUT | 3605.83 |
| Accrual safety | `prove_grow_debt_nonnegative_interest` | TIMEOUT | 3605.25 |

| Property | Harness | Verdict | Covers | Time (s) |
| --- | --- | --- | --- | --- |
| No-free-money | `prove_accrual_conserves_value_debt_000` | SUCCESSFUL | 0 of 2 | 12.47 |
| No-free-money | `prove_accrual_conserves_value_debt_001` | SUCCESSFUL | 2 of 2 | 212.36 |
| No-free-money | `prove_accrual_conserves_value_debt_002` | SUCCESSFUL | 2 of 2 | 260.05 |
| No-free-money | `prove_accrual_conserves_value_debt_255` | SUCCESSFUL | 2 of 2 | 3094.58 |

| Harness | Verdict | Time (s) |
| --- | --- | --- |
| `prove_burn_then_add_is_supply_neutral` | SUCCESSFUL | 5.85 |
| `prove_supply_credit_monotonic` | SUCCESSFUL | 41.78 |
| `prove_supply_credit_no_inflation_h001` | SUCCESSFUL | 6.34 |
| `prove_supply_credit_no_inflation_h255` | SUCCESSFUL | 11.42 |
| `prove_supply_credit_no_inflation_h100` | SUCCESSFUL | 11.13 |
| `prove_supply_cap_decides_on_post_state` | SUCCESSFUL | 33.07 |

### Reading the two timeouts

Both are resource limits on the same harness shapes the 0.67.0 campaign also
could not discharge, not refutations, and they change no conclusion recorded
above:

- `astra_grow_debt_exact_delta` exited 1 at the 3600 s harness budget with
  `CBMC timed out` and no trace. Exactly as before, the fully-symbolic exact
  delta over the production seam is intractable here; the instrumented seam,
  the composed harness, and the fixed supplies 000/001/255 all verify.
- `prove_grow_debt_nonnegative_interest` likewise exited at its 3600 s budget
  with `CBMC timed out` and no trace, matching its 0.67.0 TIMEOUT. Its
  conjuncts each verify standalone (`astra_grow_debt_rate_monotonic`,
  `astra_grow_debt_nonnegative_delta`).

`prove_setoff_exact_policy_split_and_conditions` again needs a budget above
1800 s (2271 s at 7200 s), and `prove_accrual_conserves_value_debt_255`
again completes only with a budget above 1800 s — it verified in 3095 s,
faster than the 7335 s the 0.67.0 run recorded. Cover counts are unchanged
from the 0.67.0 campaign, including the vacuous `debt_000` pass (0 of 2).

The 0.68.0 run reproduces the 0.67.0 campaign's verdicts, cover counts and
failure modes on the soroban-sdk 25.3.0 / Rust 1.98 tree: 35 SUCCESSFUL, the same
2 budget-limited TIMEOUTs, no counterexamples, no tool crashes, no harness
required a change. Timings differ in both directions and are not part of the claim.

## Solvency, as an induction over the reserve kernel

Solvency is `balance + debt >= claims + credit`, where
`claims = to_asset_from_b_token(b_supply)`,
`debt = to_asset_from_d_token(d_supply)`, `credit` is `backstop_credit` and
`balance` is the pool's token holding. It is not statable against the Certora
harness: `spec/token.rs` is a `cvlr_mock_client` mock, so the balance it
reports is unconstrained.

What is provable is part of the induction. A solvent pool stays solvent if
every transition preserves the inequality, and three are discharged here --
withdraw only on its uncapped path, as the table says. Supply and withdraw
leave `debt` and `credit` untouched, so those terms cancel and preservation
reduces to a single inequality between the claims delta and the transfer
amount, rather than the two-sided form that forced `d_supply` to be pinned in
"Accrual value conservation" above.

| Transition | Obligation | Harness | Verdict | Covers | Time (s) |
| --- | --- | --- | --- | --- | --- |
| supply | `claims_after - claims_before <= tokens_in` | `prove_supply_claims_bounded_by_tokens_in` | SUCCESSFUL | 1 of 1 | 2441.74 |
| withdraw, uncapped path | `tokens_out <= claims_before - claims_after` | `prove_withdraw_tokens_out_bounded_by_claims_released` | SUCCESSFUL | 1 of 1 | 1366.42 |

Both ran on this checkout under Kani 0.68.0 with `--solver kissat` and a
7200 s harness budget. As in the rerun campaign above, timings are not
comparable across hosts or campaigns and the verdicts are the claim. The
accrual transition is the four `prove_accrual_conserves_value_debt_*` rows in
"Accrual value conservation" above.

Neither harness holds vacuously. The rounding is what carries them:
`to_b_token_down` floors the shares a supplier receives and `to_b_token_up`
ceils the shares a withdrawal burns, so both round against the user.

The two halves are not equally novel. The withdraw bound also follows from
`prove_b_round_trip_up_conservative` together with superadditivity of `floor`,
so its harness discharges a hand step rather than proving something new. The
supply bound does not follow from the single-deposit result, because flooring
is not additive in the direction it needs.

**The induction does not close.** `ReserveData` has four further
valuation-relevant transitions in production, none of them covered here:

| Write | Transition |
| --- | --- |
| `user.rs:100`, `d_supply += amount` | borrow |
| `user.rs:120`, `d_supply -= amount` | repay |
| `user.rs:133`, `b_rate = default_b_rate(..)` | a default absorbed into `b_rate` |
| `backstop_interest_auction.rs:111`, `backstop_credit -= lot_amount` | backstop credit paid out |

Borrow and repay are the same shape as the two rows above with `d_supply` in
place of `b_supply`, and are the next step. Until all four are discharged this
is a result about the supplier side of the book plus accrual, not about
solvency.

**Assumptions.**

- `balance` is never a modelled quantity. Each obligation is stated as a
  claims delta against the transfer amount, and solvency preservation follows
  by adding the unchanged terms to both sides -- given that the handler moves
  `balance` by exactly that amount, which nothing here checks.
- Supplies, rates and amounts are `u8`-derived: supplies and amounts 0 to 255,
  `b_rate` in hundredths of unity and so capped at 2.55x. Production `b_rate`
  exceeds that after sustained accrual.
- `b_rate > 0` is assumed because both conversions divide by it. Production can
  reach `b_rate == 0`: `default_b_rate` subtracts a ceiled loss and clamps at
  zero, so a reserve that has absorbed a default is outside the domain.
- The withdraw harness assumes `shares <= b_supply` and takes `tokens_out` as
  the free variable, which is the uncapped path. `plan_withdraw`'s capped
  branch instead derives `tokens_out` from the capped share count, so it is not
  an instance of this parameterisation and the withdraw transition is not
  discharged for it.
- The harnesses drive the two mutators, `ReserveData::mint_supply` and
  `burn_supply`, not the handlers: `User::add_supply` and
  `User::add_collateral` reach them through an `Env`, a Soroban `Map` and
  `update_b_emissions`, none of which Kani can drive. Linking them is an
  enumeration argument held by `test_b_supply_writers_are_enumerated` rather
  than by the compiler, since `b_supply` stays `pub` under `#[contracttype]`;
  making the field private would convert that step into a compiler guarantee.


## Claim boundary

The released 3,197/3,197 result applies exactly to commit `753b1ae`. It is not a full-inventory result for the rebased or file-separated PR tree; current validation remains the native suite plus the Kani 0.68.0 rerun recorded above, which supersedes the 0.67.0 property-mapped tables.

- Do not weaken a proof obligation, expected cover count, or production behavior to obtain a green verdict.
- Do not substitute a different contract, tolerance, or historical waiver.
- Treat any source digest change as a new proof candidate; historical receipts remain historical.
- Keep failures and unreachable properties visible rather than relabeling them.
