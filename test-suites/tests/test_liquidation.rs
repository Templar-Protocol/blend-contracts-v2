#![cfg(test)]
use cast::i128;
use pool::{AuctionData, PoolDataKey, Request, RequestType, ReserveConfig};
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{
    map,
    testutils::{Address as AddressTestTrait, Events},
    vec, Address, Env, Error, FromVal, IntoVal, Symbol, TryFromVal, Val, Vec,
};
use test_suites::{
    assertions::{assert_approx_eq_abs, assert_approx_eq_rel},
    create_fixture_with_data,
    test_fixture::{TokenIndex, SCALAR_7},
};

fn assert_fill_auction_event_no_data(
    env: &Env,
    event: (Address, Vec<Val>, Val),
    pool_address: &Address,
    auction_user: &Address,
    auction_type: u32,
    filler: &Address,
    fill_pct: i128,
) {
    let (event_pool_address, topics, data) = event;
    assert_eq!(event_pool_address, pool_address.clone());

    assert_eq!(topics.len(), 3);
    assert_eq!(
        Symbol::from_val(env, &topics.get_unchecked(0)),
        Symbol::new(env, "fill_auction")
    );
    assert_eq!(u32::from_val(env, &topics.get_unchecked(1)), auction_type);
    assert_eq!(
        Address::from_val(env, &topics.get_unchecked(2)),
        auction_user.clone()
    );

    let event_data = Vec::<Val>::from_val(env, &data);
    assert_eq!(event_data.len(), 3);
    assert_eq!(
        Address::from_val(env, &event_data.get_unchecked(0)),
        filler.clone()
    );
    assert_eq!(i128::from_val(env, &event_data.get_unchecked(1)), fill_pct);
    assert!(AuctionData::try_from_val(env, &event_data.get_unchecked(2)).is_ok());
}

