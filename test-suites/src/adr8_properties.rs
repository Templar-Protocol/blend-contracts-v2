//! Bounded public-client replay for the external ADR0008 property runner.
//! Every sequence registers the compile-time optimized Wasm, never native contracts.
use crate::{
    create_fixture_with_data,
    test_fixture::{TestFixture, TokenIndex, SCALAR_7},
};
use pool::{PoolState, PositionData, Request, RequestType, ReserveEmissionMetadata};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    vec,
    xdr::{ScErrorCode, ScErrorType, ToXdr},
    Address, Error, IntoVal, InvokeError, Symbol, Val,
};

const ASSETS: [TokenIndex; 3] = [TokenIndex::STABLE, TokenIndex::WETH, TokenIndex::XLM];

/// Independent integer oracle; never invoke production settlement/conversion helpers.
/// Returns (remaining ordinary supply, residual defaulted d-tokens).
fn model_default(data: &mut pool::ReserveData, debt: i128, claim: i128) -> (i128, i128) {
    let scale = 1_000_000_000_000;
    let mut remaining = claim;
    let mut defaulted = debt;
    if debt > 0 && claim > 0 && data.b_rate > 0 {
        let assets = (debt * data.d_rate + scale - 1) / scale;
        let burned = claim.min((assets * scale + data.b_rate - 1) / data.b_rate);
        let covered = burned * data.b_rate / scale;
        let repaid = debt.min(covered * scale / data.d_rate);
        if repaid > 0 {
            remaining -= burned;
            data.b_supply -= burned;
            defaulted -= repaid;
        }
    }
    data.d_supply -= debt;
    if defaulted > 0 && data.b_supply > 0 {
        let assets = (defaulted * data.d_rate + scale - 1) / scale;
        let loss = (assets * scale + data.b_supply - 1) / data.b_supply;
        data.b_rate = (data.b_rate - loss).max(0);
    }
    (remaining, defaulted)
}

fn settlement_cash(f: &TestFixture<'_>) -> std::vec::Vec<i128> {
    let mut cash = std::vec::Vec::new();
    for token in f.tokens.iter() {
        for user in f.users.iter() {
            cash.push(token.balance(user));
        }
        cash.push(token.balance(&f.pools[0].pool.address));
        cash.push(token.balance(&f.backstop.address));
    }
    for user in f.users.iter() {
        cash.push(f.lp.balance(user));
    }
    cash.push(f.lp.balance(&f.pools[0].pool.address));
    cash.push(f.lp.balance(&f.backstop.address));
    cash
}

fn auction_exists(f: &TestFixture<'_>, user: &Address) -> bool {
    let e = &f.env;
    let key: (Symbol, soroban_sdk::Map<Symbol, Val>) = (
        Symbol::new(e, "Auction"),
        soroban_sdk::map![
            e,
            (Symbol::new(e, "auct_type"), 0u32.into_val(e)),
            (Symbol::new(e, "user"), user.clone().into_val(e)),
        ],
    );
    e.as_contract(&f.pools[0].pool.address, || {
        e.storage().temporary().has(&key)
    })
}

fn backstop_balance(f: &TestFixture<'_>) -> backstop::PoolBalance {
    f.env.as_contract(&f.backstop.address, || {
        f.env
            .storage()
            .persistent()
            .get(&backstop::BackstopDataKey::PoolBalance(
                f.pools[0].pool.address.clone(),
            ))
            .unwrap()
    })
}

#[track_caller]
fn rejected<T, E>(
    f: &TestFixture<'_>,
    code: u32,
    call: impl FnOnce() -> Result<Result<T, E>, Result<Error, InvokeError>>,
) {
    let before = f.env.to_ledger_snapshot();
    let pre_events = f.env.to_snapshot().events.0.len();
    assert_eq!(
        call().err(),
        Some(Ok(Error::from_contract_error(code))),
        // SDK snapshots omit diagnostics and Logs::all omits error topics.
        "rejection diagnostics: {:?}",
        f.env.host().get_diagnostic_events()
    );
    assert_eq!(
        f.env.to_ledger_snapshot(),
        before,
        "failed transaction changed ledger or TTL"
    );
    assert_no_new_committed_events(f, pre_events, "failed transaction committed events");
}

/// Contract events appended after `pre_len` may exist only as failed-call
/// diagnostic copies; rollback never emits one for real.
fn assert_no_new_committed_events(f: &TestFixture<'_>, pre_len: usize, context: &str) {
    let post = f.env.to_snapshot().events;
    assert!(
        post.0[pre_len..].iter().all(|event| {
            event.failed_call || event.event.type_ != soroban_sdk::xdr::ContractEventType::Contract
        }),
        "{context}"
    );
}

/// All six disabled entrypoints reject exactly 1000 with unchanged ledger and
/// zero new events; empty reward zone holds after each trap batch.
fn traps(f: &TestFixture<'_>, user: &Address) {
    let e = &f.env;
    let p = &f.pools[0].pool.address;
    e.mock_auths(&[]);
    let before = e.to_ledger_snapshot();
    macro_rules! trap {
        ($call:expr) => {{
            assert_eq!($call.err(), Some(Ok(Error::from_contract_error(1000))));
            assert_eq!(
                e.to_ledger_snapshot(),
                before,
                "entry trap changed ledger/TTL"
            );
            assert!(e.events().all().is_empty(), "entry trap emitted events");
            assert!(e.auths().is_empty(), "entry trap recorded authorization");
        }};
    }
    trap!(f.backstop.try_distribute());
    trap!(f.backstop.try_gulp_emissions(p));
    trap!(f.backstop.try_add_reward(p, &None));
    trap!(f.backstop.try_remove_reward(p));
    trap!(f.backstop.try_claim(user, &vec![e, p.clone()], &0));
    trap!(f.backstop.try_drop());
    assert!(f.backstop.reward_zone().is_empty());
    e.mock_all_auths();
}

