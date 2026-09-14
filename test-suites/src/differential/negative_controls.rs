//! Mutation-based negative controls for the ADR0008 Step8 differential.
//!
//! Every control perturbs a REAL captured byte at a known semantic location
//! inside the captured [`Observation`] and proves BOTH invariants through the
//! SAME comparator used for the genuine BASE-vs-FORK comparison
//! ([`crate::differential::compare_states`]):
//!
//! 1. mutated-vs-mutated equality still holds (mutation is deterministic);
//! 2. unmutated-vs-mutated MUST be detected (comparator is not blind).
//!
//! Grounded storage shapes (all verified in installed sources, not guessed):
//! - MockToken (sep-41-token-1.2.0 testutils wasm, contractspecv0 strings):
//!   allowance row = ContractData key `Vec[Symbol("Allowance"), Map{from,
//!   spender}]`, value `Map{amount: I128, expiration_ledger: U32}`; balance =
//!   ContractData key `Symbol("Balance")` under instance entry (state rows:
//!   AllowanceDataKey/State/Balance from specv0 symbol table).
//! - Pool (blend pool/src/storage.rs): ResData row = ContractData with
//!   `PoolDataKey::ResData(Address)` serialized as
//!   `Vec[Symbol("ResData"), Address]`, value = struct map containing
//!   `b_rate` etc; EmisData = `Vec[Symbol("EmisData"), U32]` with
//!   ReserveEmissionData `{expiration, eps, index, last_time}`.
//!
//! No fake logical flags; no alternative comparison path; missing expected
//! storage context panics (a defect), never skips.

use soroban_sdk::xdr::{Int128Parts, LedgerEntryData, LedgerKey, ScVal};

use crate::differential::{compare_states, Observation, SideCapture};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn addr_bytes(sc_addr: &soroban_sdk::xdr::ScAddress) -> Option<[u8; 32]> {
    match sc_addr {
        soroban_sdk::xdr::ScAddress::Contract(h) => Some(h.0),
        _ => None,
    }
}

/// Apply `f` to every ledger entry until it returns true; panics when no
/// entry matched (missing context is a defect).
fn find_entry_mut<F>(obs: &mut Observation, mut f: F)
where
    F: FnMut(&LedgerKey, &mut soroban_sdk::xdr::LedgerEntry) -> bool,
{
    for (key_box, (entry, _ttl)) in obs.snap.ledger.ledger_entries.iter_mut() {
        if f(key_box.as_ref(), entry.as_mut()) {
            return;
        }
    }
    panic!("differential harness negative control: targeted entry absent");
}

/// Bump an i128 stored as `Int128Parts{hi:i64,lo:u64}` by +delta ≥ +1.
fn bump_i128(parts: &mut Int128Parts, delta: i128) {
    let raw = ((parts.hi as i128) << 64) | (parts.lo as u64 as i128 & u64::MAX as i128);
    let bumped = raw.wrapping_add(delta);
    *parts = Int128Parts {
        hi: (bumped >> 64) as i64,
        lo: bumped as u64,
    };
}

// ---------------------------------------------------------------------------
// 1+2. Allowance amount / expiration (fixture BLND SAC ContractData row)
// ---------------------------------------------------------------------------

pub fn mutate_allowance_amount(obs: &mut Observation, token: &[u8; 32]) {
    find_entry_mut(obs, |key, entry| {
        let LedgerKey::ContractData(data_key) = key else {
            return false;
        };
        if addr_bytes(&data_key.contract) != Some(*token)
            || data_key.key == ScVal::LedgerKeyContractInstance
        {
            return false;
        }
        // Allowance key shape: Vec[Symbol("Allowance"), Map{from,spender}]
        let ScVal::Vec(Some(kv)) = &data_key.key else {
            return false;
        };
        if kv.len() != 2 {
            return false;
        }
        if !matches!(&kv.get(0).expect("checked"), ScVal::Symbol(s)
            if String::from_utf8_lossy(&s.to_vec()) == "Allowance")
        {
            return false;
        }
        let LedgerEntryData::ContractData(ce) = &mut entry.data else {
            return false;
        };
        let ScVal::Map(Some(map)) = &mut ce.val else {
            panic!("negative control: allowance value not a struct map");
        };
        // VecM derefs immutably only: edit via Vec round-trip, order kept.
        let mut fields: std::vec::Vec<soroban_sdk::xdr::ScMapEntry> = map.to_vec();
        for field in fields.iter_mut() {
            if matches!(&field.key, ScVal::Symbol(s)
                if String::from_utf8_lossy(&s.to_vec()) == "amount")
            {
                let ScVal::I128(parts) = &mut field.val else {
                    panic!("negative control: allowance amount not i128");
                };
                bump_i128(parts, 1_000_000);
                *map = <soroban_sdk::xdr::ScMap as TryFrom<
                    std::vec::Vec<soroban_sdk::xdr::ScMapEntry>,
                >>::try_from(fields)
                .expect("allowance map rebuild");
                return true;
            }
        }
        panic!("negative control: amount field absent in allowance map");
    });
}