#[test]
fn test_liquidations() {
    let fixture = create_fixture_with_data(false);
    let frodo = fixture.users.get(0).unwrap();
    let pool_fixture = &fixture.pools[0];

    // accrue interest
    let requests: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 10,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 10,
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 10,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 10,
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::WETH].address.clone(),
            amount: 10,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::WETH].address.clone(),
            amount: 10,
        },
    ];
    pool_fixture.pool.submit(&frodo, &frodo, &frodo, &requests);

    // Disable rate modifiers
    let mut usdc_config: ReserveConfig = fixture.read_reserve_config(0, TokenIndex::STABLE);
    usdc_config.reactivity = 0;

    let mut xlm_config: ReserveConfig = fixture.read_reserve_config(0, TokenIndex::XLM);
    xlm_config.reactivity = 0;
    let mut weth_config: ReserveConfig = fixture.read_reserve_config(0, TokenIndex::WETH);
    weth_config.reactivity = 0;

    fixture.env.as_contract(&fixture.pools[0].pool.address, || {
        let key = PoolDataKey::ResConfig(fixture.tokens[TokenIndex::STABLE].address.clone());
        fixture
            .env
            .storage()
            .persistent()
            .set::<PoolDataKey, ReserveConfig>(&key, &usdc_config);
        let key = PoolDataKey::ResConfig(fixture.tokens[TokenIndex::XLM].address.clone());
        fixture
            .env
            .storage()
            .persistent()
            .set::<PoolDataKey, ReserveConfig>(&key, &xlm_config);
        let key = PoolDataKey::ResConfig(fixture.tokens[TokenIndex::WETH].address.clone());
        fixture
            .env
            .storage()
            .persistent()
            .set::<PoolDataKey, ReserveConfig>(&key, &weth_config);
    });

    // have Frodo Q4W some backstop deposits
    let frodo_pre_q4w_amount = 10_000 * SCALAR_7;
    fixture
        .backstop
        .queue_withdrawal(&frodo, &pool_fixture.pool.address, &frodo_pre_q4w_amount);

    // Create a user
    let samwise = Address::generate(&fixture.env); //sam will be supplying XLM and borrowing STABLE

    // Mint users tokens
    fixture.tokens[TokenIndex::XLM].mint(&samwise, &(500_000 * SCALAR_7));
    fixture.tokens[TokenIndex::WETH].mint(&samwise, &(50 * 10i128.pow(9)));
    fixture.tokens[TokenIndex::USDC].mint(&frodo, &(100_000 * SCALAR_7));

    let frodo_requests: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 30_000 * 10i128.pow(6),
        },
    ];
    // Supply frodo tokens
    pool_fixture
        .pool
        .submit(&frodo, &frodo, &frodo, &frodo_requests);
    // Supply and borrow sam tokens
    let sam_requests: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 160_000 * SCALAR_7,
        },
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::WETH].address.clone(),
            amount: 17 * 10i128.pow(9),
        },
        // Sam's max borrow is 39_200 STABLE
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 28_000 * 10i128.pow(6),
        }, // reduces Sam's max borrow to 14_526.31579 STABLE
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 65_000 * SCALAR_7,
        },
    ];
    let sam_positions = pool_fixture
        .pool
        .submit(&samwise, &samwise, &samwise, &sam_requests);

    //Utilization is now:
    // * 36_000 / 40_000 = .9 for STABLE
    // * 130_000 / 260_000 = .5 for XLM
    // This equates to the following rough annual interest rates
    //  * 31% for STABLE borrowing
    //  * 25.11% for STABLE lending
    //  * rate will be dragged up to rate modifier
    //  * 6% for XLM borrowing
    //  * 2.7% for XLM lending

    // Let three months go by so ordinary interest accrues. The weekly
    // emissions distribution/gulp calls are gone under the fork (globally
    // unconfigured rewards in this cohort), leaving pure time advancement.
    for _ in 0..12 {
        fixture.jump(60 * 60 * 24 * 7);
    }
    let liq_pct = 30;
    // Start a liquidation auction
    let auction_data = pool_fixture.pool.new_auction(
        &0,
        &samwise,
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::STABLE].address.clone(),
            fixture.tokens[TokenIndex::XLM].address.clone(),
        ],
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::WETH].address.clone(),
            fixture.tokens[TokenIndex::XLM].address.clone(),
        ],
        &liq_pct,
    );
    let usdc_bid_amount = auction_data
        .bid
        .get_unchecked(fixture.tokens[TokenIndex::STABLE].address.clone());
    assert_approx_eq_abs(
        usdc_bid_amount,
        sam_positions
            .liabilities
            .get(0)
            .unwrap()
            .fixed_mul_ceil(i128(liq_pct * 100000), SCALAR_7)
            .unwrap(),
        SCALAR_7,
    );
    let xlm_bid_amount = auction_data
        .bid
        .get_unchecked(fixture.tokens[TokenIndex::XLM].address.clone());
    assert_approx_eq_abs(
        xlm_bid_amount,
        sam_positions
            .liabilities
            .get(1)
            .unwrap()
            .fixed_mul_ceil(i128(liq_pct * 100000), SCALAR_7)
            .unwrap(),
        SCALAR_7,
    );
    let xlm_lot_amount = auction_data
        .lot
        .get_unchecked(fixture.tokens[TokenIndex::XLM].address.clone());
    let weth_lot_amount = auction_data
        .lot
        .get_unchecked(fixture.tokens[TokenIndex::WETH].address.clone());
    // Both assets contribute the same proportional collateral slice, rounded
    // up by at most one b-token. Do not pin stock-take interest-era lot values.
    assert!(xlm_lot_amount > 0 && xlm_lot_amount < sam_positions.collateral.get_unchecked(1));
    assert!(weth_lot_amount > 0 && weth_lot_amount < sam_positions.collateral.get_unchecked(2));
    assert_approx_eq_abs(
        xlm_lot_amount
            .fixed_div_floor(sam_positions.collateral.get_unchecked(1), SCALAR_7)
            .unwrap(),
        weth_lot_amount
            .fixed_div_floor(sam_positions.collateral.get_unchecked(2), SCALAR_7)
            .unwrap(),
        1,
    );
    let events = fixture.env.events().all();
    let event = vec![&fixture.env, events.get_unchecked(events.len() - 1)];
    assert_eq!(
        event,
        vec![
            &fixture.env,
            (
                pool_fixture.pool.address.clone(),
                (
                    Symbol::new(&fixture.env, "new_auction"),
                    0 as u32,
                    samwise.clone(),
                )
                    .into_val(&fixture.env),
                (liq_pct, auction_data.clone()).into_val(&fixture.env)
            )
        ]
    );

    //let 100 blocks pass to scale up the modifier
    fixture.jump_with_sequence(101 * 5);
    // Repay some existing debt, then fill one user auction in two installments.
    let auct_type_1: u32 = 0;
    let fill_requests = vec![
        &fixture.env,
        // This is an underlying-token repayment, not a d-token quantity.
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: usdc_bid_amount,
        },
        Request {
            request_type: RequestType::FillUserLiquidationAuction as u32,
            address: samwise.clone(),
            amount: 25,
        },
        Request {
            request_type: RequestType::FillUserLiquidationAuction as u32,
            address: samwise.clone(),
            amount: 100,
        },
    ];
    let frodo_stable_balance = fixture.tokens[TokenIndex::STABLE].balance(&frodo);
    let frodo_xlm_balance = fixture.tokens[TokenIndex::XLM].balance(&frodo);
    let frodo_weth_balance = fixture.tokens[TokenIndex::WETH].balance(&frodo);
    let frodo_positions_pre = pool_fixture.pool.get_positions(&frodo);
    let stable_res_pre_partial = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::STABLE].address);
    let repaid_before_partial =
        stable_res_pre_partial.to_d_token_down(&fixture.env, usdc_bid_amount);
    assert!(repaid_before_partial <= frodo_positions_pre.liabilities.get_unchecked(0));
    let frodo_positions_post_fill =
        pool_fixture
            .pool
            .submit(&frodo, &frodo, &frodo, &fill_requests);
    let events = fixture.env.events().all();
    assert_approx_eq_abs(
        frodo_positions_post_fill.collateral.get_unchecked(2),
        weth_lot_amount
            .fixed_div_floor(2_0000000, SCALAR_7)
            .unwrap()
            + 10 * 10i128.pow(9),
        1000,
    );
    assert_approx_eq_abs(
        frodo_positions_post_fill.collateral.get_unchecked(1),
        xlm_lot_amount.fixed_div_floor(2_0000000, SCALAR_7).unwrap() + 100_000 * SCALAR_7,
        1000,
    );
    assert_approx_eq_abs(
        frodo_positions_post_fill.liabilities.get_unchecked(1),
        xlm_bid_amount + 65_000 * SCALAR_7,
        1000,
    );
    assert_approx_eq_abs(
        i128::from(frodo_positions_post_fill.liabilities.get_unchecked(0))
            - i128::from(frodo_positions_pre.liabilities.get(0).unwrap_or(0)),
        usdc_bid_amount - repaid_before_partial,
        10i128.pow(6),
    );
    // Token transfers occur after request processing; select the actual fill
    // events rather than relying on their distance from the end of the log.
    let fill_topics: Vec<Val> = (
        Symbol::new(&fixture.env, "fill_auction"),
        0u32,
        samwise.clone(),
    )
        .into_val(&fixture.env);
    let mut fills = events.iter().filter(|(contract, topics, _)| {
        contract == &pool_fixture.pool.address && topics == &fill_topics
    });
    assert_fill_auction_event_no_data(
        &fixture.env,
        fills.next().unwrap(),
        &pool_fixture.pool.address,
        &samwise,
        auct_type_1,
        &frodo,
        25,
    );
    assert_fill_auction_event_no_data(
        &fixture.env,
        fills.next().unwrap(),
        &pool_fixture.pool.address,
        &samwise,
        auct_type_1,
        &frodo,
        100,
    );
    assert!(fills.next().is_none());
    // Fills settle through internal b-token position swaps; the only token
    // movement is the repay burning its own STABLE liability funding.
    assert_approx_eq_abs(
        fixture.tokens[TokenIndex::STABLE].balance(&frodo),
        frodo_stable_balance - usdc_bid_amount,
        10i128.pow(6),
    );
    assert_approx_eq_abs(
        fixture.tokens[TokenIndex::XLM].balance(&frodo),
        frodo_xlm_balance,
        SCALAR_7,
    );
    assert_approx_eq_abs(
        fixture.tokens[TokenIndex::WETH].balance(&frodo),
        frodo_weth_balance,
        10i128.pow(9),
    );

    //tank eth price
    fixture.oracle.set_price_stable(&vec![
        &fixture.env,
        500_0000000, // eth
        1_0000000,   // usdc
        0_1000000,   // xlm
        1_0000000,   // stable
    ]);

    //fully liquidate user
    let samwise_pre_full_liq = pool_fixture.pool.get_positions(&samwise);
    let liq_pct = 100;
    let auction_data_2 = pool_fixture.pool.new_auction(
        &0,
        &samwise,
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::STABLE].address.clone(),
            fixture.tokens[TokenIndex::XLM].address.clone(),
        ],
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::WETH].address.clone(),
            fixture.tokens[TokenIndex::XLM].address.clone(),
        ],
        &liq_pct,
    );

    let stable_bid_amount = auction_data_2
        .bid
        .get_unchecked(fixture.tokens[TokenIndex::STABLE].address.clone());
    assert_eq!(
        stable_bid_amount,
        samwise_pre_full_liq.liabilities.get_unchecked(0)
    );
    let xlm_bid_amount = auction_data_2
        .bid
        .get_unchecked(fixture.tokens[TokenIndex::XLM].address.clone());
    assert_eq!(
        xlm_bid_amount,
        samwise_pre_full_liq.liabilities.get_unchecked(1)
    );
    let xlm_lot_amount = auction_data_2
        .lot
        .get_unchecked(fixture.tokens[TokenIndex::XLM].address.clone());
    assert_eq!(
        xlm_lot_amount,
        samwise_pre_full_liq.collateral.get_unchecked(1)
    );
    let weth_lot_amount = auction_data_2
        .lot
        .get_unchecked(fixture.tokens[TokenIndex::WETH].address.clone());
    assert_eq!(
        weth_lot_amount,
        samwise_pre_full_liq.collateral.get_unchecked(2)
    );

    //allow 250 blocks to pass
    fixture.jump_with_sequence(251 * 5);

    // Full liquidation transfers 75% of debt to the filler and defaults the
    // remainder directly to suppliers; subsequent requests repay both assets.
    let frodo_stable_balance = fixture.tokens[TokenIndex::STABLE].balance(&frodo);
    let frodo_xlm_balance = fixture.tokens[TokenIndex::XLM].balance(&frodo);
    let fill_requests = vec![
        &fixture.env,
        Request {
            request_type: RequestType::FillUserLiquidationAuction as u32,
            address: samwise.clone(),
            amount: 100,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: stable_bid_amount
                .fixed_div_floor(2_0000000, SCALAR_7)
                .unwrap(),
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: xlm_bid_amount.fixed_div_floor(2_0000000, SCALAR_7).unwrap(),
        },
    ];
    let stable_filled = stable_bid_amount
        .fixed_mul_ceil(0_7500000, SCALAR_7)
        .unwrap();
    let xlm_filled = xlm_bid_amount.fixed_mul_ceil(0_7500000, SCALAR_7).unwrap();
    // Capture reserves before the full-percent fill: whatever bid it leaves
    // unpaid is defaulted straight to suppliers under the fork.
    let stable_res_pre_full_liq = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::STABLE].address);
    let xlm_res_pre_full_liq = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::XLM].address);
    let stable_repayment = stable_bid_amount / 2;
    let xlm_repayment = xlm_bid_amount / 2;
    let stable_repaid = stable_res_pre_full_liq.to_d_token_down(&fixture.env, stable_repayment);
    let xlm_repaid = xlm_res_pre_full_liq.to_d_token_down(&fixture.env, xlm_repayment);
    let new_frodo_positions = pool_fixture
        .pool
        .submit(&frodo, &frodo, &frodo, &fill_requests);
    assert_approx_eq_abs(
        frodo_positions_post_fill.collateral.get(1).unwrap() + xlm_lot_amount,
        new_frodo_positions.collateral.get(1).unwrap(),
        SCALAR_7,
    );
    assert_approx_eq_abs(
        frodo_positions_post_fill.collateral.get(2).unwrap() + weth_lot_amount,
        new_frodo_positions.collateral.get(2).unwrap(),
        SCALAR_7,
    );
    assert_approx_eq_abs(
        frodo_positions_post_fill.liabilities.get(0).unwrap() + stable_filled - stable_repaid,
        new_frodo_positions.liabilities.get(0).unwrap(),
        10i128.pow(6),
    );
    assert_approx_eq_abs(
        frodo_positions_post_fill.liabilities.get(1).unwrap() + xlm_filled - xlm_repaid,
        new_frodo_positions.liabilities.get(1).unwrap(),
        SCALAR_7,
    );
    assert_approx_eq_abs(
        frodo_stable_balance - stable_repayment,
        fixture.tokens[TokenIndex::STABLE].balance(&frodo),
        10i128.pow(6),
    );
    assert_approx_eq_abs(
        frodo_xlm_balance - xlm_repayment,
        fixture.tokens[TokenIndex::XLM].balance(&frodo),
        SCALAR_7,
    );

    // The debtor is cleared and the backstop never receives its debt.
    let samwise_positions_post_bd = pool_fixture.pool.get_positions(&samwise);
    assert_eq!(samwise_positions_post_bd.liabilities.len(), 0);
    assert_eq!(samwise_positions_post_bd.collateral.len(), 0);
    // bid scaled to 75%, so 25% is bad debt
    let stable_bad_debt = samwise_pre_full_liq
        .liabilities
        .get(0)
        .unwrap()
        .fixed_mul_floor(0_2500000, SCALAR_7)
        .unwrap();
    let xlm_bad_debt = samwise_pre_full_liq
        .liabilities
        .get(1)
        .unwrap()
        .fixed_mul_floor(0_2500000, SCALAR_7)
        .unwrap();
    // The full-percent fill consumed every lot and settled 75% of the book;
    // the unpaid 25% residual is defaulted against suppliers via b-rate
    // writeback, so the backstop never takes custody under the fork.
    let backstop_positions = pool_fixture.pool.get_positions(&fixture.backstop.address);
    assert_eq!(backstop_positions.liabilities.len(), 0);
    let stable_res_post_full_liq = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::STABLE].address);
    assert_eq!(
        stable_res_post_full_liq.data.d_supply,
        stable_res_pre_full_liq.data.d_supply - stable_bad_debt - stable_repaid
    );
    assert!(stable_res_post_full_liq.data.b_rate < stable_res_pre_full_liq.data.b_rate);
    assert_approx_eq_abs(
        stable_res_pre_full_liq.total_supply(&fixture.env)
            - stable_res_post_full_liq.total_supply(&fixture.env),
        stable_res_post_full_liq.to_asset_from_d_token(&fixture.env, stable_bad_debt),
        0_0000100,
    );
    let xlm_res_post_full_liq = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::XLM].address);
    assert_eq!(
        xlm_res_post_full_liq.data.d_supply,
        xlm_res_pre_full_liq.data.d_supply - xlm_bad_debt - xlm_repaid
    );
    assert!(xlm_res_post_full_liq.data.b_rate < xlm_res_pre_full_liq.data.b_rate);
    assert_approx_eq_abs(
        xlm_res_pre_full_liq.total_supply(&fixture.env)
            - xlm_res_post_full_liq.total_supply(&fixture.env),
        xlm_res_post_full_liq.to_asset_from_d_token(&fixture.env, xlm_bad_debt),
        0_0000100,
    );

    // Ordinary queued and later withdrawals are not slashed by supplier defaults.
    let original_deposit = 50_000 * SCALAR_7;
    let original_deposit_remaining = original_deposit - frodo_pre_q4w_amount;
    let pre_withdraw_frodo_bstp = fixture.lp.balance(&frodo);
    // withdraw pre_q4w_amount
    fixture
        .backstop
        .withdraw(&frodo, &pool_fixture.pool.address, &frodo_pre_q4w_amount);
    fixture.backstop.queue_withdrawal(
        &frodo,
        &pool_fixture.pool.address,
        &original_deposit_remaining,
    );
    //jump a month
    fixture.jump(45 * 24 * 60 * 60);
    fixture.backstop.withdraw(
        &frodo,
        &pool_fixture.pool.address,
        &original_deposit_remaining,
    );
    // With auction-era slashes and donation receipts gone, frodo's shares
    // come back one-for-one modulo share rounding.
    assert_approx_eq_abs(
        fixture.lp.balance(&frodo) - pre_withdraw_frodo_bstp,
        original_deposit,
        SCALAR_7,
    );

    // Test bad debt is burned and defaulted correctly
    // Deposit barely over the minimum backstop threshold in tokens
    fixture
        .backstop
        .deposit(&frodo, &pool_fixture.pool.address, &1100_0000000);

    // Sam re-borrows
    let sam_requests: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::WETH].address.clone(),
            amount: 1 * 10i128.pow(9),
        },
        // Sam's max borrow is 39_200 STABLE
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 100 * 10i128.pow(6),
        }, // reduces Sam's max borrow to 14_526.31579 STABLE
    ];
    let sam_positions = pool_fixture
        .pool
        .submit(&samwise, &samwise, &samwise, &sam_requests);

    // Nuke eth price more
    fixture.oracle.set_price_stable(&vec![
        &fixture.env,
        10_0000000, // eth
        1_0000000,  // usdc
        0_1000000,  // xlm
        1_0000000,  // stable
    ]);

    // Liquidate sam
    let liq_pct: u32 = 100;
    let auction_data = pool_fixture.pool.new_auction(
        &0,
        &samwise,
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::STABLE].address.clone(),
        ],
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::WETH].address.clone(),
        ],
        &liq_pct,
    );
    let usdc_bid_amount = auction_data
        .bid
        .get_unchecked(fixture.tokens[TokenIndex::STABLE].address.clone());
    assert_approx_eq_abs(
        usdc_bid_amount,
        sam_positions
            .liabilities
            .get(0)
            .unwrap()
            .fixed_mul_ceil(i128(liq_pct * 100000), SCALAR_7)
            .unwrap(),
        SCALAR_7,
    );

    //jump 400 blocks
    fixture.jump_with_sequence(401 * 5);

    let frodo_before_zero_bid = pool_fixture.pool.get_positions(&frodo);
    let stable_before_zero_bid = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::STABLE].address);
    //fill liq
    let bad_debt_fill_request = vec![
        &fixture.env,
        Request {
            request_type: RequestType::FillUserLiquidationAuction as u32,
            address: samwise.clone(),
            amount: 100,
        },
    ];
    pool_fixture
        .pool
        .submit(&frodo, &frodo, &frodo, &bad_debt_fill_request);
    let zero_bid_events = fixture.env.events().all();
    // At 400 blocks the bid is ZERO despite the requested 100% fill:
    // all residual debt defaults, while the filler only receives collateral.
    let samwise_post_full_fill = pool_fixture.pool.get_positions(&samwise);
    assert_eq!(samwise_post_full_fill.liabilities.len(), 0);
    assert_eq!(samwise_post_full_fill.collateral.len(), 0);
    let stable_after_zero_bid = pool_fixture
        .pool
        .get_reserve(&fixture.tokens[TokenIndex::STABLE].address);
    let defaulted = sam_positions.liabilities.get_unchecked(0);
    assert_eq!(
        stable_after_zero_bid.data.d_supply,
        stable_before_zero_bid.data.d_supply - defaulted
    );
    assert_eq!(
        stable_after_zero_bid.data.b_supply,
        stable_before_zero_bid.data.b_supply
    );
    assert!(stable_after_zero_bid.data.b_rate < stable_before_zero_bid.data.b_rate);
    assert_eq!(
        pool_fixture.pool.get_positions(&frodo).liabilities,
        frodo_before_zero_bid.liabilities
    );
    let default_topics: Vec<Val> = (
        Symbol::new(&fixture.env, "defaulted_debt"),
        fixture.tokens[TokenIndex::STABLE].address.clone(),
    )
        .into_val(&fixture.env);
    assert!(zero_bid_events
        .iter()
        .any(
            |(contract, topics, data)| contract == pool_fixture.pool.address
                && topics == default_topics
                && i128::try_from_val(&fixture.env, &data).ok() == Some(defaulted)
        ));
}

