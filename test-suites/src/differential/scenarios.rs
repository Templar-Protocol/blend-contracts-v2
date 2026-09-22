//! Common-domain runtime replay: no restricted control plane, rewards, or default.
//! Each independent group starts with the same deterministic public fixture.
//! Capture every invocation before another getter can change its auth/TTL state.
//! Direct backstop `draw`/`donate` calls are synthetic-authority body controls,
//! not evidence that the fork has a production pool path to those entrypoints.

use pool::{Request, RequestType};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    vec, Address, IntoVal, Symbol,
};

use crate::create_fixture_with_wasm;
use crate::differential::{
    addr_bytes_of, declare_runtime_identity, Capture, SideCapture, SideHashes,
};
use crate::test_fixture::{TestFixture, TokenIndex, SCALAR_7};

pub fn replay_all(
    hashes: SideHashes,
    pool_wasm: &[u8],
    backstop_wasm: &[u8],
    label: &'static str,
) -> SideCapture {
    let mut fixture = create_fixture_with_wasm(pool_wasm, backstop_wasm);
    let mut cap = Capture::new(label);
    cap.observe(&fixture.env, "init", None);
    replay_supply_collateral_repay_withdraw(&mut fixture, &mut cap);
    replay_interest_and_time(&mut fixture, &mut cap);
    replay_statuses(&mut fixture, &mut cap);

    fixture = create_fixture_with_wasm(pool_wasm, backstop_wasm);
    cap.observe(&fixture.env, "backstop_init", None);
    replay_backstop_deposit_queue_withdraw(&mut fixture, &mut cap);

    fixture = create_fixture_with_wasm(pool_wasm, backstop_wasm);
    cap.observe(&fixture.env, "liquidation_init", None);
    replay_partial_then_full_liquidation(&mut fixture, &mut cap);

    fixture = create_fixture_with_wasm(pool_wasm, backstop_wasm);
    cap.observe(&fixture.env, "stale_init", None);
    replay_stale_auction_deletion(&mut fixture, &mut cap);

    let (backstop_id, factory_id, _salt) = declare_runtime_identity();
    SideCapture {
        label,
        pool_id: addr_bytes_of(&fixture.pools[0].pool.address),
        blnd_id: addr_bytes_of(&fixture.tokens[TokenIndex::BLND].address),
        reserve_asset_id: addr_bytes_of(&fixture.tokens[TokenIndex::XLM].address),
        emitter_id: addr_bytes_of(&fixture.emitter.address),
        backstop_id,
        factory_id,
        hashes,
        capture: cap,
    }
}

