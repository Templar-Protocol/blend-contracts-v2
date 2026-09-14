//! Causal stock-positive Wasm controls for ADR0008/ADR0011.
//!
//! No runtime snapshot normalization: the prepared snapshot carries real
//! base code bytes; only the named backstop ContractInstance executable is
//! swapped base->fork (with a reverse-recovery proof), and both rehydrated Envs
//! replay real scenarios through fresh generated clients. Stock success must
//! produce distinctive effects; fork rejection must be exactly `1000` with a
//! byte-identical ledger/TTL and no auth/event evidence.
//!
//! Entry point wired by the main runner: `run_stock_controls`.
use std::path::{Path, PathBuf};

use crate::{
    create_fixture_with_wasm,
    test_fixture::{TokenIndex, SCALAR_12, SCALAR_7},
};
use backstop::BackstopClient;
use pool::{FlashLoan, PoolClient, Request, RequestType};
use soroban_sdk::{
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    vec as svec,
    xdr::{
        ContractEventType, ContractExecutable, Hash, LedgerEntryData, LedgerKey, ScAddress,
        ScContractInstance, ScVal, ToXdr,
    },
    Address, Env, Error, IntoVal,
};

/// SDK 22.0.7 has no public `Address::contract_id`; go through the verified
/// `From<&Address> for ScAddress` conversion and read the contract hash.
fn target_bytes(addr: &Address) -> [u8; 32] {
    match soroban_sdk::xdr::ScAddress::from(addr) {
        soroban_sdk::xdr::ScAddress::Contract(h) => h.0,
        other => panic!(
            "stock control target is not a contract address: {:?}",
            other
        ),
    }
}

/// Out-of-crate entry point wired by the main runner:
/// executes all thirteen controls against supplied artifacts.
pub fn run_stock_controls(
    base_pool: &[u8],
    base_backstop: &[u8],
    fork_pool: &[u8],
    fork_backstop: &[u8],
) {
    let root = PathBuf::from(
        std::env::var_os("ADR8_DIFF_OUTPUT_DIR")
            .expect("ADR8_DIFF_OUTPUT_DIR must name retained evidence directory"),
    );
    assert!(!root.as_os_str().is_empty());
    std::fs::create_dir_all(&root).unwrap();
    // ---- Six ADR0011 backstop traps ----
    add_reward_control(&root, base_pool, base_backstop, fork_backstop);
    distribute_control(&root, base_pool, base_backstop, fork_backstop);
    gulp_emissions_control(&root, base_pool, base_backstop, fork_backstop);
    remove_reward_control(&root, base_pool, base_backstop, fork_backstop);
    claim_control(&root, base_pool, base_backstop, fork_backstop);
    drop_control(&root, base_pool, base_backstop, fork_backstop);
    // ---- Seven closed ADR0008 pool restriction classes ----
    class1_freeze_control(&root, base_pool, base_backstop, fork_pool);
    class2_zero_take_control(&root, base_pool, base_backstop, fork_pool);
    class3_sealed_config_control(&root, base_pool, base_backstop, fork_pool);
    class4_surplus_auction_control(&root, base_pool, base_backstop, fork_pool);
    class5_price_control(&root, base_pool, base_backstop, fork_pool);
    class6_flash_loan_control(&root, base_pool, base_backstop, fork_pool);
    class7_default_control(&root, base_pool, base_backstop, fork_pool);
}

// ===========================================================================
// Shared helpers
// ===========================================================================

type Snapshot = soroban_sdk::testutils::Snapshot;

/// Convert the SDK Address into its own environment for cross-Env use.
fn sc_address(e: &Env, addr: &Address) -> Address {
    use soroban_sdk::TryFromVal;
    Address::try_from_val(e, &<soroban_sdk::xdr::ScAddress>::from(addr)).unwrap()
}

fn save(root: &Path, case: &str, stage: &str, snap: &Snapshot) {
    let path = root.join(case).join(format!("{stage}.json"));
    snap.write_file(&path)
        .expect("complete SDK Snapshot retained");
    let roundtrip = Snapshot::read_file(&path).unwrap();
    assert_eq!(
        &roundtrip, snap,
        "snapshot custody round-trip changed bytes"
    );
}

fn stable_hash(env: &Env, wasm: &[u8]) -> Hash {
    Hash(
        env.crypto()
            .sha256(&soroban_sdk::Bytes::from_slice(env, wasm))
            .to_array(),
    )
}

const INSTANCE_KEY: ScVal = ScVal::LedgerKeyContractInstance;

/// Change exactly one named instance from the expected executable to the new
/// executable, leaving its storage, TTL, all code entries and other state intact.
fn replace_executable(
    snap: &mut Snapshot,
    target_bytes: &[u8; 32],
    expect_old: &Hash,
    expect_new: &Hash,
) {
    let mut matches = 0usize;
    let target = ScAddress::Contract(Hash(*target_bytes));
    for (key, (entry, _ttl)) in &mut snap.ledger.ledger_entries {
        if let LedgerKey::ContractData(data_key) = key.as_ref() {
            if data_key.contract != target || data_key.key != INSTANCE_KEY {
                continue;
            }
            matches += 1;
            let LedgerEntryData::ContractData(entry_data) = &mut entry.data else {
                panic!("instance key does not carry contract data");
            };
            assert_eq!(entry_data.contract, target);
            assert_eq!(entry_data.key, INSTANCE_KEY);
            let ScVal::ContractInstance(ScContractInstance { executable, .. }) =
                &mut entry_data.val
            else {
                panic!("unsupported instance schema");
            };
            assert_eq!(*executable, ContractExecutable::Wasm(expect_old.clone()));
            *executable = ContractExecutable::Wasm(expect_new.clone());
        }
    }
    assert_eq!(
        matches, 1,
        "named target instance must be found exactly once"
    );
}