pub fn mutate_allowance_expiration(obs: &mut Observation, token: &[u8; 32]) {
    find_entry_mut(obs, |key, entry| {
        let LedgerKey::ContractData(data_key) = key else {
            return false;
        };
        if addr_bytes(&data_key.contract) != Some(*token)
            || data_key.key == ScVal::LedgerKeyContractInstance
        {
            return false;
        }
        let ScVal::Vec(Some(kv)) = &data_key.key else {
            return false;
        };
        if kv.len() != 2 {
            return false;
        }
        if !matches!(&kv.get(0).expect("checked"), ScVal::Symbol(s)
            if String::from_utf8_lossy(&s.to_vec()) == "Allowance")
        {
            return false;
        }
        let LedgerEntryData::ContractData(ce) = &mut entry.data else {
            return false;
        };
        let ScVal::Map(Some(map)) = &mut ce.val else {
            panic!("negative control: allowance value not a struct map");
        };
        // VecM derefs immutably only: edit via Vec round-trip, order kept.
        let mut fields: std::vec::Vec<soroban_sdk::xdr::ScMapEntry> = map.to_vec();
        for field in fields.iter_mut() {
            if matches!(&field.key, ScVal::Symbol(s)
                if String::from_utf8_lossy(&s.to_vec()) == "live_until_ledger")
            {
                let cur = match &field.val {
                    ScVal::U32(n) => *n,
                    other => panic!(
                        "negative control: live_until_ledger not u32 (got {:?})",
                        other
                    ),
                };
                field.val = ScVal::U32(cur ^ 0x5A5A_5A5A);
                *map = <soroban_sdk::xdr::ScMap as TryFrom<
                    std::vec::Vec<soroban_sdk::xdr::ScMapEntry>,
                >>::try_from(fields)
                .expect("allowance map rebuild");
                return true;
            }
        }
        panic!("negative control: live_until_ledger field absent");
    });
}

// ---------------------------------------------------------------------------
// 3. Ledger TTL mutation (explicit live_until columns)
// ---------------------------------------------------------------------------

pub fn mutate_all_ttls(obs: &mut Observation, delta: u32) {
    let mut changed = 0usize;
    for (_key, (_entry, ttl)) in obs.snap.ledger.ledger_entries.iter_mut() {
        if let Some(t) = ttl.as_mut() {
            *t = t.saturating_add(delta);
            changed += 1;
        }
    }
    assert!(
        changed > 0,
        "negative control: no explicit TTL rows present"
    );
}

/// Bump the TTL of exactly one entry: the pool ResData row for `asset`.
pub fn mutate_resdata_ttl(obs: &mut Observation, pool: &[u8; 32], asset: &[u8; 32]) {
    bump_row_ttl(obs, |k| matches_pool_row(k, pool, "ResData", asset));
}

fn is_pool_row(key_val: &ScVal, variant: &'static str, arg: &[u8; 32]) -> bool {
    let ScVal::Vec(Some(args)) = key_val else {
        return false;
    };
    if args.len() != 2 {
        return false;
    }
    let first = args.as_slice().first();
    if !matches!(first, Some(ScVal::Symbol(s)) if String::from_utf8_lossy(&s.to_vec()) == variant) {
        return false;
    }
    match args.get(1).expect("checked") {
        ScVal::Address(a) => addr_bytes(a) == Some(*arg),
        _ => false,
    }
}