fn replay_supply_collateral_repay_withdraw(fixture: &mut TestFixture, cap: &mut Capture) {
    let env = &fixture.env;
    let pool = &fixture.pools[0].pool;
    let user = Address::generate(env);
    let xlm = &fixture.tokens[TokenIndex::XLM];
    let stable = &fixture.tokens[TokenIndex::STABLE];
    xlm.mint(&user, &(160_000 * SCALAR_7));
    cap.observe(env, "lending_mint_xlm", None);
    stable.mint(&user, &(6_000 * 10i128.pow(6)));
    cap.observe(env, "lending_mint_stable", None);

    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: xlm.address.clone(),
                amount: 160_000 * SCALAR_7,
            },
        ],
    );
    cap.value(env, "supply_collateral", positions.clone());
    assert!(positions.collateral.get_unchecked(1) > 0);
    // Initial STABLE cash is 2k; this borrow remains below maximum utilization.
    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::Borrow as u32,
                address: stable.address.clone(),
                amount: 400 * 10i128.pow(6),
            },
        ],
    );
    cap.value(env, "borrow", positions.clone());
    let initial_debt = positions.liabilities.get_unchecked(0);
    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::Repay as u32,
                address: stable.address.clone(),
                amount: 250 * 10i128.pow(6),
            },
        ],
    );
    cap.value(env, "partial_repay", positions.clone());
    let debt = positions.liabilities.get_unchecked(0);
    assert!(debt > 0 && debt < initial_debt);
    let reserve = pool.get_reserve(&stable.address);
    cap.value(env, "repay_reserve", reserve.clone());
    let remaining = reserve.to_asset_from_d_token(env, debt);
    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::Repay as u32,
                address: stable.address.clone(),
                amount: remaining,
            },
        ],
    );
    cap.value(env, "full_repay", positions.clone());
    assert!(positions.liabilities.is_empty());

    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::Supply as u32,
                address: stable.address.clone(),
                amount: 2_000 * 10i128.pow(6),
            },
        ],
    );
    cap.value(env, "supply", positions.clone());
    assert!(positions.supply.get_unchecked(0) > 0);
    let balance = stable.balance(&user);
    cap.value(env, "withdraw_balance_before", balance);
    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::Withdraw as u32,
                address: stable.address.clone(),
                amount: i128::MAX,
            },
        ],
    );
    cap.value(env, "withdraw", positions.clone());
    assert!(positions.supply.is_empty());
    let after = stable.balance(&user);
    cap.value(env, "withdraw_balance_after", after);
    assert!(after > balance);
    let positions = pool.submit(
        &user,
        &user,
        &user,
        &vec![
            env,
            Request {
                request_type: RequestType::WithdrawCollateral as u32,
                address: xlm.address.clone(),
                amount: i128::MAX,
            },
        ],
    );
    cap.value(env, "withdraw_collateral", positions.clone());
    assert!(positions.collateral.is_empty());
}

fn replay_interest_and_time(fixture: &mut TestFixture, cap: &mut Capture) {
    let env = &fixture.env;
    let pool = &fixture.pools[0].pool;
    let frodo = &fixture.users[0];
    let stable = &fixture.tokens[TokenIndex::STABLE].address;
    let before = pool.get_reserve(stable);
    cap.value(env, "interest_reserve_before", before.clone());
    fixture.jump(7 * 24 * 60 * 60);
    cap.observe(env, "interest_week_jump", None);
    let positions = pool.submit(
        frodo,
        frodo,
        frodo,
        &vec![
            env,
            Request {
                request_type: RequestType::Supply as u32,
                address: stable.clone(),
                amount: 100 * 10i128.pow(6),
            },
        ],
    );
    cap.value(env, "interest_sync_supply", positions);
    let after = pool.get_reserve(stable);
    cap.value(env, "interest_reserve_after", after.clone());
    assert!(after.data.d_rate > before.data.d_rate);
    assert!(after.data.b_rate > before.data.b_rate);
}

fn replay_statuses(fixture: &mut TestFixture, cap: &mut Capture) {
    let env = &fixture.env;
    let pool = &fixture.pools[0].pool;
    let config = pool.get_config();
    cap.value(env, "status_initial_config", config.clone());
    assert_eq!(config.status, 1);
    pool.set_status(&3);
    cap.observe(env, "set_status_on_ice", None);
    let config = pool.get_config();
    cap.value(env, "status_on_ice_config", config.clone());
    assert_eq!(config.status, 3);
    let status = pool.update_status();
    cap.value(env, "update_status_active", status);
    assert_eq!(status, 1);
}