fn snapshot_split(
    root: &Path,
    case: &str,
    fixture_env: &Env,
    target: &Address,
    target_base_wasm: &[u8],
    target_fork_wasm: &[u8],
) -> (Env, Env) {
    let new_hash_bytes: [u8; 32] = fixture_env
        .deployer()
        .upload_contract_wasm(target_fork_wasm)
        .to_array();
    // Full raw snapshot from the preparation environment (real base code bytes).
    let prepared = fixture_env.to_snapshot();
    save(root, case, "stock-prepared", &prepared);

    // Swap only the target instance executable to fork; prove reverse swaps back.
    let named_bytes = target_bytes(target);
    let old_hash = stable_hash(fixture_env, target_base_wasm);
    let new_hash = Hash(new_hash_bytes);
    assert_ne!(old_hash, new_hash);
    let mut substituted = prepared.clone();
    replace_executable(&mut substituted, &named_bytes, &old_hash, &new_hash);
    save(root, case, "fork-substituted", &substituted);

    let mut reversed = substituted.clone();
    replace_executable(&mut reversed, &named_bytes, &new_hash, &old_hash);
    assert_eq!(&reversed, &prepared, "reverse swap must recover original");
    save(root, case, "reverse-recovered", &reversed);

    let stock = Env::from_snapshot(prepared);
    let fork = Env::from_snapshot(substituted);
    stock.mock_all_auths();
    fork.mock_all_auths();
    stock.cost_estimate().budget().reset_unlimited();
    fork.cost_estimate().budget().reset_unlimited();
    save(root, case, "stock-before", &stock.to_snapshot());
    save(root, case, "fork-before", &fork.to_snapshot());
    (stock, fork)
}

// ===========================================================================
// Fork rejection invariants (ADR0011)
// ===========================================================================

/// Generated SDK clients expose invocation errors separately from return conversion.
fn expect_trap(error: Option<Result<Error, soroban_sdk::InvokeError>>) {
    assert_eq!(error, Some(Ok(Error::from_contract_error(1000))));
}

/// Call immediately after the operation, before any getter can replace auth
/// evidence. Snapshot retains the entire raw event envelope, including failures.
fn capture_call(
    root: &Path,
    case: &str,
    stage: &str,
    env: &Env,
    result: &impl std::fmt::Debug,
) -> Snapshot {
    let snapshot = env.to_snapshot();
    let events = env.events().all();
    let auth = env.auths();
    save(root, case, stage, &snapshot);
    std::fs::write(
        root.join(case).join(format!("{stage}-result.txt")),
        format!("result={result:?}\ncontract_events={events:?}\nauth={auth:?}\n"),
    )
    .unwrap();
    snapshot
}

fn check_unaffected(case: &str, stage: &str, pre: &Snapshot, post: &Snapshot) {
    assert_eq!(
        pre.ledger, post.ledger,
        "{case}/{stage}: fork trap must leave complete ledger/TTL unchanged"
    );
}

// ===========================================================================
// Six ADR0011 backstop traps, each with a real prepared stock success
// ===========================================================================

/// Upstream witness: test_backstop_rz_changes adds the seeded 50k-LP pool.
fn add_reward_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork: &[u8]) {
    let case = "backstop_add_reward";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    assert!(f.backstop.reward_zone().is_empty());
    let pool_addr = f.pools[0].pool.address.clone();
    let (stock, fork_env) =
        snapshot_split(root, case, &f.env, &f.backstop.address, base_backstop, fork);
    let sc = BackstopClient::new(&stock, &sc_address(&stock, &f.backstop.address));
    let kc = BackstopClient::new(&fork_env, &sc_address(&fork_env, &f.backstop.address));
    let pre = evidence(root, case, "fork-before", &fork_env);
    sc.add_reward(&sc_address(&stock, &pool_addr), &None);
    capture_call(root, case, "stock-add-reward", &stock, &());
    assert_eq!(
        sc.reward_zone(),
        svec![&stock, sc_address(&stock, &pool_addr)],
        "stock add_reward must enter pool into reward zone"
    );
    expect_trap(
        kc.try_add_reward(&sc_address(&fork_env, &pool_addr), &None)
            .err(),
    );
    reject_silent(
        case,
        "add",
        &pre,
        &evidence(root, case, "fork-after", &fork_env),
    );
}