#[test]
fn test_user_restore_position_and_delete_liquidation() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];
    let stable_pool_index = pool_fixture.reserves[&TokenIndex::STABLE];
    let xlm_pool_index = pool_fixture.reserves[&TokenIndex::XLM];

    // Create a user that is supply STABLE (cf = 90%, $1) and borrowing XLM (lf = 75%, $0.10)
    let samwise = Address::generate(&fixture.env);
    fixture.tokens[TokenIndex::STABLE].mint(&samwise, &(1100 * 10i128.pow(6)));
    fixture.tokens[TokenIndex::XLM].mint(&samwise, &(10000 * SCALAR_7));

    // deposit $1k stable and borrow to 90% borrow limit ($810)
    let setup_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 1000 * 10i128.pow(6),
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 6075 * SCALAR_7,
        },
    ];
    pool_fixture
        .pool
        .submit(&samwise, &samwise, &samwise, &setup_request);

    // simulate 20% XLM price increase ($972 liabilities, $900 limit) and create user liquidation
    fixture.oracle.set_price_stable(&vec![
        &fixture.env,
        2000_0000000, // eth
        1_0000000,    // usdc
        0_1200000,    // xlm
        1_0000000,    // stable
    ]);
    pool_fixture.pool.new_auction(
        &0,
        &samwise,
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::XLM].address.clone(),
        ],
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::STABLE].address.clone(),
        ],
        &50,
    );
    assert!(pool_fixture.pool.try_get_auction(&0, &samwise).is_ok());

    // jump 200 blocks
    fixture.jump_with_sequence(200 * 5);

    // validate liquidation can't be deleted without restoring position
    let delete_only_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::DeleteLiquidationAuction as u32,
            address: Address::generate(&fixture.env),
            amount: i128::MAX,
        },
    ];
    let delete_only =
        pool_fixture
            .pool
            .try_submit(&samwise, &samwise, &samwise, &delete_only_request);
    assert_eq!(
        delete_only.err(),
        Some(Ok(Error::from_contract_error(1205)))
    );

    // validate health factor must be fully restored before deleting position
    let short_supply_delete_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 79 * 10i128.pow(6), // need $80 more collateral
        },
        Request {
            request_type: RequestType::DeleteLiquidationAuction as u32,
            address: Address::generate(&fixture.env),
            amount: i128::MAX,
        },
    ];
    let short_supply_delete =
        pool_fixture
            .pool
            .try_submit(&samwise, &samwise, &samwise, &short_supply_delete_request);
    assert_eq!(
        short_supply_delete.err(),
        Some(Ok(Error::from_contract_error(1205)))
    );

    let short_repay_delete_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::DeleteLiquidationAuction as u32,
            address: Address::generate(&fixture.env),
            amount: i128::MAX,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 449 * SCALAR_7, // need to repay 450 XLM
        },
    ];
    let short_repay_delete =
        pool_fixture
            .pool
            .try_submit(&samwise, &samwise, &samwise, &short_repay_delete_request);
    assert_eq!(
        short_repay_delete.err(),
        Some(Ok(Error::from_contract_error(1205)))
    );

    // validate positions can't be modified without deleting liquidation
    let healthy_no_delete_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 10000 * SCALAR_7,
        },
    ];
    let healthy_no_delete =
        pool_fixture
            .pool
            .try_submit(&samwise, &samwise, &samwise, &healthy_no_delete_request);
    assert_eq!(
        healthy_no_delete.err(),
        Some(Ok(Error::from_contract_error(1212)))
    );

    // validate liquidation can be deleted after restoring position
    let delete_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 41 * 10i128.pow(6),
        },
        Request {
            request_type: RequestType::DeleteLiquidationAuction as u32,
            address: Address::generate(&fixture.env),
            amount: i128::MAX,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 226 * SCALAR_7,
        },
    ];
    let sam_positions = pool_fixture
        .pool
        .submit(&samwise, &samwise, &samwise, &delete_request);
    // fuzz assert wide to account for b and d rates (only verify actions occurred)
    assert_approx_eq_abs(
        sam_positions.collateral.get_unchecked(stable_pool_index),
        1041 * 10i128.pow(6),
        10000,
    );
    assert_approx_eq_abs(
        sam_positions.liabilities.get_unchecked(xlm_pool_index),
        5849 * SCALAR_7,
        SCALAR_7,
    );
    assert!(pool_fixture.pool.try_get_auction(&0, &samwise).is_err());
}