fn replay_backstop_deposit_queue_withdraw(fixture: &mut TestFixture, cap: &mut Capture) {
    let env = &fixture.env;
    let pool = &fixture.pools[0].pool.address;
    let frodo = &fixture.users[0];
    let depositor = Address::generate(env);
    let lp_balance = fixture.lp.balance(frodo);
    cap.value(env, "backstop_lp_funder_balance", lp_balance);
    assert!(lp_balance >= 10_000 * SCALAR_7);
    fixture.lp.transfer(frodo, &depositor, &(10_000 * SCALAR_7));
    cap.observe(env, "backstop_lp_funding", None);
    let shares = fixture
        .backstop
        .deposit(&depositor, pool, &(10_000 * SCALAR_7));
    cap.value(env, "backstop_deposit", shares);
    assert!(shares > 0);
    let data = fixture.backstop.pool_data(pool);
    cap.value(env, "backstop_pool_data", data);
    let balance = fixture.backstop.user_balance(pool, &depositor);
    cap.value(env, "backstop_user_balance", balance.clone());
    assert_eq!(balance.shares, shares);
    let queued = shares * 3 / 10;
    let q4w = fixture.backstop.queue_withdrawal(&depositor, pool, &queued);
    cap.value(env, "backstop_queue", q4w.clone());
    assert_eq!(q4w.amount, queued);
    let dequeued = queued / 2;
    fixture
        .backstop
        .dequeue_withdrawal(&depositor, pool, &dequeued);
    cap.observe(env, "backstop_dequeue", None);
    fixture.jump(q4w.exp - env.ledger().timestamp());
    cap.observe(env, "backstop_queue_expiry_jump", None);
    let before = fixture.lp.balance(&depositor);
    cap.value(env, "backstop_withdraw_balance_before", before);
    let withdrawn = fixture
        .backstop
        .withdraw(&depositor, pool, &(queued - dequeued));
    cap.value(env, "backstop_withdraw", withdrawn);
    assert!(withdrawn > 0);
    let after = fixture.lp.balance(&depositor);
    cap.value(env, "backstop_withdraw_balance_after", after);
    assert_eq!(after - before, withdrawn);

    // The fixture mocks every required signer. These direct calls verify the
    // shared backstop bodies and exact auth trees, not production reachability.

    let amount = 1_000 * SCALAR_7;
    fixture.backstop.draw(pool, &amount, frodo);
    cap.observe(env, "backstop_draw", None);
    assert_eq!(
        env.auths(),
        std::vec![(
            pool.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    fixture.backstop.address.clone(),
                    Symbol::new(env, "draw"),
                    (pool.clone(), amount, frodo.clone()).into_val(env)
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
    fixture.lp.approve(
        frodo,
        &fixture.backstop.address,
        &amount,
        &(env.ledger().sequence() + 100),
    );
    cap.observe(env, "backstop_donate_approve", None);
    let allowance = fixture.lp.allowance(frodo, &fixture.backstop.address);
    cap.value(env, "backstop_donate_allowance_before", allowance);
    assert_eq!(allowance, amount);
    fixture.backstop.donate(frodo, pool, &amount);
    cap.observe(env, "backstop_donate", None);
    let invocation = AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            fixture.backstop.address.clone(),
            Symbol::new(env, "donate"),
            (frodo.clone(), pool.clone(), amount).into_val(env),
        )),
        sub_invocations: std::vec![],
    };
    assert_eq!(
        env.auths(),
        std::vec![
            (frodo.clone(), invocation.clone()),
            (pool.clone(), invocation)
        ]
    );
    let allowance = fixture.lp.allowance(frodo, &fixture.backstop.address);
    cap.value(env, "backstop_donate_allowance_after", allowance);
    assert_eq!(allowance, 0);
    let zone = fixture.backstop.reward_zone();
    cap.value(env, "backstop_reward_zone", zone.clone());
    assert!(zone.is_empty());
}