/// Stock initializes its cursor, then accrues exactly the following interval.
fn distribute_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork: &[u8]) {
    let case = "backstop_distribute";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    // Prepare genuine stock emission state via upstream seed sequence.
    f.backstop.add_reward(&f.pools[0].pool.address, &None);
    f.emitter.distribute();
    assert_eq!(f.backstop.distribute(), 0);
    let elapsed = 7 * 24 * 60 * 60;
    f.jump(elapsed);
    f.emitter.distribute();
    let pool_addr = f.pools[0].pool.address.clone();
    let (stock, fork_env) =
        snapshot_split(root, case, &f.env, &f.backstop.address, base_backstop, fork);
    let sc = BackstopClient::new(&stock, &sc_address(&stock, &f.backstop.address));
    let kc = BackstopClient::new(&fork_env, &sc_address(&fork_env, &f.backstop.address));
    let bs = sc_address(&stock, &f.backstop.address);
    let sp = sc_address(&stock, &pool_addr);
    let blnd = soroban_sdk::token::Client::new(
        &stock,
        &sc_address(&stock, &f.tokens[TokenIndex::BLND].address),
    );
    let pre = evidence(root, case, "fork-before", &fork_env);
    let balance_before = blnd.balance(&bs);
    // Stock distribute performs NO transfer: it mints through the emitter and
    // accrues the full allocation onto this single-pool reward zone. Returned
    // elapsed*SCALAR_7 is therefore a pure accounting delta; minted supply
    // rose in the PRE-SPLIT emitter call for the same window.
    let accrued = sc.distribute();
    capture_call(root, case, "stock-distribute", &stock, &accrued);
    assert_eq!(
        accrued,
        i128::from(elapsed) * SCALAR_7,
        "single-RZ pool receives elapsed seconds of emitter emissions exactly"
    );
    assert_eq!(
        blnd.balance(&bs),
        balance_before,
        "distribute transfers nothing"
    );
    assert_eq!(blnd.allowance(&bs, &sp), 0, "no allowance before gulp");
    assert_eq!(
        sc.reward_zone(),
        svec![&stock, sc_address(&stock, &pool_addr)]
    );
    expect_trap(kc.try_distribute().err());
    reject_silent(
        case,
        "distribute",
        &pre,
        &evidence(root, case, "fork-after", &fork_env),
    );
}

/// Upstream witness: after distribute, a pool in the reward zone gulps its
/// real share as an BLND allowance; a second call inside the same epoch
/// returns zero. Fork rejects before creating any pool claim.
fn gulp_emissions_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork: &[u8]) {
    let case = "backstop_gulp_emissions";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    f.backstop.add_reward(&f.pools[0].pool.address, &None);
    f.emitter.distribute();
    f.backstop.distribute();
    let elapsed = 7 * 24 * 60 * 60;
    f.jump(elapsed);
    f.emitter.distribute();
    f.backstop.distribute();
    let pool_addr = f.pools[0].pool.address.clone();
    let (stock, fork_env) =
        snapshot_split(root, case, &f.env, &f.backstop.address, base_backstop, fork);
    let sc = BackstopClient::new(&stock, &sc_address(&stock, &f.backstop.address));
    let kc = BackstopClient::new(&fork_env, &sc_address(&fork_env, &f.backstop.address));
    let bs = sc_address(&stock, &f.backstop.address);
    let sp = sc_address(&stock, &pool_addr);
    let blnd = soroban_sdk::token::Client::new(
        &stock,
        &sc_address(&stock, &f.tokens[TokenIndex::BLND].address),
    );
    assert_eq!(blnd.allowance(&bs, &sp), 0);
    let pre = evidence(root, case, "fork-before", &fork_env);
    // Backstop accrued the full elapsed allocation in prep; gulp moves 30%
    // to a pool allowance now and configures 70% for user emissions (eps).
    let amount = sc.gulp_emissions(&sp);
    capture_call(root, case, "stock-gulp", &stock, &amount);
    assert_eq!(
        blnd.allowance(&bs, &sp),
        amount,
        "gulp must grant exactly its returned allocation"
    );
    assert_eq!(
        amount,
        i128::from(elapsed) * SCALAR_7 * 3 / 10,
        "gulp is exactly 30 percent of the single-pool accrued stock emissions"
    );
    // Stock enforces a one-day interval between gulps, even with zero accrued.
    let before_repeat = stock.to_ledger_snapshot();
    expect_trap(sc.try_gulp_emissions(&sp).err());
    assert_eq!(stock.to_ledger_snapshot(), before_repeat);
    expect_trap(
        kc.try_gulp_emissions(&sc_address(&fork_env, &pool_addr))
            .err(),
    );
    reject_silent(
        case,
        "gulp_emissions",
        &pre,
        &evidence(root, case, "fork-after", &fork_env),
    );
}

