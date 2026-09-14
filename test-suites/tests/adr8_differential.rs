//! ADR0008 Step8 combined-with-ADR0011-Step5 runtime differential runner.
//!
//! NOT part of the ordinary suite: invoked explicitly by Main with the four
//! artifact env vars (ADR8_BASE_POOL_WASM, ADR8_BASE_BACKSTOP_WASM,
//! ADR8_FORK_POOL_WASM, ADR8_FORK_BACKSTOP_WASM). Absent variables are an
//! explicit hard failure naming every missing path — never a silent pass,
//! and never evidence of coverage that did not happen.

#![cfg(test)]

use test_suites::differential::{run_full_diff, DiffBundle};

#[test]
#[ignore]
fn adr8_base_fork_differential() {
    let mut missing: Vec<&'static str> = Vec::new();
    for var in [
        "ADR8_BASE_POOL_WASM",
        "ADR8_BASE_BACKSTOP_WASM",
        "ADR8_FORK_POOL_WASM",
        "ADR8_FORK_BACKSTOP_WASM",
    ] {
        let absent = match std::env::var(var) {
            Err(_) => true,
            Ok(v) => v.is_empty(),
        };
        if absent {
            missing.push(var);
        }
    }
    assert!(
        missing.is_empty(),
        "adr8 differential runner requires all four artifact env vars; missing: {:?}. \
         Invoke explicitly with the BASE baseline {{pool,backstop}}.wasm files and the \
         FORK optimized artifacts under target/wasm32-unknown-unknown/optimized/. \
         Ordinary cargo runs skip this #[ignore] test.",
        missing,
    );

    let mut bundle = DiffBundle::from_env();
    run_full_diff(&mut bundle);
}
