//! Runtime differential harness for ADR0008 Step8 combined with ADR0011 Step5.
//!
//! Compares externally supplied BASE and FORK pool/backstop Wasm through the
//! shared fixture (`create_fixture_with_wasm`) by replaying identical
//! common-domain scenario prefixes and diffing the SDK's complete
//! `env.to_snapshot()` envelope — full ledger with explicit TTLs, ALL events
//! (System + Contract) with per-event failed_call flags, and the complete
//! auth history — after applying ONLY narrowly-declared executable
//! normalization:
//!
//! * `LedgerKey::ContractCode` entries keyed by that side's own recorded pool
//!   or backstop wasm hash: key hash AND entry identity substituted to distinct
//!   fixed role markers; code bytes and derived cost inputs erased only here;
//! * `ScContractInstance.executable` of exactly the declared runtime contracts
//!   (matched by address bytes, not key shape);
//! * factory instance-storage PoolMeta.pool_hash (ScVal::Map field).
//!
//! Everything else — tokens/balances/allowances/positions/reserves/emissions/
//! TTLs/errors/events/auth/unknown contract/code entries — must match exactly.
//! Missing storage context is a defect, never an allowed skip. Negative
//! controls perturb real captured bytes; each must be flagged by this module's
//! single comparator.

pub mod artifact;
pub mod negative_controls;
pub mod observation;
pub mod scenarios;

pub use artifact::DiffBundle;
pub use observation::{Capture, Observation, ObservedStep};

use crate::test_fixture::{RUNTIME_BACKSTOP_ID, RUNTIME_FACTORY_ID, RUNTIME_POOL_SALT};
use soroban_sdk::xdr::{
    ContractDataEntry, ContractExecutable, Hash as XdrHash, LedgerEntryData, LedgerKey, ScAddress,
    ScVal,
};

/// Fixed zero identity for normalized executable references during compare.
const ZERO_HASH: [u8; 32] = [0u8; 32];
/// Keep code-row ownership distinct even when all other normalized bytes match.
const BACKSTOP_CODE_HASH: [u8; 32] = [1u8; 32];

/// Factory instance-storage PoolMeta symbol key.
const POOL_META_KEY: &str = "PoolMeta";

/// Hashes computed BEFORE setup from externally supplied artifacts.
#[derive(Clone)]
pub struct SideHashes {
    pub pool: [u8; 32],
    pub backstop: [u8; 32],
}

impl SideHashes {
    /// Predeclared identity anchors, built from artifact hashes already
    /// computed by `DiffBundle::compute_hashes` — no scratch re-upload.
    pub fn from_hashes(pool: [u8; 32], backstop: [u8; 32]) -> Self {
        Self { pool, backstop }
    }
}

/// Predeclared runtime identities fixed before any setup runs.
pub fn declare_runtime_identity() -> ([u8; 32], [u8; 32], [u8; 32]) {
    (RUNTIME_BACKSTOP_ID, RUNTIME_FACTORY_ID, RUNTIME_POOL_SALT)
}

/// One side's completed replay.
#[derive(Clone)]
pub struct SideCapture {
    pub label: &'static str,
    /// Pool contract id recorded by the scenario module during replay.
    pub pool_id: [u8; 32],
    /// BLND token id (fixture.tokens[TokenIndex::BLND]) for token controls.
    pub blnd_id: [u8; 32],
    /// First reserve asset of the replayed pool (reserve-accrual targets).
    pub reserve_asset_id: [u8; 32],
    /// Emitter contract id as created by the fixture (recorded for controls).
    pub emitter_id: [u8; 32],
    /// This side's runtime backstop/factory ids (declaration + controls).
    pub backstop_id: [u8; 32],
    pub factory_id: [u8; 32],
    pub hashes: SideHashes,
    pub capture: Capture,
}

// ---------------------------------------------------------------------------
// Normalization on the snapshot envelope
// ---------------------------------------------------------------------------

fn addr_bytes(addr: &ScAddress) -> [u8; 32] {
    match addr {
        ScAddress::Contract(XdrHash(bytes)) => *bytes,
        other => panic!(
            "differential harness: unexpected non-contract address {:?}",
            other
        ),
    }
}

/// SDK 22.0.7 has no `Address::to_bytes`; go through the verified
/// `From<&Address> for ScAddress` conversion and read the contract hash.
pub(crate) fn addr_bytes_of(addr: &soroban_sdk::Address) -> [u8; 32] {
    match ScAddress::from(addr) {
        ScAddress::Contract(XdrHash(bytes)) => bytes,
        other => panic!(
            "differential harness: address is not a contract id ({:?})",
            other
        ),
    }
}