fn matches_pool_row(
    key: &LedgerKey,
    pool: &[u8; 32],
    variant: &'static str,
    asset: &[u8; 32],
) -> bool {
    let LedgerKey::ContractData(data_key) = key else {
        return false;
    };
    addr_bytes(&data_key.contract) == Some(*pool) && is_pool_row(&data_key.key, variant, asset)
}

fn bump_row_ttl<F>(obs: &mut Observation, pred: F)
where
    F: Fn(&LedgerKey) -> bool,
{
    for (key_box, (_entry, ttl)) in obs.snap.ledger.ledger_entries.iter_mut() {
        if pred(key_box.as_ref()) {
            let t = ttl
                .as_mut()
                .expect("differential harness negative control: row has no explicit TTL to mutate");
            *t = t.saturating_add(3);
            return;
        }
    }
    panic!("negative control: TTL-targeted row not found");
}

// ---------------------------------------------------------------------------
// 4. Accrued reserve b_rate (pool ResData struct-map field)
// ---------------------------------------------------------------------------
pub fn mutate_reserve_accrual(obs: &mut Observation, pool: &[u8; 32], asset: &[u8; 32]) {
    find_entry_mut(obs, |key, entry| {
        if !matches_pool_row(key, pool, "ResData", asset) {
            return false;
        }
        let LedgerEntryData::ContractData(ce) = &mut entry.data else {
            return false;
        };
        let ScVal::Map(Some(map)) = &mut ce.val else {
            panic!("negative control: ResData value not a struct map");
        };
        // VecM derefs immutably only: edit via Vec round-trip, order kept.
        let mut fields: std::vec::Vec<soroban_sdk::xdr::ScMapEntry> = map.to_vec();
        for field in fields.iter_mut() {
            if matches!(&field.key, ScVal::Symbol(s)
                if String::from_utf8_lossy(&s.to_vec()) == "b_rate")
            {
                let ScVal::I128(parts) = &mut field.val else {
                    panic!("negative control: b_rate not i128");
                };
                bump_i128(parts, 10_000);
                *map = <soroban_sdk::xdr::ScMap as TryFrom<
                    std::vec::Vec<soroban_sdk::xdr::ScMapEntry>,
                >>::try_from(fields)
                .expect("ResData map rebuild");
                return true;
            }
        }
        panic!("negative control: b_rate field absent in ResData");
    });
}

// ---------------------------------------------------------------------------
// 5. Emissions accrual — inject one schema-valid contradictory EmisData row
// ---------------------------------------------------------------------------

/// Build an ScSymbol from a short string (MockToken/pool variant names).
fn symbol_m(data: &str) -> soroban_sdk::xdr::ScSymbol {
    soroban_sdk::xdr::ScSymbol(
        soroban_sdk::xdr::StringM::try_from(data.as_bytes().to_vec())
            .expect("symbol within 32 bytes"),
    )
}

/// ReserveEmissionData{expiration u64, eps u64, index i128, last_time u64}.
fn emission_data_map(index: i128) -> ScVal {
    ScVal::Map(Some(
        [
            ("expiration", ScVal::U64(9_999_999)),
            ("eps", ScVal::U64(1_234_567)),
            (
                "index",
                ScVal::I128(Int128Parts {
                    hi: (index >> 64) as i64,
                    lo: index as u64,
                }),
            ),
            ("last_time", ScVal::U64(1441065600 + 86_400)),
        ]
        .into_iter()
        .map(|(k, v)| soroban_sdk::xdr::ScMapEntry {
            key: ScVal::Symbol(symbol_m(k)),
            val: v,
        })
        .collect::<std::vec::Vec<_>>()
        .try_into()
        .expect("fixed-size emission map"),
    ))
}

