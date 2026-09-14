//! Artifact loading for externally supplied BASE/FORK runtime Wasm.
//!
//! Reads each artifact path from its own dedicated ADR8_*_WASM env var; any
//! absent variable is a hard failure with an unmistakable message. Byte
//! hashes are host-computed via a scratch Env's `deployer().upload_contract_wasm`
//! BEFORE any fixture setup, so they double as predeclared identity anchors.

use std::{env, fs};

/// One externally supplied runtime artifact.
pub struct RuntimeArtifact {
    pub label: &'static str,
    pub wasm: std::vec::Vec<u8>,
    /// Host-computed sha256 of `wasm` bytes (filled by DiffBundle).
    pub hash: [u8; 32],
}

impl RuntimeArtifact {
    fn new(label: &'static str, env_var: &'static str) -> Self {
        let path = env::var(env_var).unwrap_or_else(|_| {
            panic!(
                "differential harness: required environment variable {} is \
                 absent; set all four ADR8_*_WASM variables to run this harness",
                env_var
            )
        });
        let wasm = fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "differential harness: cannot read {} artifact at {}: {}",
                label, path, e
            )
        });
        if wasm.is_empty() {
            panic!(
                "differential harness: {} artifact at {} is empty",
                label, path
            );
        }
        Self {
            label,
            wasm,
            hash: [0u8; 32],
        }
    }
}

/// All four artifacts needed to run one differential comparison.
pub struct DiffBundle {
    pub base_pool: RuntimeArtifact,
    pub base_backstop: RuntimeArtifact,
    pub fork_pool: RuntimeArtifact,
    pub fork_backstop: RuntimeArtifact,
}

impl DiffBundle {
    pub fn from_env() -> Self {
        Self {
            base_pool: RuntimeArtifact::new("base-pool", "ADR8_BASE_POOL_WASM"),
            base_backstop: RuntimeArtifact::new("base-backstop", "ADR8_BASE_BACKSTOP_WASM"),
            fork_pool: RuntimeArtifact::new("fork-pool", "ADR8_FORK_POOL_WASM"),
            fork_backstop: RuntimeArtifact::new("fork-backstop", "ADR8_FORK_BACKSTOP_WASM"),
        }
    }

    /// Compute all four artifact hashes in scratch Envs before any setup.
    pub fn compute_hashes(&mut self) {
        self.base_pool.hash = host_wasm_hash(&self.base_pool.wasm);
        self.base_backstop.hash = host_wasm_hash(&self.base_backstop.wasm);
        self.fork_pool.hash = host_wasm_hash(&self.fork_pool.wasm);
        self.fork_backstop.hash = host_wasm_hash(&self.fork_backstop.wasm);
    }

    /// Print artifact hash anchors (also the opt-in evidence transcript line).
    pub fn print_hashes(&self) {
        println!("adr8 differential artifact hashes:");
        println!("  base pool      {:02x?}", self.base_pool.hash);
        println!("  base backstop  {:02x?}", self.base_backstop.hash);
        println!("  fork pool      {:02x?}", self.fork_pool.hash);
        println!("  fork backstop  {:02x?}", self.fork_backstop.hash);
    }
}

fn host_wasm_hash(wasm: &[u8]) -> [u8; 32] {
    scratch_env()
        .deployer()
        .upload_contract_wasm(wasm)
        .to_array()
}

/// Minimal deterministic Env used only for wasm-hash derivation.
///
/// Mirrors the ordinary fixture ledger settings so upload fees never trap.
pub fn scratch_env() -> soroban_sdk::Env {
    use soroban_sdk::{
        testutils::{EnvTestConfig, Ledger as _, LedgerInfo},
        Env,
    };
    let e = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    e.mock_all_auths();
    e.cost_estimate().budget().reset_unlimited();
    e.ledger().set(LedgerInfo {
        timestamp: 1441065600,
        protocol_version: 22,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 500000,
        min_persistent_entry_ttl: 500000,
        max_entry_ttl: 9999999,
    });
    e
}
