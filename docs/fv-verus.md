# Verus operational guide

[`VERIFICATION.md`](../VERIFICATION.md) owns proof results, mathematical domains
and the production-refinement boundary. This guide covers the pinned Verus tools
and their invocation through the [shared local gate](../VERIFICATION.md#reproducing-this).

## Toolchain

Use Linux and Python 3.11+. Provision the Verus release, bundled solver/libraries
and isolated Rust compiler pinned in
[`verification/verus/toolchain.json`](../verification/verus/toolchain.json).
That file contains the download URLs, checksums and Rust setup commands; do not
substitute a newer release or the workspace compiler.

The default home is `/tmp/blend-verification-tools/verus`; `VERUS_HOME` may select
the same layout elsewhere:

```text
<VERUS_HOME>/verus-x86-linux/   # verifier, solver and bundled libraries
<VERUS_HOME>/rustup/           # pinned compiler
<VERUS_HOME>/cargo/            # isolated Cargo home
```

The home and its parent must be owned by the operator and not group-/world-writable.
The runner validates custody, file hashes and compiler identity before execution.
Tool installation is explicit; verification does not install a missing toolchain.

## Execution and verdicts

Run `make verify-local` from the repository root after the shared prerequisites
are satisfied. [`scripts/verify_verus.py`](../scripts/verify_verus.py) selects
[`supply_delta.rs`](../verification/verus/supply_delta.rs), enables `--no-cheating`,
uses one verifier thread and checks the structured JSON result against the exact
required function set and pinned commit.

The gate also generates an isolated floor-to-ceil mint mutation. It must produce
assertion failures in the exact expected functions; a timeout, parser error or
compiler failure is not an accepted negative control. The original proof source
is not modified.

Read `verus-positive.log` and `verus-negative.log` under
`target/verification-local/`. Do not infer success from process exit status alone
or transfer a mathematical theorem to production Rust without the refinement
identified in the main report.
