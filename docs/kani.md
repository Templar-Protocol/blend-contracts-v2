# Kani lending-kernel verification

This PR keeps the code that is reviewed and merged: production arithmetic seams and their `#[cfg(kani)]` proofs. Campaign inventories, runners, receipts, and generated closure data are published as release evidence instead of living in the merge diff.

## Tracked proof surface

- Runtime code remains in the ordinary `pool/src` and `backstop/src` modules.
- Proof bodies live in sibling `*_verification.rs` files behind `#[cfg(kani)]` (or the existing `#[cfg(all(kani, test))]` status gate).
- `pool/src/pool/inverse_partitions.rs` is proof-only exhaustive partition code.

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

## Claim boundary

The released 3,197/3,197 result applies exactly to commit `753b1ae`. It is not a full-inventory result for the rebased or file-separated PR tree; current validation remains the native suite plus the scoped moved-module Kani reruns recorded on PR #4.

- Do not weaken a proof obligation, expected cover count, or production behavior to obtain a green verdict.
- Do not substitute a different contract, tolerance, or historical waiver.
- Treat any source digest change as a new proof candidate; historical receipts remain historical.
- Keep failures and unreachable properties visible rather than relabeling them.