#[test]
fn test_stale_liquidation_deletion() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];

    // Create a user
    let samwise = Address::generate(&fixture.env);

    // Use the retained cross-reserve liquidation fixture: STABLE collateral,
    // XLM debt. A same-reserve price change cannot change their value ratio.
    fixture.tokens[TokenIndex::STABLE].mint(&samwise, &(1000 * 10i128.pow(6)));
    let setup_request: Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 1000 * 10i128.pow(6),
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 6075 * SCALAR_7,
        },
    ];
    pool_fixture
        .pool
        .submit(&samwise, &samwise, &samwise, &setup_request);

    fixture.jump(60 * 60 * 24 * 14);

    // Raise the debt asset's price to cross the liquidation threshold.
    fixture.oracle.set_price_stable(&vec![
        &fixture.env,
        2000_0000000,
        1_0000000,
        0_1200000,
        1_0000000,
    ]);
    pool_fixture.pool.new_auction(
        &0u32,
        &samwise,
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::XLM].address.clone(),
        ],
        &vec![
            &fixture.env,
            fixture.tokens[TokenIndex::STABLE].address.clone(),
        ],
        &50u32,
    );

    // skip 500 blocks (499 past start of auction)
    fixture.jump_with_sequence(500 * 5);

    // validate the auction can't be deleted
    let early_delete = pool_fixture.pool.try_del_auction(&0u32, &samwise);
    assert_eq!(
        early_delete.err(),
        Some(Ok(Error::from_contract_error(1200)))
    );

    let auction = pool_fixture.pool.get_auction(&0u32, &samwise);
    assert_eq!(auction.bid.len(), 1);
    assert_eq!(auction.lot.len(), 1);

    // skip 1 more block
    fixture.jump_with_sequence(5);

    // delete the auction
    pool_fixture.pool.del_auction(&0u32, &samwise);
    assert!(fixture.env.auths().is_empty());
    let event = vec![&fixture.env, fixture.env.events().all().last_unchecked()];
    assert_eq!(
        event,
        vec![
            &fixture.env,
            (
                pool_fixture.pool.address.clone(),
                (
                    Symbol::new(&fixture.env, "delete_auction"),
                    0u32,
                    samwise.clone()
                )
                    .into_val(&fixture.env),
                ().into_val(&fixture.env)
            )
        ]
    );

    let auction = pool_fixture.pool.try_get_auction(&0u32, &samwise);
    assert!(auction.is_err());
}