/// Injects a fresh `PoolDataKey::EmisData(index)` row whose accrued `index`
/// contradicts the honest snapshot by a fixed delta, into the comparison
/// copy ONLY (emissions-free common snapshots carry no EmisData row, so this
/// control must add one, not seek one).
pub fn mutate_emissions_entry(obs: &mut Observation, pool: &[u8; 32]) {
    let key_val = ScVal::Vec(Some(
        vec![ScVal::Symbol(symbol_m("EmisData")), ScVal::U32(0)]
            .try_into()
            .expect("two-element enum key"),
    ));
    let new_entry = soroban_sdk::xdr::LedgerEntry {
        ext: soroban_sdk::xdr::LedgerEntryExt::V0,
        last_modified_ledger_seq: obs.snap.ledger.sequence_number,
        data: LedgerEntryData::ContractData(soroban_sdk::xdr::ContractDataEntry {
            ext: soroban_sdk::xdr::ExtensionPoint::V0,
            contract: soroban_sdk::xdr::ScAddress::Contract(soroban_sdk::xdr::Hash(*pool)),
            key: key_val.clone(),
            durability: soroban_sdk::xdr::ContractDataDurability::Persistent,
            val: emission_data_map(777_777_777),
        }),
    };
    obs.snap.ledger.ledger_entries.push((
        Box::new(soroban_sdk::xdr::LedgerKey::ContractData(
            soroban_sdk::xdr::LedgerKeyContractData {
                contract: soroban_sdk::xdr::ScAddress::Contract(soroban_sdk::xdr::Hash(*pool)),
                key: key_val,
                durability: soroban_sdk::xdr::ContractDataDurability::Persistent,
            },
        )),
        (Box::new(new_entry), Some(500_000u32)),
    ));
}

// ---------------------------------------------------------------------------
// 6. Token balance bump under the token-owned keyed row
// ---------------------------------------------------------------------------

pub fn mutate_balance(obs: &mut Observation, token: &[u8; 32]) {
    find_entry_mut(obs, |key, entry| {
        let LedgerKey::ContractData(data_key) = key else {
            return false;
        };
        if addr_bytes(&data_key.contract) != Some(*token)
            || data_key.key == ScVal::LedgerKeyContractInstance
        {
            return false;
        }
        let ScVal::Vec(Some(key)) = &data_key.key else {
            return false;
        };
        if !matches!(key.first(), Some(ScVal::Symbol(s)) if s.to_vec() == b"Balance") {
            return false;
        }
        let LedgerEntryData::ContractData(ce) = &mut entry.data else {
            return false;
        };
        let ScVal::Map(Some(map)) = &mut ce.val else {
            panic!("negative control: SAC balance is not a struct");
        };
        let mut fields = map.to_vec();
        let amount = fields
            .iter_mut()
            .find(|f| matches!(&f.key, ScVal::Symbol(s) if s.to_vec() == b"amount"))
            .expect("SAC balance amount");
        let ScVal::I128(parts) = &mut amount.val else {
            panic!("SAC amount is not i128")
        };
        bump_i128(parts, 777);
        *map = fields.try_into().expect("SAC balance map");
        true
    });
}

// ---------------------------------------------------------------------------
// 7. Event payload perturbation
// ---------------------------------------------------------------------------

pub fn mutate_event(obs: &mut Observation, idx: usize) {
    let event = &mut obs.snap.events.0[idx];
    let soroban_sdk::xdr::ContractEventBody::V0(body) = &mut event.event.body;
    // The event payload is an arbitrary ScVal; replace exactly that field.
    body.data = ScVal::Bool(!matches!(body.data, ScVal::Bool(true)));
}

// ---------------------------------------------------------------------------
// 8. Auth history perturbation (host-recorded SorobanAuthorizedInvocation)
// ---------------------------------------------------------------------------

pub fn mutate_auth(obs: &mut Observation) {
    let auth = &mut obs.snap.auth.0;
    // Find the last top-level record carrying a Contract invocation.
    for record in auth.iter_mut().rev() {
        for (addr, inv) in record.iter_mut() {
            if let soroban_sdk::xdr::SorobanAuthorizedFunction::ContractFn(fn_with_args) =
                &mut inv.function
            {
                // StringM's inner field is private with no DerefMut: rebuild
                // the name via the public to_vec/TryFrom surface.
                let mut renamed = fn_with_args.function_name.to_vec();
                renamed.push(b'X');
                fn_with_args.function_name = soroban_sdk::xdr::ScSymbol(
                    soroban_sdk::xdr::StringM::try_from(renamed)
                        .expect("fn name within 32 bytes after suffix"),
                );
                let _ = addr;
                return;
            }
        }
    }
    panic!("negative control: no contract authorization trace present");
}