fn request(
    f: &TestFixture<'_>,
    user: &Address,
    kind: RequestType,
    asset: TokenIndex,
    amount: i128,
) {
    f.pools[0].pool.submit(
        user,
        user,
        user,
        &vec![
            &f.env,
            Request {
                request_type: kind as u32,
                address: f.tokens[asset].address.clone(),
                amount,
            },
        ],
    );
}

fn balances(f: &TestFixture<'_>) -> std::vec::Vec<i128> {
    let addresses: std::vec::Vec<_> = f
        .users
        .iter()
        .chain([
            &f.bombadil,
            &f.pools[0].pool.address,
            &f.backstop.address,
            &f.lp.address,
            &f.emitter.address,
            &f.pool_factory.address,
            &f.oracle.address,
        ])
        .chain(f.tokens.iter().map(|t| &t.address))
        .collect();
    f.tokens
        .iter()
        .map(|t| addresses.iter().map(|a| t.balance(a)).sum::<i128>())
        .chain(std::iter::once(
            addresses.iter().map(|a| f.lp.balance(a)).sum::<i128>(),
        ))
        .collect()
}
fn invariants(
    f: &TestFixture<'_>,
    cash: &[i128],
    foreign_blnd: i128,
    seeded: &[u32],
    seed_eps: u64,
) {
    let pool = &f.pools[0].pool;
    assert_eq!(f.read_pool_config(0).bstop_rate, 0);
    let emissions: Option<soroban_sdk::Map<u32, u64>> = f.env.as_contract(&pool.address, || {
        f.env
            .storage()
            .persistent()
            .get(&Symbol::new(&f.env, "PoolEmis"))
    });
    assert!(
        emissions.is_none(),
        "pool emissions map must never exist: set_emissions_config is trapped"
    );
    assert_eq!(balances(f), cash, "token/LP conservation");
    assert_eq!(
        f.tokens[TokenIndex::BLND].balance(&f.backstop.address),
        foreign_blnd
    );
    let custody = pool.get_positions(&pool.address);
    assert!(custody.collateral.is_empty());
    assert!(custody.liabilities.is_empty());
    let backstop_positions = pool.get_positions(&f.backstop.address);
    assert!(backstop_positions.liabilities.is_empty());
    assert!(backstop_positions.collateral.is_empty());
    assert!(backstop_positions.supply.is_empty());
    for asset in ASSETS {
        let reserve = f.read_reserve_data(0, asset);
        let index = f.pools[0].reserves[&asset];
        let mut supplied = custody.supply.get(index).unwrap_or(0);
        let mut owed = 0;
        for user in &f.users {
            let p = pool.get_positions(user);
            supplied += p.supply.get(index).unwrap_or(0) + p.collateral.get(index).unwrap_or(0);
            owed += p.liabilities.get(index).unwrap_or(0);
        }
        assert_eq!(
            reserve.b_supply, supplied,
            "b-token ownership including orphan custody"
        );
        assert_eq!(reserve.d_supply, owed, "debt-token ownership");
        assert!(reserve.b_rate >= 0);
        assert!(reserve.d_rate > 0);
        assert_eq!(reserve.backstop_credit, 0);
        for id in [index * 2, index * 2 + 1] {
            let emission = pool.get_reserve_emissions(&id);
            if seeded.contains(&id) {
                let emission = emission.unwrap();
                assert_eq!(
                    (
                        emission.eps,
                        emission.index,
                        emission.expiration,
                        emission.last_time
                    ),
                    (seed_eps, 0, 0, 0)
                );
            } else {
                assert!(emission.is_none());
            }
            for user in f.users.iter().chain([&pool.address]) {
                if let Some(emission) = pool.get_user_emissions(user, &id) {
                    assert!(seeded.contains(&id));
                    assert_eq!((emission.index, emission.accrued), (0, 0));
                }
            }
        }
    }
    let mut shares = 0;
    let mut queued = 0;
    for user in &f.users {
        let balance = f.backstop.user_balance(&pool.address, user);
        shares += balance.shares;
        queued += balance.q4w.iter().map(|q| q.amount).sum::<i128>();
    }
    let balance = backstop_balance(f);
    assert_eq!(balance.shares, shares + queued);
    assert_eq!(balance.q4w, queued);
    assert_eq!(balance.tokens, f.lp.balance(&f.backstop.address));
    assert!(f.backstop.reward_zone().is_empty());
}

/// Expected exact outcome for withdrawing `amount` shares under the live
/// queue: `Ok(tokens_out)` when executable, otherwise the exact error code.
fn expect_withdraw(f: &TestFixture<'_>, user: &Address, amount: i128) -> Result<i128, u32> {
    let e = &f.env;
    let mut remaining = amount;
    let mut code = None;
    let ts = e.ledger().timestamp();
    for q in f
        .backstop
        .user_balance(&f.pools[0].pool.address, user)
        .q4w
        .iter()
    {
        if q.exp > ts {
            code = Some(1001);
            break;
        }
        if q.amount >= remaining {
            remaining = 0;
            break;
        }
        remaining -= q.amount;
    }

    if code.is_none() && remaining > 0 {
        code = Some(10);
    }
    if code.is_some() {
        return Err(code.unwrap());
    }
    let out = backstop_balance(&f).convert_to_tokens(amount);
    if out == 0 {
        Err(1006)
    } else {
        Ok(out)
    }
}

