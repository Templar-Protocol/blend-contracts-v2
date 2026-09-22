# Blend Protocol V2

This repository contains the smart contracts for an implementation of the Blend Protocol. Blend is a universal liquidity protocol primitive that enables the permissionless creation of lending pools.

## Documentation

To learn more about the Blend Protocol, visit the docs:

- [Blend Docs](https://docs.blend.capital/)

## Audits

Conducted audits can be viewed in the `audits` folder.

## Getting Started

Build the contracts with the bounded root entrypoint:

```console
just build
```

Run all unit and integration tests with:

```console
just test
```

Both commands preserve the existing Makefile behavior while running it through
the verification resource guard described below.

## Verification

The root `justfile` is the command interface for fuzzing and bounded model
checking. Every build, test, fuzz campaign, replay, and proof starts inside a
transient cgroup before source compilation. The guard fails closed unless it
can verify the effective limits.

For the design rationale, claim boundaries, evidence model, and security
audit-readiness assessment, see
[Verification Architecture and Audit Readiness](docs/verification-and-audit-readiness.md).

### Prerequisites

- Linux with systemd and a delegated cgroup v2 memory controller
- Bash, jq, flock, Python 3, Make, Cargo, rustup, sha256sum, tee, and mkfifo
- Just 1.51.0 or newer
- Rust 1.81 with `wasm32-unknown-unknown`
- Stellar CLI 22.6.0
- `nightly-2025-11-25` with `rust-src`
- cargo-fuzz 0.13.2
- Kani 0.68.0 with its pinned CBMC installed by `cargo kani setup`

Run the containment checks before the expensive commands:

```console
just verify-doctor
just verify-self-test
```

`verify-self-test` exercises fail-closed startup, exit-status propagation,
timeouts, OOM kills, mutual exclusion, and SIGINT/SIGTERM cancellation. It
also checks that no workload child survives cancellation.

### Resource policy

`just verify-run` creates one transient service with these default limits:

- 4 GiB total process-tree memory and zero swap
- 256 tasks
- 200% CPU quota
- one repository-wide verification workload at a time

The runner requires the selected memory cap plus 1 GiB of available headroom.
`VERIFY_MEMORY_MIB` may lower, but never raise, the 4 GiB ceiling. Local runs
use the user systemd manager by default; CI sets
`VERIFY_SYSTEMD_MODE=system`. Each run records its command, limits, tool
versions, combined output, cgroup counters, peak memory, and exit result below
`target/verification/runs/`.

### Fuzzing

The seven registered targets exercise production pool, backstop, and factory
contracts:

```console
just verify-list fuzz all
just fuzz-replay all both
just fuzz all 60 1
```

Select one target with, for example,
`just fuzz fuzz_pool_factory 60 1`. Inputs decode to at most eight fixed-width
operations, and libFuzzer accepts at most 65 bytes: the one-byte header plus
eight eight-byte operation records. Committed seeds include deep-success and
rejection/boundary sequences. `both` replay mode requires identical native and
optimized-Wasm reports. The libFuzzer campaigns use AddressSanitizer, a
10-second per-input timeout, and a 2 GiB libFuzzer RSS limit inside the stricter
4 GiB process-tree cgroup.

The drivers assert selected accounting and state-transition invariants,
including conservative accrued reserve coverage, post-submit actor health,
exact status decisions, configuration boundaries, auction staleness,
withdrawal maturity, and claim clearing. Only Soroban errors whose runtime type
is `Contract` count as generated rejections; host, authorization, budget, and
VM faults remain harness failures. Authorization sensitivity is otherwise
checked by the existing integration suite rather than a general fuzz-driver
authorization oracle. External token, oracle, and liquidity-pool dependencies
remain deterministic Soroban test fixtures; this is not a live network or
deployment test.
Generated corpora and crash artifacts are ignored locally; CI retains crash
and run evidence for 14 days.

### Kani

The production contracts call a small `no_std` policy kernel in
`contract-kernel`. Its 38 registered harnesses prove configuration bounds,
pool status/action rules, auction schedules, backstop threshold arithmetic
and monotonicity, queue conservation and maturity, and emission allocation:

```console
just verify-list kani all
just kani
just kani backstop
just kani all kani_proofs::config::pool_config_bounds
```

Registry discovery must exactly match `cargo kani list` schema 0.1. Harnesses
run serially with `--exact`, `--jobs=1`, a 600-second per-harness timeout, and
required satisfiable `kani::cover!` obligations. Each proof emits checked JSON
under `target/verification/`; any failed, undetermined, solver-error,
unsatisfiable, missing, extra, or vacuous cover result fails the command.

The proofs establish only the stated pure-kernel properties over their
documented bounds and partitions. They do not prove Soroban host behavior,
cross-contract authorization, storage atomicity, deployment correctness, or
unbounded arithmetic. Native/Wasm replays and the existing integration suite
check those separate surfaces.

### CI

- `verification-fuzz.yml` runs containment self-tests, builds production Wasm,
  replays every seed in both modes, then runs all seven 60-second campaigns.
- `verification-kani.yml` discovers and runs all exact harnesses serially.
- Both workflows run on pull requests, relevant pushes, a weekly schedule, and
  manual dispatch. Toolchains, command-line tools, and GitHub actions are
  pinned.

### Reference conformance

The design was checked against source at fixed upstream commits:

- [rustls `eba6ba2`](https://github.com/rustls/rustls/tree/eba6ba2eedd812e119fbb6507bcc1355e5e653ba)
  uses an isolated cargo-fuzz workspace, explicit binary targets, committed
  corpora, and fixed-duration
  [CI fuzzing](https://github.com/rustls/rustls/blob/eba6ba2eedd812e119fbb6507bcc1355e5e653ba/.github/workflows/cifuzz.yml).
  Blend adopts those patterns, but pins its tools and actions, adds
  native/Wasm parity, and runs locally under a hard process-tree cap rather
  than claiming OSS-Fuzz equivalence.
- [s2n-quic `f063938`](https://github.com/aws/s2n-quic/tree/f063938a8f984d30baeddbfb4516d6c919c39c8f)
  keeps bounded Kani attributes next to production tests and uses Bolero
  generators for concrete and symbolic execution, as described in its
  [Kani guide](https://github.com/aws/s2n-quic/blob/f063938a8f984d30baeddbfb4516d6c919c39c8f/docs/dev-guide/kani.md).
  Blend adopts bounded inputs, explicit unwind/solver choices, and
  differential oracles. It uses dedicated direct Kani harnesses because the
  Soroban-facing contracts are not themselves suitable symbolic targets.
- [verify-rust-std `82ab358`](https://github.com/model-checking/verify-rust-std/tree/82ab35800882d71a84b78049ea3f46f0ea66bf07)
  discovers harnesses from versioned JSON and partitions them in
  [CI](https://github.com/model-checking/verify-rust-std/blob/82ab35800882d71a84b78049ea3f46f0ea66bf07/.github/workflows/kani.yml),
  while its
  [runner](https://github.com/model-checking/verify-rust-std/blob/82ab35800882d71a84b78049ea3f46f0ea66bf07/scripts/run-kani.sh)
  warns that individual harnesses can approach 10 GiB. Blend adopts exact
  registry validation and explicit proof-scope reporting, but deliberately
  runs one harness at a time under 4 GiB instead of inheriting upstream
  parallelism or memory assumptions.

This comparison validates reusable engineering patterns only. Upstream proof
or fuzz success is not evidence for Blend's contracts, bounds, deployment, or
resource requirements.

## Deployment

The `make` command creates an optimized and un-optimized set of WASM contracts. It's recommended to use the optimized version if deploying to a network.

These can be found at the path:

```
target/wasm32-unknown-unknown/optimized
```

For help with deployment to a network, please visit the [Blend Utils](https://github.com/blend-capital/blend-utils) repo.

## Contributing

Notes for contributors:

- Under no circumstances should the "overflow-checks" flag be removed otherwise contract math will become unsafe

## Community Links

A set of links for various things in the community. Please submit a pull request if you would like a link included.

- [Blend Discord](https://discord.com/invite/a6CDBQQcjW)