/// Upstream witness: test_backstop_rz_changes removes pool below threshold
/// and empties rz after real queue/withdraw threshold deterioration.
fn remove_reward_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork: &[u8]) {
    let case = "backstop_remove_reward";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    f.backstop.add_reward(&f.pools[0].pool.address, &None);
    f.backstop
        .queue_withdrawal(&f.users[0], &f.pools[0].pool.address, &(45000 * SCALAR_7));
    f.jump(21 * 24 * 60 * 60);
    f.emitter.distribute();
    f.backstop.distribute();
    f.backstop
        .withdraw(&f.users[0], &f.pools[0].pool.address, &(45000 * SCALAR_7));
    assert_eq!(f.backstop.reward_zone().len(), 1);
    let pool_addr = f.pools[0].pool.address.clone();
    let (stock, fork_env) =
        snapshot_split(root, case, &f.env, &f.backstop.address, base_backstop, fork);
    let sc = BackstopClient::new(&stock, &sc_address(&stock, &f.backstop.address));
    let kc = BackstopClient::new(&fork_env, &sc_address(&fork_env, &f.backstop.address));
    let pre = evidence(root, case, "fork-before", &fork_env);
    sc.remove_reward(&sc_address(&stock, &pool_addr));
    capture_call(root, case, "stock-remove-reward", &stock, &());
    assert!(
        sc.reward_zone().is_empty(),
        "reward zone emptied by real threshold deterioration"
    );
    expect_trap(
        kc.try_remove_reward(&sc_address(&fork_env, &pool_addr))
            .err(),
    );
    reject_silent(
        case,
        "remove_reward",
        &pre,
        &evidence(root, case, "fork-after", &fork_env),
    );
}

/// Upstream witness: test_claim uses seeded emissions + queued user data to
/// mint LP into the backstop's comet-held pool (dep_tokn_amt_in semantics).
fn claim_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork: &[u8]) {
    let case = "backstop_claim";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    f.backstop.add_reward(&f.pools[0].pool.address, &None);
    f.emitter.distribute();
    f.backstop.distribute();
    f.jump(7 * 24 * 60 * 60);
    f.emitter.distribute();
    f.backstop.distribute();
    f.backstop.gulp_emissions(&f.pools[0].pool.address);
    f.jump(3 * 24 * 60 * 60);
    let user = f.users[0].clone();
    let pool_addr = f.pools[0].pool.address.clone();
    let (stock, fork_env) =
        snapshot_split(root, case, &f.env, &f.backstop.address, base_backstop, fork);
    let sc = BackstopClient::new(&stock, &sc_address(&stock, &f.backstop.address));
    let kc = BackstopClient::new(&fork_env, &sc_address(&fork_env, &f.backstop.address));
    // Comet-backed claim: the user's accrued BLND is pulled from the backstop
    // by the comet contract itself, minted LP tokens land on the backstop and
    // are re-deposited as pool shares for the user. Return = lp_tokens_out.
    let lp = soroban_sdk::token::Client::new(&stock, &sc_address(&stock, &f.lp.address));
    let lps_before_user = lp.balance(&sc_address(&stock, &user));
    let pre = evidence(root, case, "fork-before", &fork_env);
    let shares_before = sc
        .user_balance(&sc_address(&stock, &pool_addr), &sc_address(&stock, &user))
        .shares;
    let minted = sc.claim(
        &sc_address(&stock, &user),
        &svec![&stock, sc_address(&stock, &pool_addr)],
        &0,
    );
    capture_call(root, case, "stock-claim", &stock, &minted);
    assert!(minted > 0, "claimed emissions must mint real LP tokens");
    let shares_after = sc
        .user_balance(&sc_address(&stock, &pool_addr), &sc_address(&stock, &user))
        .shares;
    assert!(
        shares_after > shares_before,
        "claim must convert emitted BLND into deposited pool shares"
    );
    assert_eq!(
        lps_before_user,
        lp.balance(&sc_address(&stock, &user)),
        "LP tokens are held by the backstop, not the claiming user"
    );
    expect_trap(
        kc.try_claim(
            &sc_address(&fork_env, &user),
            &svec![&fork_env, sc_address(&fork_env, &pool_addr)],
            &0,
        )
        .err(),
    );
    reject_silent(
        case,
        "claim",
        &pre,
        &evidence(root, case, "fork-after", &fork_env),
    );
}

/// Upstream witness: test_backstop_emitters_trapped_stock_remains_abroad shows
/// emitter retained BLND for drop; backstop forwards its drop_list through the
/// emitter, whose SAC mints the listed allocations. Post-drop BLND balance of
/// each drop-list recipient rises by exactly the drop allocation.
fn drop_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork: &[u8]) {
    let case = "backstop_drop";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    // The fixture's constructor-equivalent wiring configured the real emitter
    // (SAC token, start/duration stamps, drop_list) BEFORE any distribute, so
    // the backstop's own require_authed emitter call path is live stock state.
    let (stock, fork_env) =
        snapshot_split(root, case, &f.env, &f.backstop.address, base_backstop, fork);
    let sc = BackstopClient::new(&stock, &sc_address(&stock, &f.backstop.address));
    let kc = BackstopClient::new(&fork_env, &sc_address(&fork_env, &f.backstop.address));
    let blnd = soroban_sdk::token::Client::new(
        &stock,
        &sc_address(&stock, &f.tokens[TokenIndex::BLND].address),
    );
    let bombadil = sc_address(&stock, &f.bombadil);
    let frodo = sc_address(&stock, &f.users[0]);
    let before_bombadil = blnd.balance(&bombadil);
    let before_frodo = blnd.balance(&frodo);
    let pre = evidence(root, case, "fork-before", &fork_env);
    sc.drop();
    capture_call(root, case, "stock-drop", &stock, &());
    expect_trap(kc.try_drop().err());
    reject_silent(
        case,
        "drop",
        &pre,
        &evidence(root, case, "fork-after", &fork_env),
    );
    assert_eq!(
        blnd.balance(&bombadil),
        before_bombadil + 10_000_000 * SCALAR_7
    );
    assert_eq!(blnd.balance(&frodo), before_frodo + 30_000_000 * SCALAR_7);
}