fn normalize_code_entry(entry: &mut soroban_sdk::xdr::LedgerEntry, role: [u8; 32]) {
    if let LedgerEntryData::ContractCode(code_entry) = &mut entry.data {
        code_entry.hash = XdrHash(role);
        code_entry.code = std::vec::Vec::<u8>::new().try_into().expect("empty code");
        // Derived cost accounting must normalize too (ADR0008 surface);
        // V0 has no cost inputs, so nothing else to touch there.
        if let soroban_sdk::xdr::ContractCodeEntryExt::V1(cost) = &mut code_entry.ext {
            let c = &mut cost.cost_inputs;
            *c = soroban_sdk::xdr::ContractCodeCostInputs {
                ext: soroban_sdk::xdr::ExtensionPoint::V0,
                n_instructions: 0,
                n_functions: 0,
                n_globals: 0,
                n_table_entries: 0,
                n_types: 0,
                n_data_segments: 0,
                n_elem_segments: 0,
                n_imports: 0,
                n_exports: 0,
                n_data_segment_bytes: 0,
            };
        }
    }
}

fn zero_instance_executable(val: &mut ScVal, expected: [u8; 32]) {
    if let ScVal::ContractInstance(inst) = val {
        match &mut inst.executable {
            ContractExecutable::Wasm(h) => {
                assert_eq!(*h, XdrHash(expected), "unexpected instance executable hash");
                *h = XdrHash(ZERO_HASH);
            }
            other => panic!(
                "differential harness: unexpected non-wasm executable {:?} \
                 on declared runtime contract",
                other
            ),
        }
    } else {
        panic!("differential harness: missing instance payload");
    }
}

