# Blend Protocol V2

This repository contains the smart contacts for an implementation of the Blend Protocol. Blend is a universal liquidity protocol primitive that enables the permissionless creation of lending pools.

## Documentation

To learn more about the Blend Protocol, visit the docs:

- [Blend Docs](https://docs.blend.capital/)

## Audits

Conducted audits can be viewed in the `audits` folder.

## Getting Started

Build the contracts with:

```
make
```

Run all unit tests and the integration test suite with:

```
make test

Explicit base-versus-fork differential (ADR 0008 / ADR 0011; builds the pinned stock baseline from git). Run inside `devenv shell`, which provides the pinned Stellar CLI:

```
make differential
```
```

## Deployment

The `make` command creates an optimized and un-optimized set of WASM contracts. It's recommended to use the optimized version if deploying to a network.

These can be found at the path:

```
target/wasm32-unknown-unknown/optimized
```

For help with deployment to a network, please visit the [Blend Utils](https://github.com/blend-capital/blend-utils) repo.

## Minimal Kani proof attempt (findings)

A minimally invasive Kani proof attempt was made against this branch
(`verification-kani-minimal`, based on `main` at `7b63baf`) with zero changes to
production function bodies, signatures, arithmetic, or dependency versions.
Five bounded harnesses were prepared over main's unchanged backstop policy
functions — `is_pool_above_threshold`, `PoolBalance::convert_to_shares`, and
`PoolBalance::convert_to_tokens` — restricted to `#[cfg(kani)]` wiring plus a
gated proof module.

**Result: zero proofs were demonstrated; the attempt is blocked at common
crate compilation.** Discovery (`cargo kani -p backstop --lib --manifest-path
Cargo.toml list --format json`, Kani 0.68.0) fails before any harness compiles
because the mandatory `soroban-sdk` dependency chain (`soroban-env-host` →
`ethnum` 1.5.0) does not compile under the Kani compiler:

```text
error[E0512]: cannot transmute between types of different sizes, or dependently-sized types
  --> ethnum-1.5.0/src/error.rs:16:14
   |
16 |     unsafe { mem::transmute(()) }
   |              ^^^^^^^^^^^^^^
   |   = note: source type: `()` (0 bits)
   |   = note: target type: `core::num::TryFromIntError` (8 bits)
```

All five candidates (threshold boundary, threshold zero-axis, threshold
saturation, share-conversion floor, token-conversion floor) were blocked by
this common dependency failure, so none was attempted or rejected
individually. Patching the dependency, altering production code, or proving a
copied policy crate instead of main's functions was out of scope by design.
The proof module, registry, runner, and workflow were removed rather than
shipping infrastructure that cannot execute; the full extracted-kernel Kani
campaign remains preserved on the `verification-review-fixes` branch (PR #5).

## Contributing

- Under no circumstances should the "overflow-checks" flag be removed otherwise contract math will become unsafe

## Community Links

A set of links for various things in the community. Please submit a pull request if you would like a link included.

- [Blend Discord](https://discord.com/invite/a6CDBQQcjW)