/// Capture pre/post evidence: full snapshot plus the raw event/auth envelope
fn evidence(root: &Path, case: &str, stage: &str, env: &Env) -> Snapshot {
    let snapshot = env.to_snapshot();
    let events = snapshot.events.clone();
    let auth = env.auths();
    save(root, case, stage, &snapshot);
    std::fs::write(
        root.join(case).join(format!("{stage}-evidence.txt")),
        format!("contract_events={events:?}\nauth={auth:?}\n"),
    )
    .unwrap();
    snapshot
}

/// Six-control contract: fork trap leaves ledger+TTLs identical, authorizes
/// nothing, and emits no new committed (failed_call=false, type Contract)
/// event. Snapshot envelope keeps the failed diagnostics for evidence.
fn reject_silent(case: &str, stage: &str, pre: &Snapshot, post: &Snapshot) {
    check_unaffected(case, stage, pre, post);
    assert!(
        post.auth.0[pre.auth.0.len()..]
            .iter()
            .all(std::vec::Vec::is_empty),
        "{case}/{stage}: rejected call must not authorize anything"
    );
    assert!(
        post.events.0[pre.events.0.len()..]
            .iter()
            .all(|event| event.failed_call || event.event.type_ != ContractEventType::Contract),
        "{case}/{stage}: rejected call must not commit a contract event"
    );
}

// ===========================================================================
// Seven closed ADR0008 pool restriction classes
// ===========================================================================

fn expect_code(actual: Option<Result<Error, soroban_sdk::InvokeError>>, expected: u32) {
    assert_eq!(actual, Some(Ok(Error::from_contract_error(expected))));
}
/// Class 1 - independent absorbing freeze.
/// ADR: authenticated status-4 request is handled before any backstop call,
/// succeeds idempotently, and every leave attempt rejects before effects.
fn class1_freeze_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork_pool: &[u8]) {
    let case = "pool_freeze_absorbing";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    f.pools[0].pool.set_status(&4);
    assert_eq!(f.pools[0].pool.get_config().status, 4);
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    for (env, id) in [(&stock, &stock_id), (&fork_env, &fork_id)] {
        let admin = sc_address(env, &f.bombadil);
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: id,
                fn_name: "set_status",
                args: (0u32,).into_val(env),
                sub_invokes: &[],
            },
        }]);
    }
    let pre = fork_env.to_snapshot();
    save(root, case, "fork-before", &pre);
    expect_code(kc.try_set_status(&0).err(), 1204);
    let post = fork_env.to_snapshot();
    check_unaffected(case, "leave", &pre, &post);
    save(root, case, "fork-after", &post);
    sc.set_status(&0);
    assert_eq!(sc.get_config().status, 0, "stock can leave admin freeze");
    let fork_admin = sc_address(&fork_env, &f.bombadil);
    fork_env.mock_auths(&[MockAuth {
        address: &fork_admin,
        invoke: &MockAuthInvoke {
            contract: &fork_id,
            fn_name: "set_status",
            args: (4u32,).into_val(&fork_env),
            sub_invokes: &[],
        },
    }]);
    save(root, case, "stock-after", &stock.to_snapshot());
    kc.set_status(&4);
    assert_eq!(kc.get_config().status, 4, "fork freeze is idempotent");
}

/// Class 2 - zero take. Initialization accepted only zero; update_pool always
/// rejects in the fork while stock performs the authenticated update.
fn class2_zero_take_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork_pool: &[u8]) {
    let case = "pool_zero_take";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    for (env, id) in [(&stock, &stock_id), (&fork_env, &fork_id)] {
        let admin = sc_address(env, &f.bombadil);
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: id,
                fn_name: "update_pool",
                args: (50u32, 20u32, 200_0000000i128).into_val(env),
                sub_invokes: &[],
            },
        }]);
    }
    let pre = fork_env.to_snapshot();
    save(root, case, "fork-before", &pre);
    expect_code(kc.try_update_pool(&50, &20, &200_0000000).err(), 1200);
    let post = fork_env.to_snapshot();
    check_unaffected(case, "update_pool", &pre, &post);
    save(root, case, "fork-after", &post);
    sc.update_pool(&50, &20, &200_0000000);
    save(root, case, "stock-after", &stock.to_snapshot());
    let config = sc.get_config();
    assert_eq!(config.bstop_rate, 50);
    assert_eq!(config.max_positions, 20);
    assert_eq!(config.min_collateral, 200_0000000);
}