/// Apply the narrowly-declared executable normalization to one side's full
/// snapshot. Panics on missing/extra storage context rather than skipping.
///
/// `pool_id`/`backstop_id` are THIS side's recorded contract addresses.
pub fn normalize_executables(
    obs: &Observation,
    hashes: &SideHashes,
    pool_id: &[u8; 32],
    backstop_id: &[u8; 32],
    factory_id: &[u8; 32],
) -> Observation {
    let mut out = obs.clone();

    // 1+2. Code entries keyed by this side's own hashes and instance fields.
    let mut normalized_code_keys = [0usize; 2];
    let mut normalized_instances = [0usize; 2];

    for (key, entry_ttl) in out.snap.ledger.ledger_entries.iter_mut() {
        match key.as_ref() {
            LedgerKey::ContractCode(code_key) => {
                if code_key.hash == XdrHash(hashes.pool)
                    || code_key.hash == XdrHash(hashes.backstop)
                {
                    let LedgerEntryData::ContractCode(code) = &entry_ttl.0.data else {
                        panic!("code key has non-code payload");
                    };
                    assert_eq!(code.hash, code_key.hash, "code key/payload hash mismatch");
                    let index = usize::from(code_key.hash == XdrHash(hashes.backstop));
                    let role = if index == 0 {
                        ZERO_HASH
                    } else {
                        BACKSTOP_CODE_HASH
                    };
                    normalize_code_entry(entry_ttl.0.as_mut(), role);
                    if let LedgerKey::ContractCode(code_key) = key.as_mut() {
                        code_key.hash = XdrHash(role);
                    } else {
                        unreachable!("normalized runtime code key");
                    }
                    normalized_code_keys[index] += 1;
                }
                // Other code entries stay exact bytes.
            }
            LedgerKey::ContractData(data_key) => {
                if data_key.key != ScVal::LedgerKeyContractInstance {
                    continue;
                }
                let owner = addr_bytes(&data_key.contract);
                if owner != *pool_id && owner != *backstop_id && owner != *factory_id {
                    continue;
                }
                let entry_box = &mut entry_ttl.0;
                if let LedgerEntryData::ContractData(ContractDataEntry { val, .. }) =
                    &mut entry_box.data
                {
                    // Factory is addressed for its PoolMeta below; here only
                    // true Wasm instances of the two runtime contracts.
                    if owner == *factory_id {
                        continue;
                    }
                    let index = usize::from(owner == *backstop_id);
                    let expected = if index == 0 {
                        hashes.pool
                    } else {
                        hashes.backstop
                    };
                    zero_instance_executable(val, expected);
                    normalized_instances[index] += 1;
                } else {
                    panic!(
                        "differential harness: missing ContractData payload for \
                         declared runtime contract"
                    );
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        normalized_code_keys,
        [1, 1],
        "expected one code entry per runtime role"
    );
    assert_eq!(
        normalized_instances,
        [1, 1],
        "missing or duplicate runtime instance"
    );

    normalize_pool_meta_hash(&mut out, factory_id, hashes.pool);
    out.snap.ledger.ledger_entries.sort();

    out
}

// ---------------------------------------------------------------------------
// Factory PoolMeta.pool_hash substitution
// ---------------------------------------------------------------------------

/// Substitute ONLY the `pool_hash` 32-byte value inside the declared factory
/// instance-storage PoolMeta map. All other PoolMeta fields, storage keys,
/// TTLs and unrelated metadata stay byte-exact. Panics when absent/duplicate.
fn normalize_pool_meta_hash(out: &mut Observation, factory_id: &[u8; 32], expected: [u8; 32]) {
    let target = ScAddress::Contract(XdrHash(*factory_id));
    let mut hits = 0usize;

    for (key, entry_ttl) in out.snap.ledger.ledger_entries.iter_mut() {
        let LedgerKey::ContractData(data_key) = key.as_ref() else {
            continue;
        };
        if data_key.contract != target {
            continue;
        }
        if data_key.key != ScVal::LedgerKeyContractInstance {
            continue;
        }
        let entry_box = &mut entry_ttl.0;
        let LedgerEntryData::ContractData(ContractDataEntry { val, .. }) = &mut entry_box.data
        else {
            panic!("differential harness: factory instance payload missing");
        };
        let ScVal::ContractInstance(inst) = val else {
            panic!("differential harness: factory instance val missing");
        };
        let Some(storage_map) = inst.storage.as_ref() else {
            panic!(
                "differential harness: factory instance storage absent — \
                 PoolMeta must exist after a fixture deploy"
            );
        };
        // stellar-xdr VecM derefs immutably only: edit via Vec round-trip,
        // preserving entry order (boring, no new dependencies).
        let mut entries: std::vec::Vec<soroban_sdk::xdr::ScMapEntry> = storage_map.to_vec();
        let mut found = false;
        for entry_pair in entries.iter_mut() {
            let matches_key = matches!(&entry_pair.key, ScVal::Symbol(sym)
                if sym.0.as_slice() == POOL_META_KEY.as_bytes());
            if !matches_key {
                continue;
            }
            assert!(!found, "differential harness: duplicate PoolMeta entry");
            found = true;
            let ScVal::Map(Some(meta_map)) = &mut entry_pair.val else {
                panic!("differential harness: PoolMeta is not a Map");
            };
            let mut fields: std::vec::Vec<soroban_sdk::xdr::ScMapEntry> = meta_map.to_vec();
            let mut seen = false;
            for kv in fields.iter_mut() {
                let is_pool_hash = matches!(&kv.key, ScVal::Symbol(sym)
                    if sym.0.as_slice() == b"pool_hash");
                if !is_pool_hash {
                    continue;
                }
                assert!(!seen, "duplicate pool_hash field");
                seen = true;
                match &mut kv.val {
                    ScVal::Bytes(soroban_sdk::xdr::ScBytes(bytes)) => {
                        assert_eq!(
                            bytes.as_slice(),
                            expected.as_slice(),
                            "unexpected factory pool_hash"
                        );
                        *bytes = ZERO_HASH.to_vec().try_into().expect("32-byte pool hash");
                    }
                    other => panic!(
                        "differential harness: pool_hash not Bytes (got {:?})",
                        other
                    ),
                }
            }
            assert!(seen, "differential harness: pool_hash field absent");
            *meta_map = <soroban_sdk::xdr::ScMap as TryFrom<
                std::vec::Vec<soroban_sdk::xdr::ScMapEntry>,
            >>::try_from(fields)
            .expect("meta map rebuild");
        }
        assert!(
            found,
            "differential harness: PoolMeta entry absent in factory storage"
        );
        inst.storage =
            Some(<soroban_sdk::xdr::ScMap as TryFrom<
                std::vec::Vec<soroban_sdk::xdr::ScMapEntry>,
            >>::try_from(entries)
            .expect("storage map rebuild"));
        hits += 1;
    }

    assert_eq!(
        hits, 1,
        "differential harness: exactly one factory instance expected, found {}",
        hits
    );
}

// ---------------------------------------------------------------------------
// Comparison — single comparator shared by real diff and negative controls
// ---------------------------------------------------------------------------

/// First divergence found between two ALREADY-NORMALIZED observations, if any.
///
/// Compare separately: ledger info+entries, events (with failed_call flags),
/// auth history — never whole-Snapshot equality, which would wrongly require
/// identical generator state across distinct Envs. result_repr compares
/// verbatim. This is THE comparator; negative controls reuse it unchanged.
pub(crate) fn compare_states(
    base: &Observation,
    fork: &Observation,
) -> Option<std::string::String> {
    let b = &base.snap;
    let f = &fork.snap;

    // Ledger header fields.
    let li_equal = b.ledger.protocol_version == f.ledger.protocol_version
        && b.ledger.sequence_number == f.ledger.sequence_number
        && b.ledger.timestamp == f.ledger.timestamp
        && b.ledger.network_id == f.ledger.network_id
        && b.ledger.base_reserve == f.ledger.base_reserve
        && b.ledger.min_persistent_entry_ttl == f.ledger.min_persistent_entry_ttl
        && b.ledger.min_temp_entry_ttl == f.ledger.min_temp_entry_ttl
        && b.ledger.max_entry_ttl == f.ledger.max_entry_ttl;
    if !li_equal {
        return Some("ledger info mismatch".into());
    }

    // Ledger entries: count then per-row key/value/TTL.
    if b.ledger.ledger_entries.len() != f.ledger.ledger_entries.len() {
        return Some(format!(
            "ledger entry count mismatch: {} vs {}",
            b.ledger.ledger_entries.len(),
            f.ledger.ledger_entries.len()
        ));
    }
    for (i, ((bk, (bentry, bttl)), (fk, (fentry, fttl)))) in b
        .ledger
        .ledger_entries
        .iter()
        .zip(f.ledger.ledger_entries.iter())
        .enumerate()
    {
        if bk != fk {
            return Some(format!("entry {} key mismatch", i));
        }
        if bentry != fentry {
            return Some(format!("entry {} value mismatch", i));
        }
        if bttl != fttl {
            return Some(format!("entry {} TTL mismatch", i));
        }
    }

    // Events: complete cumulative vectors incl. failed_call flags.
    if b.events.0.len() != f.events.0.len() {
        return Some(format!(
            "event count mismatch: {} vs {}",
            b.events.0.len(),
            f.events.0.len()
        ));
    }
    for (i, (bev, fev)) in b.events.0.iter().zip(f.events.0.iter()).enumerate() {
        if bev != fev {
            return Some(format!("event {} mismatch", i));
        }
    }

    // Auth: complete history.
    if b.auth.0.len() != f.auth.0.len() {
        return Some(format!(
            "auth count mismatch: {} vs {}",
            b.auth.0.len(),
            f.auth.0.len()
        ));
    }
    for (i, (bauth, fauth)) in b.auth.0.iter().zip(f.auth.0.iter()).enumerate() {
        if bauth != fauth {
            return Some(format!("auth trace {} mismatch", i));
        }
    }

    // Top-level invocation outcome repr.
    if base.result_repr != fork.result_repr {
        return Some(format!(
            "result repr mismatch: {:?} vs {:?}",
            base.result_repr, fork.result_repr
        ));
    }

    None
}

/// Deterministic common-domain evidence retention under ADR8_DIFF_OUTPUT_DIR,
/// following the stock-control convention (per-case directory, one Snapshot
/// JSON per stage plus a `-result.txt` verdict record). Case directories carry
/// a zero-padded step index because labels repeat across scenarios. Evidence
/// is written BEFORE any divergence panic so failed comparisons keep their
/// raw captures, normalized snapshots and comparison result.
fn save_observation(case: &std::path::Path, stage: &str, obs: &Observation) {
    let path = case.join(format!("{stage}.json"));
    obs.snap
        .write_file(&path)
        .expect("complete SDK Snapshot retained");
    let roundtrip = soroban_sdk::testutils::Snapshot::read_file(&path).unwrap();
    assert_eq!(
        &roundtrip, &obs.snap,
        "snapshot custody round-trip changed bytes"
    );
}

fn retain_common_domain_evidence(
    case_index: usize,
    label: &str,
    base_raw: &Observation,
    fork_raw: &Observation,
    base_norm: &Observation,
    fork_norm: &Observation,
    divergence: Option<&std::string::String>,
) {
    let root = std::path::PathBuf::from(
        std::env::var_os("ADR8_DIFF_OUTPUT_DIR")
            .expect("ADR8_DIFF_OUTPUT_DIR must name retained evidence directory"),
    );
    assert!(!root.as_os_str().is_empty());
    let case = root.join(format!("{case_index:03}-{label}"));
    std::fs::create_dir_all(&case).unwrap();
    save_observation(&case, "raw-base", base_raw);
    save_observation(&case, "raw-fork", fork_raw);
    save_observation(&case, "normalized-base", base_norm);
    save_observation(&case, "normalized-fork", fork_norm);
    std::fs::write(
        case.join(format!("step-{case_index:03}-result.txt")),
        format!(
            "case={case_index:03}-{label}\n\
             base_result_repr={:?}\n\
             fork_result_repr={:?}\n\
             normalized_divergence={}\n",
            base_raw.result_repr,
            fork_raw.result_repr,
            match divergence {
                Some(msg) => format!("Some({msg:?})"),
                None => std::string::String::from("None"),
            }
        ),
    )
    .unwrap();
}

/// Verify both sides replayed identical step inventories before comparison.
pub fn verify_alignment(base: &Capture, fork: &Capture) {
    assert_eq!(
        base.step_count(),
        fork.step_count(),
        "differential harness: BASE/FORK replay step count mismatch"
    );
    for (b_step, f_step) in base.steps.iter().zip(fork.steps.iter()) {
        assert_eq!(
            b_step.label, f_step.label,
            "differential harness: replay step label order mismatch"
        );
    }
}

/// Compare every step under executable normalization, retaining raw +
/// normalized common-domain evidence and the comparison verdict per case;
/// panics on divergence after that case's evidence is retained.
fn differential_match_steps(base_side: &SideCapture, fork_side: &SideCapture) {
    let (backstop_id, factory_id, _salt) = declare_runtime_identity();
    assert_eq!(
        base_side.pool_id, fork_side.pool_id,
        "differential harness: fixed pool salt produced divergent pool ids"
    );
    let norm = |obs: &Observation, side: &SideCapture| {
        normalize_executables(obs, &side.hashes, &side.pool_id, &backstop_id, &factory_id)
    };

    // init
    let b_raw = base_side.capture.initial_obs();
    let f_raw = fork_side.capture.initial_obs();
    let b_norm = norm(b_raw, base_side);
    let f_norm = norm(f_raw, fork_side);
    let divergence = compare_states(&b_norm, &f_norm);
    retain_common_domain_evidence(
        0,
        "init",
        b_raw,
        f_raw,
        &b_norm,
        &f_norm,
        divergence.as_ref(),
    );
    if let Some(msg) = divergence {
        panic!("[init] {}", msg);
    }

    for (i, (b_step, f_step)) in base_side
        .capture
        .steps
        .iter()
        .zip(fork_side.capture.steps.iter())
        .enumerate()
    {
        assert_eq!(
            b_step.label, f_step.label,
            "differential harness: replay step label order mismatch"
        );
        let index = i + 1;
        let b_norm = norm(&b_step.observation, base_side);
        let f_norm = norm(&f_step.observation, fork_side);
        let divergence = compare_states(&b_norm, &f_norm);
        retain_common_domain_evidence(
            index,
            b_step.label,
            &b_step.observation,
            &f_step.observation,
            &b_norm,
            &f_norm,
            divergence.as_ref(),
        );
        if let Some(msg) = divergence {
            panic!("[{}] {}", b_step.label, msg);
        }
    }
}

/// Build BASE and FORK fixtures, replay shared prefixes, compare every step,
/// then run the mutation-based negative controls through the same comparator.
///
/// Panics on any undeclared divergence or missed negative-control detection.
pub fn run_full_diff(bundle: &mut DiffBundle) {
    bundle.compute_hashes();
    bundle.print_hashes();

    let base_side = scenarios::replay_all(
        SideHashes::from_hashes(bundle.base_pool.hash, bundle.base_backstop.hash),
        bundle.base_pool.wasm.as_slice(),
        bundle.base_backstop.wasm.as_slice(),
        "BASE",
    );
    let fork_side = scenarios::replay_all(
        SideHashes::from_hashes(bundle.fork_pool.hash, bundle.fork_backstop.hash),
        bundle.fork_pool.wasm.as_slice(),
        bundle.fork_backstop.wasm.as_slice(),
        "FORK",
    );

    verify_alignment(&base_side.capture, &fork_side.capture);
    differential_match_steps(&base_side, &fork_side);
    negative_controls::run_negative_controls(&base_side);
    crate::adr8_stock_controls::run_stock_controls(
        &bundle.base_pool.wasm,
        &bundle.base_backstop.wasm,
        &bundle.fork_pool.wasm,
        &bundle.fork_backstop.wasm,
    );

    println!("adr8 differential: all common-domain steps matched; negative controls detected every mutation");
}
