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
substring filter: `astra_grow_debt_nonnegative_delta` alone matches 258
harnesses because of the `proof_case!` fixed-supply families, so a run reports
many verdicts and the first one does not necessarily belong to the named
harness. Prefer `-Z unstable-options --harness-timeout <n>s` over an external
`timeout`, which yields a signal rather than a verdict; on 0.67.0 the flag is
unstable and exits 2 without the `-Z`.

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
```

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
| Supply cap | `prove_supply_cap_strictness` | SUCCESSFUL | 0.06 |
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
times out; Kani discharges it at the kernel level.

## Claim boundary

The released 3,197/3,197 result applies exactly to commit `753b1ae`. It is not a full-inventory result for the rebased or file-separated PR tree; current validation remains the native suite plus the property-mapped reruns recorded above.

- Do not weaken a proof obligation, expected cover count, or production behavior to obtain a green verdict.
- Do not substitute a different contract, tolerance, or historical waiver.
- Treat any source digest change as a new proof candidate; historical receipts remain historical.
- Keep failures and unreachable properties visible rather than relabeling them.