fn replay_partial_then_full_liquidation(fixture: &mut TestFixture, cap: &mut Capture) {
    let env = &fixture.env;
    let pool = &fixture.pools[0].pool;
    let frodo = &fixture.users[0];
    let sam = Address::generate(env);
    let stable = &fixture.tokens[TokenIndex::STABLE];
    let xlm = &fixture.tokens[TokenIndex::XLM];
    let weth = &fixture.tokens[TokenIndex::WETH];
    // Retained test_liquidation: fund the 28k borrow before creating Sam.
    let positions = pool.submit(
        frodo,
        frodo,
        frodo,
        &vec![
            env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: stable.address.clone(),
                amount: 30_000 * 10i128.pow(6),
            },
        ],
    );
    cap.value(env, "liq_supplier_funding", positions);
    xlm.mint(&sam, &(500_000 * SCALAR_7));
    cap.observe(env, "liq_mint_xlm", None);
    weth.mint(&sam, &(50 * 10i128.pow(9)));
    cap.observe(env, "liq_mint_weth", None);
    let positions = pool.submit(
        &sam,
        &sam,
        &sam,
        &vec![
            env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: xlm.address.clone(),
                amount: 160_000 * SCALAR_7,
            },
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: weth.address.clone(),
                amount: 17 * 10i128.pow(9),
            },
            Request {
                request_type: RequestType::Borrow as u32,
                address: stable.address.clone(),
                amount: 28_000 * 10i128.pow(6),
            },
            Request {
                request_type: RequestType::Borrow as u32,
                address: xlm.address.clone(),
                amount: 65_000 * SCALAR_7,
            },
        ],
    );
    cap.value(env, "liq_borrow_open", positions);
    for _ in 0..12 {
        fixture.jump(7 * 24 * 60 * 60);
        cap.observe(env, "liq_interest_week", None);
    }
    let bids = vec![env, stable.address.clone(), xlm.address.clone()];
    let lots = vec![env, weth.address.clone(), xlm.address.clone()];
    let auction = pool.new_auction(&0, &sam, &bids, &lots, &30);
    cap.value(env, "partial_liq_new_auction", auction);
    // Actual collateral, not merely a wallet balance, protects the filler.
    stable.mint(frodo, &(100_000 * 10i128.pow(6)));
    cap.observe(env, "liq_filler_mint", None);
    let positions = pool.submit(
        frodo,
        frodo,
        frodo,
        &vec![
            env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: stable.address.clone(),
                amount: 100_000 * 10i128.pow(6),
            },
        ],
    );
    cap.value(env, "liq_filler_collateral", positions);
    fixture.jump_with_sequence(101 * 5);
    cap.observe(env, "partial_liq_block_jump", None);
    let positions = pool.submit(
        frodo,
        frodo,
        frodo,
        &vec![
            env,
            Request {
                request_type: RequestType::FillUserLiquidationAuction as u32,
                address: sam.clone(),
                amount: 25,
            },
        ],
    );
    cap.value(env, "partial_liq_fill_25", positions);
    let remainder = pool.get_auction(&0, &sam);
    cap.value(env, "partial_liq_remaining_auction", remainder);
    let positions = pool.submit(
        frodo,
        frodo,
        frodo,
        &vec![
            env,
            Request {
                request_type: RequestType::FillUserLiquidationAuction as u32,
                address: sam.clone(),
                amount: 100,
            },
        ],
    );
    cap.value(env, "partial_liq_fill_remainder", positions);

    fixture
        .oracle
        .set_price_stable(&vec![env, 500_0000000, 1_0000000, 0_1000000, 1_0000000]);
    cap.observe(env, "full_liq_reprice_weth", None);
    let borrower = pool.get_positions(&sam);
    cap.value(env, "full_liq_borrower_before", borrower.clone());
    let auction = pool.new_auction(&0, &sam, &bids, &lots, &100);
    cap.value(env, "full_liq_new_auction", auction.clone());
    assert_eq!(
        auction.bid.get_unchecked(stable.address.clone()),
        borrower.liabilities.get_unchecked(0)
    );
    assert_eq!(
        auction.bid.get_unchecked(xlm.address.clone()),
        borrower.liabilities.get_unchecked(1)
    );
    // Auction starts next ledger: +201 means block_dif 200, exactly 100% bid
    // and lot. +251 would default residual debt and is outside this partition.
    fixture.jump_with_sequence(201 * 5);
    cap.observe(env, "full_liq_block_jump", None);
    let custody = pool.get_positions(&pool.address);
    cap.value(env, "full_liq_custody_before", custody.clone());
    let positions = pool.submit(
        frodo,
        frodo,
        frodo,
        &vec![
            env,
            Request {
                request_type: RequestType::FillUserLiquidationAuction as u32,
                address: sam.clone(),
                amount: 100,
            },
        ],
    );
    cap.value(env, "full_liq_fill", positions);
    let borrower = pool.get_positions(&sam);
    cap.value(env, "full_liq_borrower_after", borrower.clone());
    assert!(borrower.liabilities.is_empty());
    assert!(borrower.collateral.is_empty());
    let filler = pool.get_positions(frodo);
    cap.value(env, "full_liq_filler_after", filler.clone());
    let custody_after = pool.get_positions(&pool.address);
    cap.value(env, "full_liq_custody_after", custody_after.clone());
    use soroban_sdk::xdr::ToXdr;
    assert!(
        custody.supply.is_empty(),
        "common-domain fixture has no orphan custody"
    );
    assert_eq!(
        custody_after.to_xdr(env),
        custody.to_xdr(env),
        "ordinary full fill cannot create custody"
    );
}