/// Class 3 - sealed configuration. After activation only the exact
/// enabled->false transition of an existing reserve is permitted; every other
/// queue/execute rejects in the fork while the stock accepts.
fn class3_sealed_config_control(
    root: &Path,
    base_pool: &[u8],
    base_backstop: &[u8],
    fork_pool: &[u8],
) {
    let case = "pool_sealed_config";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    let asset = &f.tokens[TokenIndex::XLM].address;
    let mut tweaked = f.read_reserve_config(0, TokenIndex::XLM);
    tweaked.c_factor -= 1;
    f.pools[0].pool.queue_set_reserve(asset, &tweaked);
    f.jump(7 * 24 * 60 * 60);
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    stock.mock_auths(&[]);
    let fork_admin = sc_address(&fork_env, &f.bombadil);
    fork_env.mock_auths(&[MockAuth {
        address: &fork_admin,
        invoke: &MockAuthInvoke {
            contract: &fork_id,
            fn_name: "queue_set_reserve",
            args: (sc_address(&fork_env, asset), tweaked.clone()).into_val(&fork_env),
            sub_invokes: &[],
        },
    }]);
    let pre = fork_env.to_snapshot();
    save(root, case, "fork-before", &pre);
    expect_code(
        kc.try_set_reserve(&sc_address(&fork_env, asset)).err(),
        1202,
    );
    let post = fork_env.to_snapshot();
    check_unaffected(case, "execute_queued_tweak", &pre, &post);
    save(root, case, "fork-after", &post);
    sc.set_reserve(&sc_address(&stock, asset));
    save(root, case, "stock-after", &stock.to_snapshot());
    assert_eq!(
        sc.get_reserve(&sc_address(&stock, asset)).config.c_factor,
        tweaked.c_factor
    );
    // Each enforced replay consumes its listed credential once; re-arm per op.
    fork_env.mock_auths(&[MockAuth {
        address: &fork_admin,
        invoke: &MockAuthInvoke {
            contract: &fork_id,
            fn_name: "queue_set_reserve",
            args: (sc_address(&fork_env, asset), tweaked.clone()).into_val(&fork_env),
            sub_invokes: &[],
        },
    }]);
    // Rolled-back seal leaves the queue row behind: re-queuing rejects on
    // has_queued_reserve_set (#1200) before the sealed-transition gate.
    expect_code(
        kc.try_queue_set_reserve(&sc_address(&fork_env, asset), &tweaked)
            .err(),
        1200,
    );
    check_unaffected(case, "queue_tweak", &pre, &fork_env.to_snapshot());
    save(root, case, "fork-after-queue", &fork_env.to_snapshot());
}

/// Class 4 - no surplus auction path. Stock's gulp recognizes raw token surplus
/// held by the pool and handles it natively (backstop credit); the fork rejects
/// every surplus-creating gulp outright and also rejects creating or filling a
/// surplus-family auction. Preparation mints real underlying tokens to the
/// pool address via the ordinary stock token contract only.
fn class4_surplus_auction_control(
    root: &Path,
    base_pool: &[u8],
    base_backstop: &[u8],
    fork_pool: &[u8],
) {
    let case = "pool_surplus_path";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    let asset = &f.tokens[TokenIndex::XLM].address;
    let surplus = 5_000 * SCALAR_7;
    f.tokens[TokenIndex::XLM].mint(&f.pools[0].pool.address, &surplus);
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    stock.mock_auths(&[]);
    fork_env.mock_auths(&[]);
    let pre = fork_env.to_snapshot();
    save(root, case, "fork-before", &pre);
    expect_code(kc.try_gulp(&sc_address(&fork_env, asset)).err(), 1200);
    let post = fork_env.to_snapshot();
    check_unaffected(case, "gulp_surplus", &pre, &post);
    save(root, case, "fork-after", &post);
    let credit_before = sc
        .get_reserve(&sc_address(&stock, asset))
        .data
        .backstop_credit;
    assert_eq!(sc.gulp(&sc_address(&stock, asset)), surplus);
    save(root, case, "stock-after", &stock.to_snapshot());
    assert_eq!(
        sc.get_reserve(&sc_address(&stock, asset))
            .data
            .backstop_credit,
        credit_before + surplus
    );
    expect_code(
        kc.try_new_auction(
            &2,
            &sc_address(&fork_env, &f.backstop.address),
            &svec![&fork_env, sc_address(&fork_env, &f.lp.address)],
            &svec![&fork_env, sc_address(&fork_env, asset)],
            &100,
        )
        .err(),
        1200,
    );
    check_unaffected(case, "interest_creation", &pre, &fork_env.to_snapshot());
    save(root, case, "fork-after-interest", &fork_env.to_snapshot());
}