fn prepare(liquidation: bool, supply_selector: u64) -> TestFixture<'static> {
    let mut f = create_fixture_with_data(true);
    for _ in 0..3 {
        f.users.push(Address::generate(&f.env));
    }
    let e = &f.env;
    f.tokens[TokenIndex::BLND].mint(&f.users[0], &(1000 * SCALAR_7));
    let pool = &f.pools[0].pool;
    for user in &f.users[1..] {
        f.lp.transfer(&f.users[0], user, &(1000 * SCALAR_7));
        f.tokens[TokenIndex::BLND].mint(user, &(1000 * SCALAR_7));
    }
    if liquidation {
        // Verbatim vectors from supplier_loss_reverts_final_fill_then_independent_filler_succeeds.
        f.oracle
            .set_price_stable(&vec![e, 1_0000000, 1_0000000, 1_000000, 1_0000000]);
        f.tokens[TokenIndex::STABLE].mint(&f.users[3], &(5000 * 1_000000));
        request(
            &f,
            &f.users[3],
            RequestType::SupplyCollateral,
            TokenIndex::STABLE,
            5000 * 1_000000,
        );
        f.tokens[TokenIndex::XLM].mint(&f.users[1], &(100_000 * 10_0000000));
        f.tokens[TokenIndex::WETH].mint(&f.users[1], &2);
        pool.submit(
            &f.users[1],
            &f.users[1],
            &f.users[1],
            &vec![
                e,
                Request {
                    request_type: 2,
                    address: f.tokens[TokenIndex::XLM].address.clone(),
                    amount: 100_000 * 10_0000000,
                },
                Request {
                    request_type: 2,
                    address: f.tokens[TokenIndex::WETH].address.clone(),
                    amount: 2,
                },
                Request {
                    request_type: 4,
                    address: f.tokens[TokenIndex::STABLE].address.clone(),
                    amount: 5_000 * 1_000000,
                },
            ],
        );
        f.tokens[TokenIndex::STABLE].mint(&f.users[2], &(7 * 1_000000));
        pool.submit(
            &f.users[2],
            &f.users[2],
            &f.users[2],
            &vec![
                e,
                Request {
                    request_type: 2,
                    address: f.tokens[TokenIndex::STABLE].address.clone(),
                    amount: 7 * 1_000000,
                },
                Request {
                    request_type: 4,
                    address: f.tokens[TokenIndex::WETH].address.clone(),
                    amount: 4 * 1_000000000,
                },
            ],
        );
        // Seed ordinary supply before the auction blocks victim Supply requests.
        // Selectors 0/3 retain zero claims, 1/4 partially cover debt, 2/5 fully cover it.
        let coverage = supply_selector % 3;
        let stable = &f.tokens[TokenIndex::STABLE];
        let reserve = pool.get_reserve(&stable.address);
        let debt = pool
            .get_positions(&f.users[1])
            .liabilities
            .get_unchecked(reserve.config.index);
        let debt_assets = reserve.to_asset_from_d_token(e, debt);
        if coverage != 0 {
            let funding = if coverage == 1 {
                debt_assets / 2
            } else {
                // Minting floors b-tokens: round funding above the required claim's
                // asset value, not merely to the nominal borrowed asset amount.
                reserve.to_asset_from_b_token(e, reserve.to_b_token_up(e, debt_assets)) + 1
            };
            stable.mint(&f.users[1], &funding);
            request(
                &f,
                &f.users[1],
                RequestType::Supply,
                TokenIndex::STABLE,
                funding,
            );
        }
        let mut reserve = pool.get_reserve(&stable.address);
        let position = pool.get_positions(&f.users[1]);
        let debt = position.liabilities.get_unchecked(reserve.config.index);
        let claim = position.supply.get(reserve.config.index).unwrap_or(0);
        let (_, defaulted) = model_default(&mut reserve.data, debt, claim);
        assert!(debt > 0);
        match coverage {
            0 => assert!(claim == 0 && defaulted == debt),
            1 => assert!(claim > 0 && defaulted > 0 && defaulted < debt),
            2 => assert!(claim > 0 && defaulted == 0),
            _ => unreachable!(),
        }
        // Verbatim zeroing swap immediately before auction creation.
        f.oracle
            .set_price_stable(&vec![e, 1_0000000, 1_0000000, 1, 1_0000000]);
        let auction = pool.new_auction(
            &0,
            &f.users[1],
            &vec![e, f.tokens[TokenIndex::STABLE].address.clone()],
            &vec![e, f.tokens[TokenIndex::XLM].address.clone()],
            &100,
        );
        assert!(
            auction
                .bid
                .get_unchecked(f.tokens[TokenIndex::STABLE].address.clone())
                > 0,
            "prep numerics diverged: liquidation auction created without positive bid"
        );
        // Keep the victim and exposed filler positions intact; wallet funding
        // allows generated ordinary operations without curing their health.
        for user in &f.users[1..] {
            f.tokens[TokenIndex::WETH].mint(user, &(100 * 1_000000000));
            f.tokens[TokenIndex::XLM].mint(user, &(10_000 * SCALAR_7));
            f.tokens[TokenIndex::STABLE].mint(user, &(100_000 * 1_000000));
        }
    } else {
        for (i, user) in f.users[1..].iter().enumerate() {
            f.tokens[TokenIndex::WETH].mint(user, &(10 * 1_000000000));
            f.tokens[TokenIndex::XLM].mint(user, &(10_000 * SCALAR_7));
            f.tokens[TokenIndex::STABLE].mint(user, &(10_000 * 1_000000));
            pool.submit(
                user,
                user,
                user,
                &vec![
                    e,
                    Request {
                        request_type: 2,
                        address: f.tokens[TokenIndex::WETH].address.clone(),
                        amount: 5_000_000_000,
                    },
                    Request {
                        request_type: 2,
                        address: f.tokens[TokenIndex::XLM].address.clone(),
                        amount: 1000,
                    },
                    Request {
                        request_type: 4,
                        address: f.tokens[TokenIndex::STABLE].address.clone(),
                        amount: 25_000_001 + i as i128,
                    },
                ],
            );
        }
        // Matching proven default-orphan scenario shape, healthy non-liquidatable book.
        f.oracle
            .set_price_stable(&vec![e, 1, 1_0000000, 1, 100_000000]);
    }
    for user in &f.users {
        f.lp.approve(
            user,
            &f.backstop.address,
            &i128::MAX,
            &(e.ledger().sequence() + 999_998),
        );
    }
    f
}

