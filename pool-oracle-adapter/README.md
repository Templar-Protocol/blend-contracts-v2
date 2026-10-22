# pool-oracle-adapter

A thin multi-asset SEP-40 price-oracle adapter over the Templar proxy oracle,
for use as a Blend v2 pool oracle.

- License: MIT (see `LICENSE`). The rest of this repository keeps its own licenses.
- Imported from Templar-Protocol/precept-internal@324fc5b
  (`contracts/pool-oracle-adapter`, the audited L-05 revision).
- Gate: `just audit-ready` (needs `stellar` CLI 28.1.0, Rust 1.98.0 with
  `wasm32v1-none`, and a `pool-adapter-env` wrapper on PATH; see the CI workflow).