fn replay_stale_auction_deletion(fixture: &mut TestFixture, cap: &mut Capture) {
    let env = &fixture.env;
    let pool = &fixture.pools[0].pool;
    let sam = Address::generate(env);
    let stable = &fixture.tokens[TokenIndex::STABLE];
    let xlm = &fixture.tokens[TokenIndex::XLM].address;
    // Retained test_stale_liquidation_deletion: legal open, then debt repricing.
    stable.mint(&sam, &(1_000 * 10i128.pow(6)));
    cap.observe(env, "stale_mint_stable", None);
    let positions = pool.submit(
        &sam,
        &sam,
        &sam,
        &vec![
            env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: stable.address.clone(),
                amount: 1_000 * 10i128.pow(6),
            },
            Request {
                request_type: RequestType::Borrow as u32,
                address: xlm.clone(),
                amount: 6_075 * SCALAR_7,
            },
        ],
    );
    cap.value(env, "stale_borrow_open", positions);
    fixture.jump(14 * 24 * 60 * 60);
    cap.observe(env, "stale_interest_jump", None);
    fixture
        .oracle
        .set_price_stable(&vec![env, 2000_0000000, 1_0000000, 0_1200000, 1_0000000]);
    cap.observe(env, "stale_debt_reprice", None);
    let auction = pool.new_auction(
        &0,
        &sam,
        &vec![env, xlm.clone()],
        &vec![env, stable.address.clone()],
        &50,
    );
    cap.value(env, "stale_new_auction", auction.clone());
    fixture.jump_with_sequence(500 * 5);
    cap.observe(env, "stale_early_block_jump", None);
    let early_delete = pool.try_del_auction(&0, &sam);
    cap.observe(
        env,
        "stale_early_delete",
        Some(format!("{:?}", early_delete.as_ref().err())),
    );
    assert_eq!(
        early_delete.err(),
        Some(Ok(soroban_sdk::Error::from_contract_error(1200)))
    );
    let still_present = pool.get_auction(&0, &sam);
    assert_eq!(still_present.bid.len(), 1);
    assert_eq!(still_present.lot.len(), 1);
    cap.value(env, "stale_auction_retained", still_present);
    fixture.jump_with_sequence(5);
    cap.observe(env, "stale_expiry_block_jump", None);
    pool.del_auction(&0, &sam);
    cap.observe(env, "stale_del_auction", None);
    assert!(env.auths().is_empty());
    let deleted = pool.try_get_auction(&0, &sam);
    cap.observe(
        env,
        "stale_deleted_auction",
        Some(format!("{:?}", deleted.as_ref().err())),
    );
    assert_eq!(
        deleted.err(),
        Some(Ok(soroban_sdk::Error::from_type_and_code(
            soroban_sdk::xdr::ScErrorType::Context,
            soroban_sdk::xdr::ScErrorCode::InvalidAction,
        )))
    );
}
