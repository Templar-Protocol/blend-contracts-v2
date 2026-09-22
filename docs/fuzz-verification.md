# Fuzz verification

This branch adds bounded fuzzing and deterministic replay for the production contracts already present on `main`. The fuzz change set does not modify contract source, root workspace manifests, shared test fixtures, mocks, the original pull-request workflow, or Kani sources. It does not introduce a copied policy kernel.

## Scope

The standalone `test-suites/fuzz` workspace contains seven drivers:

- `fuzz_pool_general`
- `fuzz_pool_admin`
- `fuzz_pool_auctions`
- `fuzz_pool_flash_loan`
- `fuzz_backstop_balances`
- `fuzz_emissions`
- `fuzz_pool_factory`

Inputs contain at most eight operations and 65 bytes. The drivers exercise native contract registration and freshly built optimized Wasm. The pool-factory driver's native lane registers the real `PoolFactoryContract`; the other drivers intentionally use the existing shared fixture, including its mock factory.

Main's pool constructor rejects any nonzero backstop take rate, and a failing
constructor surfaces through `deploy_v2` as a host `Context` error instead of a
typed contract error. The factory driver therefore generates only the zero
success rate and factory-rejected rates at or above the 10,000,000 boundary;
the `0 < rate < 10,000,000` band, which main's factory accepts but no pool can
construct, is outside the strict contract-error domain and is not fuzzed.

The committed corpus contains 17 seeds. The standalone library contains 17 regression tests, including native/Wasm rejection classification, decoder bounds, accounting and health mutation checks, real-client pool-status transitions, and an independent status-oracle boundary test.

Only typed Soroban `Contract` errors count as rejected operations. Conversion failures, invocation aborts, and host faults fail the harness. Operations that require an auction or queued reserve first check that precondition and report a no-op when it is absent. This keeps expected missing-state behavior separate from actual host faults.

Native/Wasm replay requires exact `RunReport` equality. The report distinguishes decode outcomes and records executed operations as applied, rejected, or no-op. Bounded libFuzzer campaigns use ASan, a 65-byte maximum input, deterministic seeds, one target at a time, and the repository's fail-closed cgroup-v2 guard.

## Independent status oracle

The pool-admin driver computes expected status transitions privately rather than importing production policy. It uses explicit 30%, 50%, 60%, and 75% queued-withdrawal boundaries and exact error codes 1200 and 1204. Its backstop-threshold predicate floors nonnegative fixture balances to whole seven-decimal units, evaluates `BLND^4 * USDC` with `BigInt`, and compares against $10^{25}$.

That threshold comparison is intentionally claimed only for the funded fixture's nonnegative balance domain. Positive saturation in the production calculation occurs above the comparison threshold, so saturation does not change the Boolean result in this domain. No signed-domain equivalence is claimed.

## Commands

The root `justfile` is the branch interface:

```text
just verify-doctor
just verify-self-test
just --unstable _verification-fetch
just build
just test-fuzz
just test
just fuzz-replay all both
just fuzz all 60 1
```

`just verify-list fuzz all` must enumerate exactly seven targets. Unknown targets and the unsupported `kani` kind fail. Dependency fetches use both lockfiles and verify that neither changes.

The guard applies a 4 GiB process-tree memory limit, disables swap, limits the service to 256 tasks and 200% CPU, serializes verification, forwards cancellation, and checks for surviving processes. It fails closed when the requested containment cannot be established.

## Evidence limits

This is bounded, deterministic test evidence, not exhaustive verification.

- Authorization is mocked by the shared fixture; the drivers do not provide an authorization oracle.
- Fixture Soroban budgets are unlimited, so campaigns do not establish production budget safety.
- Accounting and health checks are independent only over the modeled assets, prices, rounding rules, and funded fixture states they inspect.
- The auction driver covers user liquidation (type 0) and interest (type 2) paths; it does not directly validate the complete type 1 `BadDebtAuction` lifecycle.
- Eight-operation, 65-byte inputs and time-bounded campaigns leave reachable sequences unexplored.
- Exact native/Wasm report parity depends on the Soroban SDK, host, Wasm runtime, compiler, and generated contract clients. It does not prove those external components correct.
- The evidence does not prove authorization enforcement, ledger atomicity, deployment behavior, SDK correctness, or absence of defects.

Local runs and uploaded artifacts are not hosted-CI evidence unless the corresponding GitHub Actions run is observed.