/// Class 5: stock supports scaled non-seven-decimal prices; the fork rejects
/// that oracle contract. The stable mock stamps current time, so this is not
/// falsely presented as a historical stale-price witness.
fn class5_price_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork_pool: &[u8]) {
    let case = "pool_strict_price_decimals";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    use sep_40_oracle::testutils::Asset;
    f.oracle.set_data(
        &f.bombadil,
        &Asset::Other(soroban_sdk::Symbol::new(&f.env, "USD")),
        &svec![
            &f.env,
            Asset::Stellar(f.tokens[TokenIndex::WETH].address.clone()),
            Asset::Stellar(f.tokens[TokenIndex::USDC].address.clone()),
            Asset::Stellar(f.tokens[TokenIndex::XLM].address.clone()),
            Asset::Stellar(f.tokens[TokenIndex::STABLE].address.clone()),
        ],
        &8,
        &300,
    );
    f.oracle.set_price_stable(&svec![
        &f.env,
        20_000_0000000,
        10_0000000,
        1_0000000,
        10_0000000
    ]);
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    let requests = |env: &Env| {
        svec![
            env,
            Request {
                request_type: RequestType::Borrow as u32,
                address: sc_address(env, &f.tokens[TokenIndex::XLM].address),
                amount: SCALAR_7,
            }
        ]
    };
    let fork_user = sc_address(&fork_env, &f.users[0]);
    for (env, id) in [(&stock, &stock_id), (&fork_env, &fork_id)] {
        let user = sc_address(env, &f.users[0]);
        // Borrow transfers out of the pool; only the user's submit requires a credential.
        env.mock_auths(&[MockAuth {
            address: &user,
            invoke: &MockAuthInvoke {
                contract: id,
                fn_name: "submit",
                args: (&user, &user, &user, requests(env)).into_val(env),
                sub_invokes: &[],
            },
        }]);
    }
    let pre = fork_env.to_snapshot();
    save(root, case, "fork-before", &pre);
    expect_code(
        kc.try_submit(&fork_user, &fork_user, &fork_user, &requests(&fork_env))
            .err(),
        1210,
    );
    let post = fork_env.to_snapshot();
    check_unaffected(case, "non_seven_decimal_oracle", &pre, &post);
    save(root, case, "fork-after", &post);
    let user = sc_address(&stock, &f.users[0]);
    let result = sc.submit(&user, &user, &user, &requests(&stock));
    save(root, case, "stock-after", &stock.to_snapshot());
    assert!(
        result
            .liabilities
            .get(f.pools[0].reserves[&TokenIndex::XLM])
            .unwrap()
            > 0
    );
}

/// Class 6: an ordinary stock flash loan executes its real native callback and
/// repays. The fork rejects the identical small request unconditionally.
fn class6_flash_loan_control(
    root: &Path,
    base_pool: &[u8],
    base_backstop: &[u8],
    fork_pool: &[u8],
) {
    let case = "pool_flash_loan";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    let user = Address::generate(&f.env);
    let asset = &f.tokens[TokenIndex::XLM].address;
    f.tokens[TokenIndex::XLM].mint(&user, &100);
    // Stock flash loans repay through transfer_from, not a user-authorized transfer.
    f.tokens[TokenIndex::XLM].approve(
        &user,
        &f.pools[0].pool.address,
        &(1000 * SCALAR_7 + 100),
        &(f.env.ledger().sequence() + 17280),
    );
    let receiver = crate::moderc3156::create_flashloan_receiver(&f.env);
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    // Env snapshots do not retain the native callback registry. Re-register at
    // the identical address and require the full ledger/TTL state to survive.
    for env in [&stock, &fork_env] {
        let before = env.to_ledger_snapshot();
        let id = sc_address(env, &receiver.0);
        env.register_at(
            &id,
            moderc3156_example::FlashLoanReceiverModifiedERC3156 {},
            (),
        );
        assert_eq!(
            env.to_ledger_snapshot(),
            before,
            "callback registration altered prepared state"
        );
    }
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    let loan = |env: &Env| FlashLoan {
        contract: sc_address(env, &receiver.0),
        asset: sc_address(env, asset),
        amount: 1000 * SCALAR_7,
    };
    let requests = |env: &Env| {
        svec![
            env,
            Request {
                request_type: RequestType::Repay as u32,
                address: sc_address(env, asset),
                amount: 1000 * SCALAR_7 + 100,
            }
        ]
    };
    for (env, id) in [(&stock, &stock_id), (&fork_env, &fork_id)] {
        let caller = sc_address(env, &user);
        env.mock_auths(&[MockAuth {
            address: &caller,
            invoke: &MockAuthInvoke {
                contract: id,
                fn_name: "flash_loan",
                args: (&caller, loan(env), requests(env)).into_val(env),
                sub_invokes: &[MockAuthInvoke {
                    contract: &sc_address(env, &receiver.0),
                    fn_name: "exec_op",
                    args: (&caller, sc_address(env, asset), 1000 * SCALAR_7, 0i128).into_val(env),
                    sub_invokes: &[],
                }],
            },
        }]);
    }
    let pre = fork_env.to_snapshot();
    save(root, case, "fork-before", &pre);
    expect_code(
        kc.try_flash_loan(
            &sc_address(&fork_env, &user),
            &loan(&fork_env),
            &requests(&fork_env),
        )
        .err(),
        1200,
    );
    let post = fork_env.to_snapshot();
    check_unaffected(case, "flash_loan", &pre, &post);
    save(root, case, "fork-after", &post);
    let result = sc.flash_loan(&sc_address(&stock, &user), &loan(&stock), &requests(&stock));
    save(root, case, "stock-after", &stock.to_snapshot());
    assert!(
        result.liabilities.is_empty(),
        "stock flash loan fully repaid"
    );
    use soroban_sdk::testutils::Events as _;
    assert!(
        stock.events().all().iter().any(|(id, topics, _)| {
            id == stock_id
                && topics.get(0).map(|v| v.to_xdr(&stock))
                    == Some(soroban_sdk::Symbol::new(&stock, "flash_loan").to_xdr(&stock))
        }),
        "stock callback/flash-loan event missing"
    );
}