// ---------------------------------------------------------------------------
// 9. Unrelated contract field bump (skips the four runtime-owned contracts)
// ---------------------------------------------------------------------------

pub fn mutate_unrelated_contract_field(obs: &mut Observation, runtime_owners: &[[u8; 32]]) {
    find_entry_mut(obs, |key, entry| {
        let LedgerKey::ContractData(data_key) = key else {
            return false;
        };
        if data_key.key == ScVal::LedgerKeyContractInstance {
            return false;
        }
        let owner = addr_bytes(&data_key.contract);
        if runtime_owners.iter().any(|id| Some(*id) == owner) {
            return false;
        }
        let LedgerEntryData::ContractData(ce) = &mut entry.data else {
            return false;
        };
        ce.val = ScVal::Bool(!matches!(ce.val, ScVal::Bool(true)));
        true
    });
}

// ---------------------------------------------------------------------------
// Pool/backstop code-row TTL swap adversarial control (dedicated paired
// witness, runs separately from the generic mutation table below).
// ---------------------------------------------------------------------------

/// Create a paired TTL witness in comparison-only copies: real init code TTLs
/// can be equal, which makes a direct exchange a no-op. Never edit the capture.
fn run_code_ttl_swap_control(original: &Observation, side: &SideCapture) {
    println!("Checking negative control: code_ttl_swap");
    let indices = [side.hashes.pool, side.hashes.backstop].map(|hash| {
        original.snap.ledger.ledger_entries.iter().position(|(key, _)| {
            matches!(key.as_ref(), LedgerKey::ContractCode(code) if code.hash.0 == hash)
        }).expect("negative control: runtime code row missing")
    });
    let [pool, backstop] = indices;
    assert_ne!(
        pool, backstop,
        "negative control: runtime code roles overlap"
    );

    let mut reference = original.clone();
    let pool_ttl = reference.snap.ledger.ledger_entries[pool]
        .1
         .1
        .expect("negative control: pool code TTL missing");
    assert!(
        reference.snap.ledger.ledger_entries[backstop]
            .1
             .1
            .is_some(),
        "negative control: backstop code TTL missing",
    );
    let backstop_ttl = pool_ttl
        .checked_add(1)
        .expect("negative control: code TTL overflow");
    reference.snap.ledger.ledger_entries[backstop].1 .1 = Some(backstop_ttl);
    let swapped = {
        let mut swapped = reference.clone();
        swapped.snap.ledger.ledger_entries[pool].1 .1 = Some(backstop_ttl);
        swapped.snap.ledger.ledger_entries[backstop].1 .1 = Some(pool_ttl);
        swapped
    };
    let normalize = |obs: &Observation| {
        super::normalize_executables(
            obs,
            &side.hashes,
            &side.pool_id,
            &side.backstop_id,
            &side.factory_id,
        )
    };
    let mut normalized_reference = normalize(&reference);
    let mut normalized_swapped = normalize(&swapped);
    assert!(
        compare_states(&normalized_reference, &normalized_swapped).is_some(),
        "negative control: pool/backstop code TTL swap was masked"
    );
    assert!(
        compare_states(&normalized_swapped, &normalize(&swapped)).is_none(),
        "negative control: nondeterministic normalized code TTL swap"
    );
    // Equality here proves the witness reaches that exact sorting blind spot.
    for obs in [&mut normalized_reference, &mut normalized_swapped] {
        for (key, (entry, _ttl)) in obs.snap.ledger.ledger_entries.iter_mut() {
            if let LedgerKey::ContractCode(code_key) = key.as_mut() {
                if code_key.hash.0 == super::ZERO_HASH
                    || code_key.hash.0 == super::BACKSTOP_CODE_HASH
                {
                    code_key.hash.0 = super::ZERO_HASH;
                    let LedgerEntryData::ContractCode(code) = &mut entry.data else {
                        panic!("negative control: code payload missing");
                    };
                    code.hash.0 = super::ZERO_HASH;
                }
            }
        }
        obs.snap.ledger.ledger_entries.sort();
    }
    assert!(
        compare_states(&normalized_reference, &normalized_swapped).is_none(),
        "negative control: code TTL witness does not reproduce legacy role collapse"
    );
}