#[test]
fn test_bad_debt() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];
    let stable_pool_index = pool_fixture.reserves[&TokenIndex::STABLE];
    let stable = &fixture.tokens[TokenIndex::STABLE];
    let xlm = &fixture.tokens[TokenIndex::XLM];
    let stable_scalar: i128 = 10i128.pow(stable.decimals());

    let sam = Address::generate(&fixture.env);
    // ***** Test bad debt can be invoked for user with no collateral *****
    let sam_stable_debt = 1_000 * stable_scalar;
    let sam_xlm_collateral = 15_000 * SCALAR_7;
    xlm.mint(&sam, &sam_xlm_collateral);
    let mut sam_positions = pool_fixture.pool.submit(
        &sam,
        &sam,
        &sam,
        &vec![
            &fixture.env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: xlm.address.clone(),
                amount: sam_xlm_collateral,
            },
            Request {
                request_type: RequestType::Borrow as u32,
                address: stable.address.clone(),
                amount: sam_stable_debt,
            },
        ],
    );

    fixture.jump_with_sequence(100);

    // Validate bad debt can't clear a user's liabilities if they have collateral
    let bad_debt_result_1 = pool_fixture.pool.try_bad_debt(&sam);
    assert_eq!(
        bad_debt_result_1.err(),
        Some(Ok(Error::from_contract_error(1200)))
    );

    // use magic to delete Sam's collateral
    fixture.env.as_contract(&pool_fixture.pool.address, || {
        let key = PoolDataKey::Positions(sam.clone());
        sam_positions.collateral = map![&fixture.env];
        fixture.env.storage().persistent().set(&key, &sam_positions);
    });

    // Validate invalid liquidaiton can't be created with no bid
    let result_sam_liquidation = pool_fixture.pool.try_new_auction(
        &0,
        &sam,
        &vec![&fixture.env, stable.address.clone()],
        &vec![&fixture.env, xlm.address.clone()],
        &100,
    );
    assert!(result_sam_liquidation.is_err());

    // Use bad debt to clear the position: the fork defaults the unpaid book
    // straight to suppliers via b-rate writeback rather than assigning
    // custody of the debt to the backstop.
    let sam_liab_pre_default = pool_fixture
        .pool
        .get_positions(&sam)
        .liabilities
        .get_unchecked(stable_pool_index);
    let pre_default_sam = pool_fixture.pool.get_reserve(&stable.address);
    pool_fixture.pool.bad_debt(&sam);
    let events = fixture.env.events().all();

    let sam_position_post = pool_fixture.pool.get_positions(&sam);
    assert_eq!(sam_position_post.collateral.len(), 0);
    assert_eq!(sam_position_post.liabilities.len(), 0);
    let backstop_post_bd_1 = pool_fixture.pool.get_positions(&fixture.backstop.address);
    assert_eq!(backstop_post_bd_1.collateral.len(), 0);
    assert_eq!(backstop_post_bd_1.liabilities.len(), 0);
    let post_default_sam = pool_fixture.pool.get_reserve(&stable.address);
    assert_eq!(
        post_default_sam.data.d_supply,
        pre_default_sam.data.d_supply - sam_liab_pre_default
    );
    assert_eq!(
        post_default_sam.data.b_supply,
        pre_default_sam.data.b_supply
    );
    assert!(post_default_sam.data.b_rate < pre_default_sam.data.b_rate);
    assert_approx_eq_abs(
        pre_default_sam.total_supply(&fixture.env) - post_default_sam.total_supply(&fixture.env),
        post_default_sam.to_asset_from_d_token(&fixture.env, sam_liab_pre_default),
        0_0000100,
    );
    let event = vec![&fixture.env, events.get_unchecked(events.len() - 1)];
    assert_eq!(
        event,
        vec![
            &fixture.env,
            (
                pool_fixture.pool.address.clone(),
                (
                    Symbol::new(&fixture.env, "defaulted_debt"),
                    stable.address.clone()
                )
                    .into_val(&fixture.env),
                sam_liab_pre_default.into_val(&fixture.env)
            )
        ]
    );
}