/// Class 7 uses a disclosed host-prepared residual-debt position, not a claimed
/// historical transaction: moving collateral to ordinary supply preserves all
/// token and bToken totals while satisfying stock's no-collateral precondition.
/// Identical calls then distinguish backstop debt assignment from supplier loss.
fn class7_default_control(root: &Path, base_pool: &[u8], base_backstop: &[u8], fork_pool: &[u8]) {
    let case = "pool_direct_supplier_default";
    let f = create_fixture_with_wasm(base_pool, base_backstop);
    let user = &f.users[0];
    save(
        root,
        case,
        "stock-before-host-preparation",
        &f.env.to_snapshot(),
    );
    let mut positions = f.pools[0].pool.get_positions(user);
    let liabilities = positions.liabilities.clone();
    for (index, amount) in positions.collateral.iter() {
        positions
            .supply
            .set(index, positions.supply.get(index).unwrap_or(0) + amount);
    }
    positions.collateral = soroban_sdk::Map::new(&f.env);
    f.env.as_contract(&f.pools[0].pool.address, || {
        f.env
            .storage()
            .persistent()
            .set(&pool::PoolDataKey::Positions(user.clone()), &positions);
    });
    let (stock, fork_env) = snapshot_split(
        root,
        case,
        &f.env,
        &f.pools[0].pool.address,
        base_pool,
        fork_pool,
    );
    let stock_id = sc_address(&stock, &f.pools[0].pool.address);
    let fork_id = sc_address(&fork_env, &f.pools[0].pool.address);
    let sc = PoolClient::new(&stock, &stock_id);
    let kc = PoolClient::new(&fork_env, &fork_id);
    stock.mock_auths(&[]);
    fork_env.mock_auths(&[]);
    save(root, case, "fork-before", &fork_env.to_snapshot());
    sc.bad_debt(&sc_address(&stock, user));
    save(root, case, "stock-after", &stock.to_snapshot());
    kc.bad_debt(&sc_address(&fork_env, user));
    save(root, case, "fork-after", &fork_env.to_snapshot());
    let assigned = sc.get_positions(&sc_address(&stock, &f.backstop.address));
    assert!(kc
        .get_positions(&sc_address(&fork_env, &f.backstop.address))
        .liabilities
        .is_empty());
    assert!(sc
        .get_positions(&sc_address(&stock, user))
        .liabilities
        .is_empty());
    let fork_positions = kc.get_positions(&sc_address(&fork_env, user));
    assert!(fork_positions.liabilities.is_empty());
    let mut expected_supply = positions.supply.clone();
    for (index, amount) in liabilities.iter() {
        let (token_index, _) = f.pools[0]
            .reserves
            .iter()
            .find(|(_, id)| **id == index)
            .unwrap();
        let asset = &f.tokens[*token_index].address;
        let base = sc.get_reserve(&sc_address(&stock, asset)).data;
        let fork = kc.get_reserve(&sc_address(&fork_env, asset)).data;
        let claim = positions.supply.get(index).unwrap_or(0);
        // Stock assignment preserves reserve totals and supplies accrued pre-setoff rates.
        // Compute the fork's setoff independently, without its conversion helpers.
        let (burned, repaid) = if claim > 0 && base.b_rate > 0 {
            let debt_assets = (amount * base.d_rate + SCALAR_12 - 1) / SCALAR_12;
            let required_b_tokens = (debt_assets * SCALAR_12 + base.b_rate - 1) / base.b_rate;
            let b_tokens = claim.min(required_b_tokens);
            let covered_assets = b_tokens * base.b_rate / SCALAR_12;
            let d_tokens = amount.min(covered_assets * SCALAR_12 / base.d_rate);
            (if d_tokens > 0 { b_tokens } else { 0 }, d_tokens)
        } else {
            (0, 0)
        };
        let defaulted = amount - repaid;
        let remaining_b_supply = base.b_supply - burned;
        // Production skips the denominator when the setoff burns the whole
        // b_supply; defaulted debt is forgiven at an unchanged b_rate then.
        let loss = if defaulted > 0 && remaining_b_supply > 0 {
            let debt_assets = (defaulted * base.d_rate + SCALAR_12 - 1) / SCALAR_12;
            (debt_assets * SCALAR_12 + remaining_b_supply - 1) / remaining_b_supply
        } else {
            0
        };
        assert_eq!(fork.d_supply, base.d_supply - repaid - defaulted);
        assert_eq!(fork.b_supply, remaining_b_supply);
        assert_eq!(fork.b_rate, (base.b_rate - loss).max(0));
        if burned > 0 {
            if claim == burned {
                expected_supply.remove(index);
            } else {
                expected_supply.set(index, claim - burned);
            }
        }
    }
    assert_eq!(
        fork_positions.supply.to_xdr(&fork_env),
        expected_supply.to_xdr(&f.env)
    );
    // Verified base semantics: stock passes the gross liability list to the
    // backstop (upstream pass-off); fork settles internally, leaving backstop
    // positions untouched.
    assert_eq!(
        assigned.liabilities.to_xdr(&stock),
        liabilities.to_xdr(&f.env)
    );
}