// ---------------------------------------------------------------------------
// Runner — proves the shared comparator detects every typed mutation
// ---------------------------------------------------------------------------

fn determinism_check(original: &Observation, apply: &dyn Fn(&mut Observation)) {
    let mut mutated = original.clone();
    apply(&mut mutated);
    let mut again = original.clone();
    apply(&mut again);
    assert!(
        compare_states(&mutated, &again).is_none(),
        "negative control: nondeterministic mutation"
    );
}

fn ctl_allowance_amount(o: &mut Observation, side: &SideCapture) {
    mutate_allowance_amount(o, &side.blnd_id);
}

fn ctl_allowance_expiration(o: &mut Observation, side: &SideCapture) {
    mutate_allowance_expiration(o, &side.blnd_id);
}

fn ctl_all_ttls(o: &mut Observation, _side: &SideCapture) {
    mutate_all_ttls(o, 17);
}

fn ctl_resdata_ttl(o: &mut Observation, side: &SideCapture) {
    mutate_resdata_ttl(o, &side.pool_id, &side.reserve_asset_id);
}

fn ctl_reserve_accrual(o: &mut Observation, side: &SideCapture) {
    mutate_reserve_accrual(o, &side.pool_id, &side.reserve_asset_id);
}

fn ctl_emissions_entry(o: &mut Observation, side: &SideCapture) {
    mutate_emissions_entry(o, &side.pool_id);
}

fn ctl_token_balance(o: &mut Observation, side: &SideCapture) {
    mutate_balance(o, &side.blnd_id);
}

fn ctl_event_row(o: &mut Observation, _side: &SideCapture) {
    mutate_event(o, 0);
}

fn ctl_auth_trace(o: &mut Observation, _side: &SideCapture) {
    mutate_auth(o);
}

fn ctl_unrelated_field(o: &mut Observation, side: &SideCapture) {
    mutate_unrelated_contract_field(
        o,
        &[
            side.pool_id,
            side.backstop_id,
            side.factory_id,
            side.emitter_id,
            side.blnd_id,
        ],
    );
}

const CONTROLS: [(&str, fn(&mut Observation, &SideCapture)); 10] = [
    ("allowance_amount", ctl_allowance_amount),
    ("allowance_expiration", ctl_allowance_expiration),
    ("all_ttls", ctl_all_ttls),
    ("resdata_ttl", ctl_resdata_ttl),
    ("reserve_accrual", ctl_reserve_accrual),
    ("emissions_injection", ctl_emissions_entry),
    ("token_balance", ctl_token_balance),
    ("event_row", ctl_event_row),
    ("auth_trace", ctl_auth_trace),
    ("unrelated_field", ctl_unrelated_field),
];

/// Runs the full negative-control battery over ONE side's captured
/// observation. Every control must be DETECTED by the comparator; every
/// determinism re-application must compare equal.

pub fn run_negative_controls(base_side: &SideCapture) {
    assert!(
        !base_side.capture.steps.is_empty(),
        "negative control precondition failed: empty replay capture"
    );

    // Highest-fidelity source: the init snapshot (full post-setup state).
    let original = base_side.capture.initial_obs().clone();
    let normalize = |obs: &Observation| {
        super::normalize_executables(
            obs,
            &base_side.hashes,
            &base_side.pool_id,
            &base_side.backstop_id,
            &base_side.factory_id,
        )
    };
    let normalized_original = normalize(&original);

    for (name, apply) in CONTROLS.iter() {
        println!("Checking negative control: {name}");
        determinism_check(&original, &|o: &mut Observation| apply(o, base_side));
        let mut mutated = original.clone();
        apply(&mut mutated, base_side);
        assert!(
            compare_states(&normalized_original, &normalize(&mutated)).is_some(),
            "negative control {}: comparator FAILED to detect mutation",
            name
        );
    }

    run_code_ttl_swap_control(&original, base_side);
    println!(
        "adr8 differential: {} negative controls all detected",
        CONTROLS.len() + 1
    );
}