/// Replay primitive tuples `(operation, user, reserve, amount, time)`:
/// operation 0..=18, user 0..=3, reserve 0..=2, amount selector 0..=5,
/// time selector 0..=8. No implicit modulo normalization. First tuple user
/// parity selects prepared direct-default or final-fill lifecycle; amount % 3
/// selects zero/partial/full initial ordinary-supply coverage for final fills.
/// Amounts 4/5 also seed emissions, covering partial/full respectively.
/// Setup uses established mocked authorization; action auth trees remain observable.
pub fn replay_generated(operations: &[(u8, u8, u8, u64, u32)]) {
    assert!((1..=64).contains(&operations.len()));
    assert!(
        operations.iter().all(|&(op, user, reserve, amount, time)| {
            op < 19 && user < 4 && reserve < 3 && amount < 6 && time < 9
        }),
        "out-of-domain input: {operations:?}"
    );
    let liquidation = operations[0].1 % 2 == 1;
    let f = prepare(liquidation, operations[0].3);
    let e = &f.env;
    let pool = &f.pools[0].pool;
    let cash = balances(&f);
    let mut foreign_blnd = f.tokens[TokenIndex::BLND].balance(&f.backstop.address);
    // Expired records still contradict the fork's disabled b-token emissions:
    // selectors 4/5 cover both all-zero data and nonzero stored eps.
    let seed_eps = if operations[0].3 == 5 { 1 } else { 0 };
    let seeded = if operations[0].3 >= 4 {
        let id = f.pools[0].reserves[&ASSETS[operations[0].2 as usize]] * 2 + 1;
        e.as_contract(&pool.address, || {
            e.storage().persistent().set(
                &pool::PoolDataKey::EmisData(id),
                &pool::ReserveEmissionData {
                    expiration: 0,
                    eps: seed_eps,
                    index: 0,
                    last_time: 0,
                },
            );
        });
        std::vec![id]
    } else {
        std::vec::Vec::new()
    };
    traps(&f, &f.users[0]);
    for (prefix, &(op, user, reserve, amount, time)) in operations.iter().enumerate() {
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let user = &f.users[user as usize];
            let asset = ASSETS[reserve as usize];
            let token = &f.tokens[asset];
            let amount = [0i128, 1, 2, 17, 1_000000, 10_000000][amount as usize];
            // Stable oracle mode stamps the current time; these are accrual,
            // queue and day boundaries, not stale-price evidence.
            f.jump([0u64, 1, 299, 300, 301, 86399, 86400, 86401, 1814400][time as usize]);
            let before_cash = balances(&f);
            match op {
                0 => {
                    // Actual backstop share conversion decides the rounded-zero edge.
                    let b_pre = backstop_balance(&f);
                    if b_pre.convert_to_shares(amount) == 0 {
                        rejected(&f, 1005, || {
                            f.backstop.try_deposit(user, &pool.address, &amount)
                        });
                    } else {
                        assert_eq!(
                            f.backstop.deposit(user, &pool.address, &amount),
                            b_pre.convert_to_shares(amount)
                        );
                        assert_eq!(e.auths()[0].0, *user);
                        let b_post = backstop_balance(&f);
                        assert_eq!(b_post.tokens, b_pre.tokens + amount);
                        assert_eq!(
                            b_post.shares,
                            b_pre.shares + b_pre.convert_to_shares(amount)
                        );
                    }
                }
                1 => {
                    let b = f.backstop.user_balance(&pool.address, user);
                    if amount > b.shares {
                        rejected(&f, 10, || {
                            f.backstop
                                .try_queue_withdrawal(user, &pool.address, &amount)
                        });
                    } else if b.q4w.len() >= 20 {
                        rejected(&f, 1007, || {
                            f.backstop
                                .try_queue_withdrawal(user, &pool.address, &amount)
                        });
                    } else {
                        assert_eq!(
                            f.backstop
                                .queue_withdrawal(user, &pool.address, &amount)
                                .amount,
                            amount
                        );
                    }
                }
                2 => {
                    let b = f.backstop.user_balance(&pool.address, user);
                    let queued: i128 = b.q4w.iter().map(|q| q.amount).sum();
                    if amount > queued {
                        rejected(&f, 10, || {
                            f.backstop
                                .try_dequeue_withdrawal(user, &pool.address, &amount)
                        });
                    } else {
                        f.backstop.dequeue_withdrawal(user, &pool.address, &amount);
                    }
                }
                3 => match expect_withdraw(&f, user, amount) {
                    Err(code) => rejected(&f, code, || {
                        f.backstop.try_withdraw(user, &pool.address, &amount)
                    }),
                    Ok(expected) => {
                        assert_eq!(f.backstop.withdraw(user, &pool.address, &amount), expected)
                    }
                },
                4 => {
                    // Synthetic-authority body control: mocked setup signs the
                    // donor success path only; op18 exercises the real
                    // fail-path distinction for this same operation.
                    f.backstop.donate(user, &pool.address, &amount);
                    assert_eq!(e.auths()[0].0, *user);
                }
                5 => {
                    // Insufficient liquidity edge draws the exact overdraw rejection.
                    let b = backstop_balance(&f);
                    if amount > b.tokens {
                        rejected(&f, 1003, || {
                            f.backstop.try_draw(&pool.address, &amount, user)
                        });
                    } else {
                        // Synthetic-authority body control: with mocked
                        // authorization these recording checks observe only
                        // the pool-signed success branch of production draw.
                        let backstop_before = f.lp.balance(&f.backstop.address);
                        let user_before = f.lp.balance(user);
                        f.backstop.draw(&pool.address, &amount, user);
                        assert_eq!(e.auths()[0].0, pool.address);
                        assert_eq!(backstop_balance(&f).tokens, b.tokens - amount);
                        assert_eq!(f.lp.balance(&f.backstop.address), backstop_before - amount);
                        assert_eq!(f.lp.balance(user), user_before + amount);
                    }
                }
                6 => {
                    f.tokens[TokenIndex::BLND].transfer(user, &f.backstop.address, &amount);
                    foreign_blnd += amount;
                }
                7 => {
                    let status = f.read_pool_config(0).status;
                    if status == 4 {
                        rejected(&f, 1204, || pool.try_update_status());
                    } else {
                        assert!(pool.update_status() <= 3);
                    }
                }
                8 => {
                    pool.set_status(&4);
                    assert_eq!(f.read_pool_config(0).status, 4);
                }
                9 => {
                    if f.read_pool_config(0).status == 4 {
                        rejected(&f, 1204, || pool.try_set_status(&3));
                    } else {
                        pool.set_status(&3);
                        assert_eq!(f.read_pool_config(0).status, 3);
                    }
                }
                10 => {
                    // Eligibility retains the fixture's valuation semantics;
                    // settlement amounts are modeled independently below.
                    let positions = pool.get_positions(user);
                    let data = e.as_contract(&pool.address, || {
                        let mut state = PoolState::load(e);
                        PositionData::calculate_from_positions(e, &mut state, &positions)
                    });
                    let auction = auction_exists(&f, user);
                    let eligible = !positions.liabilities.is_empty() && data.collateral_raw == 0;
                    let mut expected_positions = positions.clone();
                    let mut expected_custody = pool.get_positions(&pool.address);
                    let mut expected_defaults = soroban_sdk::Map::<u32, i128>::new(e);
                    let expected_reserves: std::vec::Vec<_> = ASSETS
                        .iter()
                        .map(|asset| {
                            let mut reserve = pool.get_reserve(&f.tokens[*asset].address);
                            let index = reserve.config.index;
                            if eligible {
                                let claim = positions.supply.get(index).unwrap_or(0);
                                let debt = positions.liabilities.get(index).unwrap_or(0);
                                let (remaining, defaulted) =
                                    model_default(&mut reserve.data, debt, claim);
                                if remaining == 0 {
                                    expected_positions.supply.remove(index);
                                } else {
                                    expected_positions.supply.set(index, remaining);
                                }
                                expected_defaults.set(index, defaulted);
                            }
                            (*asset, reserve)
                        })
                        .collect();
                    let has_default = expected_defaults.iter().any(|(_, amount)| amount > 0);
                    let blocked_emissions = has_default
                        && positions
                            .collateral
                            .iter()
                            .any(|(index, amount)| amount > 0 && seeded.contains(&(index * 2 + 1)));
                    if auction {
                        rejected(&f, 1212, || pool.try_bad_debt(user));
                    } else if !eligible || blocked_emissions {
                        rejected(&f, 1200, || pool.try_bad_debt(user));
                    } else {
                        let mut expected_events =
                            soroban_sdk::Vec::<(Address, soroban_sdk::Vec<Val>, Val)>::new(e);
                        for (index, _) in positions.liabilities.iter() {
                            expected_positions.liabilities.remove(index);
                            let defaulted = expected_defaults.get(index).unwrap_or(0);
                            if defaulted > 0 {
                                let asset = expected_reserves
                                    .iter()
                                    .find(|(_, reserve)| reserve.config.index == index)
                                    .unwrap()
                                    .1
                                    .asset
                                    .clone();
                                expected_events.push_back((
                                    pool.address.clone(),
                                    (Symbol::new(e, "defaulted_debt"), asset).into_val(e),
                                    defaulted.into_val(e),
                                ));
                            }
                        }
                        if has_default {
                            for (index, amount) in positions.collateral.iter() {
                                if amount > 0 {
                                    expected_positions.collateral.remove(index);
                                    expected_custody.supply.set(
                                        index,
                                        expected_custody.supply.get(index).unwrap_or(0) + amount,
                                    );
                                    let asset = expected_reserves
                                        .iter()
                                        .find(|(_, reserve)| reserve.config.index == index)
                                        .unwrap()
                                        .1
                                        .asset
                                        .clone();
                                    expected_events.push_back((
                                        pool.address.clone(),
                                        (
                                            Symbol::new(e, "collateral_orphaned"),
                                            user.clone(),
                                            asset,
                                        )
                                            .into_val(e),
                                        amount.into_val(e),
                                    ));
                                }
                            }
                        }
                        let cash_before = settlement_cash(&f);
                        let pre_events = e.events().all().len() as usize;
                        pool.bad_debt(user);
                        let actual_events: std::vec::Vec<_> =
                            e.events().all().iter().skip(pre_events).collect();
                        let wanted_events: std::vec::Vec<_> = expected_events.iter().collect();
                        assert_eq!(actual_events.len(), wanted_events.len());
                        for (actual, expected) in actual_events.into_iter().zip(wanted_events) {
                            assert_eq!(actual.to_xdr(e), expected.to_xdr(e));
                        }
                        assert_eq!(
                            pool.get_positions(user).to_xdr(e),
                            expected_positions.to_xdr(e)
                        );
                        assert_eq!(
                            pool.get_positions(&pool.address).to_xdr(e),
                            expected_custody.to_xdr(e)
                        );
                        for (asset, expected) in expected_reserves {
                            assert_eq!(
                                pool.get_reserve(&f.tokens[asset].address).data.to_xdr(e),
                                expected.data.to_xdr(e)
                            );
                        }
                        assert_eq!(settlement_cash(&f), cash_before);
                        assert_eq!(balances(&f), before_cash);
                    }
                }
                11 => {
                    let id = f.pools[0].reserves[&asset];
                    let pre = f.read_reserve_data(0, asset);
                    let custody = pool
                        .get_positions(&pool.address)
                        .supply
                        .get(id)
                        .unwrap_or(0);
                    let contradictory = seeded.contains(&(id * 2 + 1));
                    if pre.d_supply > 0 || contradictory {
                        rejected(&f, 1200, || pool.try_gulp(&token.address));
                    } else {
                        assert_eq!(pool.gulp(&token.address), 0);
                        let post = f.read_reserve_data(0, asset);
                        assert_eq!(post.b_supply, pre.b_supply - custody);
                        assert_eq!(post.b_rate, pre.b_rate);
                        assert_eq!(post.d_supply, 0);
                        assert!(!pool.get_positions(&pool.address).supply.contains_key(id));
                        assert_eq!(balances(&f), before_cash);
                    }
                }
                12 => {
                    // Debt-zero transition uses the production rounded-up asset quote.
                    let debt = pool
                        .get_positions(user)
                        .liabilities
                        .get(f.pools[0].reserves[&asset])
                        .unwrap_or(0);
                    if debt == 0 {
                        rejected(&f, 1219, || {
                            pool.try_submit(
                                user,
                                user,
                                user,
                                &vec![
                                    e,
                                    Request {
                                        request_type: 5,
                                        address: token.address.clone(),
                                        amount: 1,
                                    },
                                ],
                            )
                        });
                    } else {
                        // The exact `Ok` case: submit a one-over rounded-up quote to hit
                        // the overpay refund branch at user.rs apply_repay.
                        let repay = pool
                            .get_reserve(&token.address)
                            .to_asset_from_d_token(e, debt)
                            + 1;
                        if auction_exists(&f, user) {
                            rejected(&f, 1212, || {
                                pool.try_submit(
                                    user,
                                    user,
                                    user,
                                    &vec![
                                        e,
                                        Request {
                                            request_type: 5,
                                            address: token.address.clone(),
                                            amount: repay,
                                        },
                                    ],
                                )
                            });
                        } else {
                            request(&f, user, RequestType::Repay, asset, repay);
                        }
                    }
                }
                13 => {
                    // The admin emissions route is trapped: no caller can create pool
                    // b-token emissions configuration. Nothing changes.
                    let id = f.pools[0].reserves[&asset];
                    rejected(&f, 1200, || {
                        pool.try_set_emissions_config(&vec![
                            e,
                            ReserveEmissionMetadata {
                                res_index: id,
                                res_type: 1,
                                share: 1_0000000,
                            },
                        ])
                    });
                    rejected(&f, 1000, || pool.try_gulp_emissions());
                    assert_eq!(pool.claim(user, &vec![e, id * 2 + 1], user), 0);
                }
                14 => {
                    // All three submit identities are individually protected from custody access.
                    let (from, spender, to) = match reserve {
                        0 => (&pool.address, user, user),
                        1 => (user, &pool.address, user),
                        _ => (user, user, &pool.address),
                    };
                    rejected(&f, 1200, || {
                        pool.try_submit(
                            from,
                            spender,
                            to,
                            &vec![
                                e,
                                Request {
                                    request_type: 1,
                                    address: token.address.clone(),
                                    amount,
                                },
                            ],
                        )
                    });
                }
                15 => {
                    // Matured full fills transfer the entire lot, no bid, then
                    // setoff default eligible residual debt before filler checks.
                    let victim = &f.users[1];
                    let has_auction = auction_exists(&f, victim);
                    // Every final-fill caller sees the same fully matured zero-bid auction.
                    if has_auction {
                        e.ledger().with_mut(|li| li.sequence_number += 401);
                    }
                    if *user == *victim {
                        // Self-fill is rejected before auction existence is
                        // even consulted.
                        rejected(&f, 1211, || {
                            pool.try_submit(
                                user,
                                user,
                                user,
                                &vec![
                                    e,
                                    Request {
                                        request_type: 6,
                                        address: victim.clone(),
                                        amount: 100,
                                    },
                                ],
                            )
                        });
                    } else if !has_auction {
                        // The status gate never covers request type 6, so a
                        // non-victim fill of a nonexistent auction reaches
                        // storage::get_auction's unwrap and traps, changing
                        // nothing.
                        let before = e.to_ledger_snapshot();
                        let pre_events = e.to_snapshot().events.0.len();
                        let res = pool.try_submit(
                            user,
                            user,
                            user,
                            &vec![
                                e,
                                Request {
                                    request_type: 6,
                                    address: victim.clone(),
                                    amount: 100,
                                },
                            ],
                        );
                        assert_eq!(
                            res.err(),
                            Some(Ok(Error::from_type_and_code(
                                ScErrorType::Context,
                                ScErrorCode::InvalidAction
                            ))),
                            "trap diagnostics: {:?}",
                            f.env.host().get_diagnostic_events()
                        );
                        assert_eq!(
                            e.to_ledger_snapshot(),
                            before,
                            "trapped fill changed ledger or TTL"
                        );
                        assert_no_new_committed_events(
                            &f,
                            pre_events,
                            "trapped fill committed events",
                        );
                    } else {
                        let auction = pool.get_auction(&0, victim);
                        // Every observed fill observes the unconditional +401
                        // jump: scale_auction's bid modifier is exactly zero.
                        assert!(e.ledger().sequence() - auction.block >= 400);
                        let mut residual = pool.get_positions(victim);
                        let mut filler = pool.get_positions(user);
                        let previous_count = filler.effective_count();
                        let mut custody = pool.get_positions(&pool.address);
                        let reserves: std::vec::Vec<_> = ASSETS
                            .iter()
                            .map(|a| (*a, pool.get_reserve(&f.tokens[*a].address)))
                            .collect();
                        for (asset, lot) in auction.lot.iter() {
                            let index = reserves
                                .iter()
                                .find(|(_, r)| r.asset == asset)
                                .expect("auction asset outside generated reserves")
                                .1
                                .config
                                .index;
                            let remaining = residual.collateral.get(index).unwrap_or(0) - lot;
                            assert!(remaining >= 0);
                            if remaining == 0 {
                                residual.collateral.remove(index);
                            } else {
                                residual.collateral.set(index, remaining);
                            }
                            filler
                                .collateral
                                .set(index, filler.collateral.get(index).unwrap_or(0) + lot);
                        }
                        let mut modeled = e.as_contract(&pool.address, || PoolState::load(e));
                        for (_, r) in &reserves {
                            modeled.cache_reserve(r.clone());
                        }
                        // Setoff defaults when the residual position has no raw
                        // collateral; modeled reserves drive that valuation.
                        let defaults = !residual.liabilities.is_empty()
                            && e.as_contract(&pool.address, || {
                                PositionData::calculate_from_positions(e, &mut modeled, &residual)
                                    .collateral_raw
                                    == 0
                            });
                        // Emission-gate discrimination happens on actual residual
                        // default below, after modeled arithmetic settles claims.
                        let mut expected_reserves = std::vec::Vec::new();
                        let mut expected_defaults = soroban_sdk::Map::<u32, i128>::new(e);
                        let mut expected_events =
                            soroban_sdk::Vec::<(Address, soroban_sdk::Vec<Val>, Val)>::new(e);
                        for (asset, r) in &reserves {
                            let mut expected = r.clone();
                            let index = r.config.index;
                            let debt = if defaults {
                                residual.liabilities.get(index).unwrap_or(0)
                            } else {
                                0
                            };
                            if debt > 0 {
                                let claim = residual.supply.get(index).unwrap_or(0);
                                let (remaining, defaulted) =
                                    model_default(&mut expected.data, debt, claim);
                                if remaining == 0 {
                                    residual.supply.remove(index);
                                } else {
                                    residual.supply.set(index, remaining);
                                }
                                expected_defaults.set(index, defaulted);
                            } else {
                                expected_defaults.set(index, 0);
                            }
                            modeled.cache_reserve(expected.clone());
                            expected_reserves.push((*asset, expected));
                        }
                        // Emission-gate discrimination uses ACTUAL residual
                        // defaults (any_default), not bare eligibility.
                        let any_default = expected_defaults.iter().any(|(_, amount)| amount > 0);
                        let blocked = any_default
                            && e.as_contract(&pool.address, || {
                                let emissions: soroban_sdk::Map<u32, u64> = e
                                    .storage()
                                    .persistent()
                                    .get(&Symbol::new(e, "PoolEmis"))
                                    .unwrap_or(soroban_sdk::Map::new(e));
                                residual.collateral.iter().any(|(index, amount)| {
                                    let id = index * 2 + 1;
                                    let user_key = (
                                        Symbol::new(e, "UserEmis"),
                                        soroban_sdk::Map::<Symbol, Val>::from_array(
                                            e,
                                            [
                                                (
                                                    Symbol::new(e, "user"),
                                                    pool.address.clone().into_val(e),
                                                ),
                                                (Symbol::new(e, "reserve_id"), id.into_val(e)),
                                            ],
                                        ),
                                    );
                                    amount > 0
                                        && (emissions.contains_key(id)
                                            || e.storage()
                                                .persistent()
                                                .has(&pool::PoolDataKey::EmisData(id))
                                            || e.storage().persistent().has(&user_key))
                                })
                            });
                        if any_default {
                            for (index, _) in residual.liabilities.iter() {
                                let defaulted = expected_defaults.get(index).unwrap_or(0);
                                if defaulted == 0 {
                                    continue;
                                }
                                let asset = &reserves
                                    .iter()
                                    .find(|(_, r)| r.config.index == index)
                                    .unwrap()
                                    .1
                                    .asset;
                                expected_events.push_back((
                                    pool.address.clone(),
                                    (Symbol::new(e, "defaulted_debt"), asset.clone()).into_val(e),
                                    defaulted.into_val(e),
                                ));
                            }
                            for (index, amount) in residual.collateral.iter() {
                                if amount > 0 {
                                    let (_, r) = reserves
                                        .iter()
                                        .find(|(_, r)| r.config.index == index)
                                        .expect("custody asset outside generated reserves");
                                    let asset = r.asset.clone();
                                    custody.supply.set(
                                        index,
                                        custody.supply.get(index).unwrap_or(0) + amount,
                                    );
                                    expected_events.push_back((
                                        pool.address.clone(),
                                        (
                                            Symbol::new(e, "collateral_orphaned"),
                                            victim.clone(),
                                            asset,
                                        )
                                            .into_val(e),
                                        amount.into_val(e),
                                    ));
                                }
                            }
                        }
                        if defaults {
                            // Eligible setoff clears every liability entry; each
                            // was repaid by the modeled claim burn or defaulted.
                            for asset in residual.liabilities.keys().iter() {
                                residual.liabilities.remove(asset);
                            }
                        }
                        if any_default {
                            // Remove over owned key snapshots: mutating the map
                            // mid live iteration could skip entries.
                            for asset in residual.collateral.keys().iter() {
                                residual.collateral.remove(asset);
                            }
                        }
                        let health = e.as_contract(&pool.address, || {
                            PositionData::calculate_from_positions(e, &mut modeled, &filler)
                        });
                        // Rejection precedence mirrors execute_submit validation:
                        // action-time custody panic precedes max positions, an
                        // active filler auction, health, and minimum collateral.
                        // ponytail: skipping 1207 max-util modeling; pure fills never add borrow requests.
                        let code = if blocked {
                            Some(1200)
                        } else if filler.effective_count() > previous_count
                            && filler.effective_count() > modeled.config.max_positions
                        {
                            Some(1208)
                        } else if auction_exists(&f, user) {
                            Some(1212)
                        } else if !filler.liabilities.is_empty() && health.is_hf_under(e, 1_0000100)
                        {
                            Some(1205)
                        } else if !filler.liabilities.is_empty()
                            && health.collateral_base < modeled.config.min_collateral
                        {
                            Some(1224)
                        } else {
                            None
                        };
                        let fill = vec![
                            e,
                            Request {
                                request_type: 6,
                                address: victim.clone(),
                                amount: 100,
                            },
                        ];
                        if let Some(code) = code {
                            rejected(&f, code, || pool.try_submit(user, user, user, &fill));
                            assert!(auction_exists(&f, victim));
                        } else {
                            // No SupplyCollateral/Borrow rounds precede the pure
                            // fill, so deposit-side d-token supply stays untouched
                            // aside from rm_positions' mirrored transfers above.
                            let mut filled = auction.clone();
                            for asset in auction.bid.keys().iter() {
                                filled.bid.remove(asset);
                            }
                            expected_events.push_back((
                                pool.address.clone(),
                                (Symbol::new(e, "fill_auction"), 0u32, victim.clone()).into_val(e),
                                (user.clone(), 100i128, filled).into_val(e),
                            ));
                            let pre_events = e.events().all().len() as usize;
                            pool.submit(user, user, user, &fill);
                            let post_events: std::vec::Vec<_> =
                                e.events().all().iter().skip(pre_events).collect();
                            let wanted_events: std::vec::Vec<_> = expected_events.iter().collect();
                            assert_eq!(post_events.len(), wanted_events.len());
                            for (actual, expected) in post_events.into_iter().zip(wanted_events) {
                                assert_eq!(actual.to_xdr(e), expected.to_xdr(e));
                            }
                            assert_eq!(pool.get_positions(victim).to_xdr(e), residual.to_xdr(e));
                            assert_eq!(pool.get_positions(user).to_xdr(e), filler.to_xdr(e));
                            assert_eq!(
                                pool.get_positions(&pool.address).to_xdr(e),
                                custody.to_xdr(e)
                            );
                            // Custody and lot flows are mirrored transfer pairs on the
                            // same reserve. A same-user setoff additionally burns the
                            // borrower's b-tokens before residual supplier loss.
                            for (asset, expected) in expected_reserves {
                                let postreserve = f.read_reserve_data(0, asset);
                                assert_eq!(postreserve.b_supply, expected.data.b_supply);
                                assert_eq!(postreserve.d_rate, expected.data.d_rate);
                                assert_eq!(postreserve.d_supply, expected.data.d_supply);
                                assert_eq!(postreserve.b_rate, expected.data.b_rate);
                            }
                            assert!(!auction_exists(&f, victim));
                            assert_eq!(balances(&f), before_cash);
                        }
                    }
                }
                16 => {
                    // Frozen/rounding supply boundaries through the retained public operation.
                    let r = pool.get_reserve(&token.address);
                    let requests = vec![
                        e,
                        Request {
                            request_type: 0,
                            address: token.address.clone(),
                            amount,
                        },
                    ];
                    if f.read_pool_config(0).status > 3 {
                        rejected(&f, 1206, || pool.try_submit(user, user, user, &requests));
                    } else if r.to_b_token_down(e, amount) == 0 {
                        rejected(&f, 1216, || pool.try_submit(user, user, user, &requests));
                    } else if auction_exists(&f, user) {
                        rejected(&f, 1212, || pool.try_submit(user, user, user, &requests));
                    } else {
                        pool.submit(user, user, user, &requests);
                    }
                }
                17 => {
                    // Getter equals custodial LP balance exactly.
                    assert_eq!(
                        f.backstop.pool_data(&pool.address).tokens,
                        f.lp.balance(&f.backstop.address)
                    );
                }
                18 => {
                    // Identical retained call and ledger; only caller authority changes.
                    let amount = amount.max(1);
                    let pre = backstop_balance(&f);
                    // Generated contract addresses have no __check_auth implementation;
                    // withholding mocked authorization fails that invocation.
                    e.mock_auths(&[]);
                    let before = e.to_ledger_snapshot();
                    assert_eq!(
                        f.backstop.try_donate(user, &pool.address, &amount).err(),
                        Some(Ok(Error::from_type_and_code(
                            ScErrorType::Context,
                            ScErrorCode::InvalidAction,
                        ))),
                    );
                    assert_eq!(e.to_ledger_snapshot(), before);
                    assert!(e.events().all().is_empty());
                    e.mock_all_auths();
                    f.backstop.donate(user, &pool.address, &amount);
                    assert_eq!(e.auths()[0].0, *user);
                    assert_eq!(backstop_balance(&f).tokens, pre.tokens + amount);
                }
                _ => unreachable!(),
            }
            invariants(&f, &cash, foreign_blnd, &seeded, seed_eps);
            traps(&f, user);
        }));
        if let Err(error) = run {
            eprintln!("ADR0008 Wasm failure prefix={prefix} liquidation={liquidation} complete_input={operations:?}");
            std::panic::resume_unwind(error);
        }
    }
}
