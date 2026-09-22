//! Public default, accumulated custody, rollback and terminal settlement.
use pool::{PoolDataKey, Request, RequestType};
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    vec,
    xdr::ToXdr,
    Address, Error, IntoVal, Symbol, Val, Vec,
};
use test_suites::{
    create_fixture_with_data,
    test_fixture::{TestFixture, TokenIndex, SCALAR_12, SCALAR_7},
};

fn dust_borrower(fixture: &TestFixture<'_>, debt: i128) -> (Address, pool::Positions) {
    let e = &fixture.env;
    let user = Address::generate(e);
    let weth = &fixture.tokens[TokenIndex::WETH];
    let xlm = &fixture.tokens[TokenIndex::XLM];
    weth.mint(&user, &50_000_000); // 0.05 WETH initially secures the loan.
    xlm.mint(&user, &1000);
    let positions = fixture.pools[0].pool.submit(
        &user,
        &user,
        &user,
        &vec![
            e,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: weth.address.clone(),
                amount: 50_000_000,
            },
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: xlm.address.clone(),
                amount: 1000,
            },
            Request {
                request_type: RequestType::Borrow as u32,
                address: fixture.tokens[TokenIndex::STABLE].address.clone(),
                amount: debt,
            },
            // WETH is both collateral and debt: reclassification must not
            // overwrite the loss already applied to this reserve's shared cache.
            Request {
                request_type: RequestType::Borrow as u32,
                address: weth.address.clone(),
                amount: 5_000_000,
            },
        ],
    );
    (user, positions)
}

#[test]
fn test_public_default_orphan_custody_parking() {
    for wasm in [false, true] {
        for status in 0..=6 {
            let fixture = create_fixture_with_data(wasm);
            let e = &fixture.env;
            let pool = &fixture.pools[0].pool;
            let (sam, positions) = dust_borrower(&fixture, 50_000_000);
            let weth = &fixture.tokens[TokenIndex::WETH];
            let stable_index = fixture.pools[0].reserves[&TokenIndex::STABLE];
            let debt = positions.liabilities.get_unchecked(stable_index);
            assert!(debt > 0);
            assert_eq!(positions.collateral.len(), 2);
            let (second, second_positions) = dust_borrower(&fixture, 25_000_000);
            let before: std::vec::Vec<_> = [TokenIndex::STABLE, TokenIndex::WETH, TokenIndex::XLM]
                .into_iter()
                .map(|index| (index, fixture.read_reserve_data(0, index)))
                .collect();
            let addresses = [
                sam.clone(),
                second.clone(),
                pool.address.clone(),
                fixture.backstop.address.clone(),
                fixture.users[0].clone(),
            ];
            let balances: std::vec::Vec<_> = fixture
                .tokens
                .iter()
                .map(|token| {
                    addresses
                        .iter()
                        .map(|address| token.balance(address))
                        .collect::<std::vec::Vec<_>>()
                })
                .collect();
            let backstop_positions = pool.get_positions(&fixture.backstop.address);
            // Both collateral amounts are below one asset unit. Minimum positive
            // seven-decimal prices make their independently floored raw values zero.
            fixture
                .oracle
                .set_price_stable(&vec![e, 1, 1_0000000, 1, 1_0000000]);
            pool.bad_debt(&sam);
            let emitted = e.events().all(); // Capture before any getter starts another invocation.
            let after = pool.get_positions(&sam);
            assert!(after.liabilities.is_empty());
            assert!(after.collateral.is_empty());
            assert_eq!(after.supply, positions.supply);
            let backstop_after = pool.get_positions(&fixture.backstop.address);
            assert_eq!(
                (
                    backstop_after.liabilities,
                    backstop_after.collateral,
                    backstop_after.supply
                ),
                (
                    backstop_positions.liabilities,
                    backstop_positions.collateral,
                    backstop_positions.supply
                )
            );
            let custody = pool.get_positions(&pool.address);
            assert!(custody.collateral.is_empty());
            assert!(custody.liabilities.is_empty());
            assert_eq!(custody.supply, positions.collateral);
            for (index, pre) in before {
                let post = fixture.read_reserve_data(0, index);
                assert_eq!(post.b_supply, pre.b_supply);
                assert_eq!(post.d_rate, pre.d_rate);
                let reserve_index = fixture.pools[0].reserves[&index];
                let defaulted = positions.liabilities.get(reserve_index).unwrap_or(0);
                assert_eq!(post.d_supply, pre.d_supply - defaulted);
                if defaulted > 0 {
                    let assets =
                        (defaulted * pre.d_rate + 1_000_000_000_000 - 1) / 1_000_000_000_000;
                    let rate_loss = (assets * 1_000_000_000_000 + pre.b_supply - 1) / pre.b_supply;
                    assert_eq!(post.b_rate, (pre.b_rate - rate_loss).max(0));
                } else {
                    assert_eq!(post.b_rate, pre.b_rate);
                }
            }
            for (token, before) in fixture.tokens.iter().zip(&balances) {
                for (address, balance) in addresses.iter().zip(before) {
                    assert_eq!(token.balance(address), *balance);
                }
            }
            let mut expected: Vec<(Address, Vec<Val>, Val)> = Vec::new(e);
            for (index, amount) in positions.liabilities.iter() {
                let asset = fixture.pools[0]
                    .reserves
                    .iter()
                    .find(|(_, reserve)| **reserve == index)
                    .unwrap()
                    .0;
                expected.push_back((
                    pool.address.clone(),
                    (
                        Symbol::new(e, "defaulted_debt"),
                        fixture.tokens[*asset].address.clone(),
                    )
                        .into_val(e),
                    amount.into_val(e),
                ));
            }
            for (index, amount) in positions.collateral.iter() {
                let asset = fixture.pools[0]
                    .reserves
                    .iter()
                    .find(|(_, reserve)| **reserve == index)
                    .unwrap()
                    .0;
                expected.push_back((
                    pool.address.clone(),
                    (
                        Symbol::new(e, "collateral_orphaned"),
                        sam.clone(),
                        fixture.tokens[*asset].address.clone(),
                    )
                        .into_val(e),
                    amount.into_val(e),
                ));
            }
            assert_eq!(emitted, expected);

            // A second independent default accumulates, rather than replacing,
            // custody in both reserves.
            let before_second: std::vec::Vec<_> = [TokenIndex::STABLE, TokenIndex::WETH]
                .into_iter()
                .map(|index| (index, fixture.read_reserve_data(0, index)))
                .collect();
            pool.bad_debt(&second);
            let second_events = e.events().all();
            expected = vec![e];
            for (index, amount) in second_positions.liabilities.iter() {
                let asset = fixture.pools[0]
                    .reserves
                    .iter()
                    .find(|(_, reserve)| **reserve == index)
                    .unwrap()
                    .0;
                expected.push_back((
                    pool.address.clone(),
                    (
                        Symbol::new(e, "defaulted_debt"),
                        fixture.tokens[*asset].address.clone(),
                    )
                        .into_val(e),
                    amount.into_val(e),
                ));
            }
            let mut accumulated = custody.supply.clone();
            for (index, amount) in second_positions.collateral.iter() {
                accumulated.set(index, accumulated.get_unchecked(index) + amount);
                let asset = fixture.pools[0]
                    .reserves
                    .iter()
                    .find(|(_, reserve)| **reserve == index)
                    .unwrap()
                    .0;
                expected.push_back((
                    pool.address.clone(),
                    (
                        Symbol::new(e, "collateral_orphaned"),
                        second.clone(),
                        fixture.tokens[*asset].address.clone(),
                    )
                        .into_val(e),
                    amount.into_val(e),
                ));
            }
            assert_eq!(second_events, expected);
            assert_eq!(pool.get_positions(&pool.address).supply, accumulated);
            assert!(pool.get_positions(&second).liabilities.is_empty());
            assert!(pool.get_positions(&second).collateral.is_empty());
            for (index, pre) in before_second {
                let post = fixture.read_reserve_data(0, index);
                let amount = second_positions
                    .liabilities
                    .get_unchecked(fixture.pools[0].reserves[&index]);
                let assets = (amount * pre.d_rate + 1_000_000_000_000 - 1) / 1_000_000_000_000;
                let rate_loss = (assets * 1_000_000_000_000 + pre.b_supply - 1) / pre.b_supply;
                assert_eq!(post.d_supply, pre.d_supply - amount);
                assert_eq!(post.b_supply, pre.b_supply);
                assert_eq!(post.b_rate, (pre.b_rate - rate_loss).max(0));
            }
            for (token, before) in fixture.tokens.iter().zip(&balances) {
                for (address, balance) in addresses.iter().zip(before) {
                    assert_eq!(token.balance(address), *balance);
                }
            }

            // None of the three submit identities can withdraw parked custody.
            for (from, spender, to) in [
                (&pool.address, &sam, &sam),
                (&sam, &pool.address, &sam),
                (&sam, &sam, &pool.address),
            ] {
                let snapshot = e.to_ledger_snapshot();
                assert_eq!(
                    pool.try_submit(
                        from,
                        spender,
                        to,
                        &vec![
                            e,
                            Request {
                                request_type: RequestType::Withdraw as u32,
                                address: weth.address.clone(),
                                amount: 1
                            },
                        ]
                    )
                    .err(),
                    Some(Ok(Error::from_contract_error(1200)))
                );
                assert_eq!(e.to_ledger_snapshot(), snapshot);
            }

            // Exercise every stored status independently; status 4 also takes the
            // real authenticated, absorbing freeze entry.
            if status == 4 {
                pool.set_status(&4);
            } else {
                e.as_contract(&pool.address, || {
                    let mut config: pool::PoolConfig = e
                        .storage()
                        .instance()
                        .get(&Symbol::new(e, "Config"))
                        .unwrap();
                    config.status = status;
                    e.storage()
                        .instance()
                        .set(&Symbol::new(e, "Config"), &config);
                });
            }
            assert_eq!(fixture.read_pool_config(0).status, status);
            for index in [TokenIndex::WETH, TokenIndex::XLM] {
                let token = &fixture.tokens[index];
                let reserve_index = fixture.pools[0].reserves[&index];
                let custodied = pool.get_positions(&pool.address);
                let orphaned = custodied.supply.get_unchecked(reserve_index);

                // Frodo still owes this reserve: custody cannot be settled early.
                assert!(fixture.read_reserve_data(0, index).d_supply > 0);
                let snapshot = e.to_ledger_snapshot();
                assert_eq!(
                    pool.try_gulp(&token.address).err(),
                    Some(Ok(Error::from_contract_error(1200)))
                );
                assert_eq!(e.to_ledger_snapshot(), snapshot);
                let frodo = &fixture.users[0];
                let liability = pool
                    .get_positions(frodo)
                    .liabilities
                    .get_unchecked(reserve_index);
                let repay = pool
                    .get_reserve(&token.address)
                    .to_asset_from_d_token(e, liability)
                    + 1;
                pool.submit(
                    frodo,
                    frodo,
                    frodo,
                    &vec![
                        e,
                        Request {
                            request_type: RequestType::Repay as u32,
                            address: token.address.clone(),
                            amount: repay,
                        },
                    ],
                );
                assert_eq!(fixture.read_reserve_data(0, index).d_supply, 0);

                // An unsolicited surplus remains unrecognized, even at settlement.
                token.mint(&pool.address, &17);
                let cash: std::vec::Vec<_> = fixture
                    .tokens
                    .iter()
                    .map(|token| {
                        addresses
                            .iter()
                            .map(|address| token.balance(address))
                            .collect::<std::vec::Vec<_>>()
                    })
                    .collect();
                let mut expected_reserve = fixture.read_reserve_data(0, index);
                expected_reserve.b_supply -= orphaned;
                assert_eq!(pool.gulp(&token.address), 0);
                let settlement_events = e.events().all();
                assert_eq!(
                    settlement_events,
                    vec![
                        e,
                        (
                            pool.address.clone(),
                            (Symbol::new(e, "orphan_settled"), token.address.clone()).into_val(e),
                            orphaned.into_val(e)
                        ),
                        (
                            pool.address.clone(),
                            (Symbol::new(e, "gulp"), token.address.clone()).into_val(e),
                            0i128.into_val(e)
                        ),
                    ]
                );
                assert_eq!(
                    fixture.read_reserve_data(0, index).to_xdr(e),
                    expected_reserve.to_xdr(e)
                );
                let mut expected_custody = custodied;
                expected_custody.supply.remove(reserve_index);
                assert_eq!(
                    pool.get_positions(&pool.address).to_xdr(e),
                    expected_custody.to_xdr(e)
                );
                for (token, before) in fixture.tokens.iter().zip(cash) {
                    for (address, balance) in addresses.iter().zip(before) {
                        assert_eq!(token.balance(address), balance);
                    }
                }

                // Repeating the terminal operation emits only the retained
                // underlying-unit gulp(asset, 0), and changes no ledger entry.
                let snapshot = e.to_ledger_snapshot();
                assert_eq!(pool.gulp(&token.address), 0);
                let repeated_events = e.events().all();
                assert_eq!(e.to_ledger_snapshot(), snapshot);
                assert_eq!(
                    repeated_events,
                    vec![
                        e,
                        (
                            pool.address.clone(),
                            (Symbol::new(e, "gulp"), token.address.clone()).into_val(e),
                            0i128.into_val(e)
                        ),
                    ]
                );
            }
            assert!(pool.get_positions(&pool.address).supply.is_empty());
        }
    }
}

#[test]
fn same_reserve_supply_is_locked_and_netted_before_default() {
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let e = &fixture.env;
        let pool = &fixture.pools[0].pool;
        let stable = &fixture.tokens[TokenIndex::STABLE];
        let weth = &fixture.tokens[TokenIndex::WETH];
        let borrower = Address::generate(e);
        let amount = 5 * 10i128.pow(6);

        stable.mint(&borrower, &amount);
        weth.mint(&borrower, &50_000_000);
        pool.submit(
            &borrower,
            &borrower,
            &borrower,
            &vec![
                e,
                Request {
                    request_type: RequestType::Supply as u32,
                    address: stable.address.clone(),
                    amount,
                },
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: weth.address.clone(),
                    amount: 50_000_000,
                },
                Request {
                    request_type: RequestType::Borrow as u32,
                    address: stable.address.clone(),
                    amount,
                },
            ],
        );
        fixture.jump(60 * 60);
        fixture
            .oracle
            .set_price_stable(&vec![e, 1, SCALAR_7, 0_1000000, SCALAR_7]);

        let stable_index = fixture.pools[0].reserves[&TokenIndex::STABLE];
        let positions_before = pool.get_positions(&borrower);
        let reserve_before = pool.get_reserve(&stable.address).data;
        let claim = positions_before.supply.get_unchecked(stable_index);
        let debt = positions_before.liabilities.get_unchecked(stable_index);
        let snapshot = e.to_ledger_snapshot();
        let withdraw = vec![
            e,
            Request {
                request_type: RequestType::Withdraw as u32,
                address: stable.address.clone(),
                amount,
            },
        ];
        assert_eq!(
            pool.try_submit(&borrower, &borrower, &borrower, &withdraw)
                .err(),
            Some(Ok(Error::from_contract_error(1205))),
            "wasm={wasm}"
        );
        assert_eq!(e.to_ledger_snapshot(), snapshot, "wasm={wasm}");

        let debt_assets = debt
            .fixed_mul_ceil(reserve_before.d_rate, SCALAR_12)
            .unwrap();
        let b_tokens = claim.min(
            debt_assets
                .fixed_div_ceil(reserve_before.b_rate, SCALAR_12)
                .unwrap(),
        );
        let covered_assets = b_tokens
            .fixed_mul_floor(reserve_before.b_rate, SCALAR_12)
            .unwrap();
        let repaid = debt.min(
            covered_assets
                .fixed_div_floor(reserve_before.d_rate, SCALAR_12)
                .unwrap(),
        );
        let defaulted = debt - repaid;
        assert!(defaulted > 0, "fixture must exercise partial coverage");
        let b_supply = reserve_before.b_supply - b_tokens;
        let loss = defaulted
            .fixed_mul_ceil(reserve_before.d_rate, SCALAR_12)
            .unwrap()
            .fixed_div_ceil(b_supply, SCALAR_12)
            .unwrap();
        let token_balances = (
            stable.balance(&borrower),
            stable.balance(&pool.address),
            weth.balance(&borrower),
            weth.balance(&pool.address),
        );

        pool.bad_debt(&borrower);

        let positions_after = pool.get_positions(&borrower);
        let reserve_after = pool.get_reserve(&stable.address).data;
        assert!(positions_after.liabilities.is_empty(), "wasm={wasm}");
        assert_eq!(
            positions_after.supply.get(stable_index).unwrap_or(0),
            claim - b_tokens,
            "wasm={wasm}"
        );
        assert_eq!(
            reserve_after.d_supply,
            reserve_before.d_supply - debt,
            "wasm={wasm}"
        );
        assert_eq!(reserve_after.b_supply, b_supply, "wasm={wasm}");
        assert_eq!(
            reserve_after.b_rate,
            reserve_before.b_rate - loss,
            "wasm={wasm}"
        );
        assert_eq!(
            (
                stable.balance(&borrower),
                stable.balance(&pool.address),
                weth.balance(&borrower),
                weth.balance(&pool.address),
            ),
            token_balances,
            "wasm={wasm}"
        );
    }
}

#[test]
fn contradictory_emissions_revert_public_default_and_settlement() {
    for wasm in [false, true] {
        for settlement in [false, true] {
            for record in ["pool-map", "reserve-data", "pool-user-data"] {
                let fixture = create_fixture_with_data(wasm);
                let e = &fixture.env;
                let pool = &fixture.pools[0].pool;
                let (sam, positions) = dust_borrower(&fixture, 50_000_000);
                let asset = &fixture.tokens[TokenIndex::WETH];
                let index = fixture.pools[0].reserves[&TokenIndex::WETH];
                // The contradiction is on the second collateral reserve,
                // after another reserve has already passed its precheck.
                assert_eq!(positions.collateral.keys().last(), Some(index));
                assert_eq!(positions.collateral.len(), 2);
                fixture
                    .oracle
                    .set_price_stable(&vec![e, 1, 1_0000000, 1, 1_0000000]);
                if settlement {
                    pool.bad_debt(&sam);
                    let frodo = &fixture.users[0];
                    let debt = pool.get_positions(frodo).liabilities.get_unchecked(index);
                    let repay = pool
                        .get_reserve(&asset.address)
                        .to_asset_from_d_token(e, debt)
                        + 1;
                    pool.submit(
                        frodo,
                        frodo,
                        frodo,
                        &vec![
                            e,
                            Request {
                                request_type: RequestType::Repay as u32,
                                address: asset.address.clone(),
                                amount: repay,
                            },
                        ],
                    );
                    assert_eq!(fixture.read_reserve_data(0, TokenIndex::WETH).d_supply, 0);
                    assert!(
                        pool.get_positions(&pool.address)
                            .supply
                            .get_unchecked(index)
                            > 0
                    );
                }
                match record {
                    // The admin route is trapped; plant the contradictory map directly.
                    "pool-map" => e.as_contract(&pool.address, || {
                        e.storage().persistent().set(
                            &Symbol::new(e, "PoolEmis"),
                            &soroban_sdk::map![e, (index * 2 + 1, 1_0000000u64)],
                        );
                    }),
                    "reserve-data" => e.as_contract(&pool.address, || {
                        e.storage().persistent().set(
                            &PoolDataKey::EmisData(index * 2 + 1),
                            &pool::ReserveEmissionData {
                                expiration: e.ledger().timestamp() + 1000,
                                eps: 1,
                                index: 0,
                                last_time: e.ledger().timestamp(),
                            },
                        );
                    }),
                    "pool-user-data" => e.as_contract(&pool.address, || {
                        let key: (Symbol, soroban_sdk::Map<Symbol, Val>) = (
                            Symbol::new(e, "UserEmis"),
                            soroban_sdk::map![
                                e,
                                (Symbol::new(e, "reserve_id"), (index * 2 + 1).into_val(e)),
                                (Symbol::new(e, "user"), pool.address.clone().into_val(e)),
                            ],
                        );
                        e.storage().persistent().set(
                            &key,
                            &pool::UserEmissionData {
                                index: 0,
                                accrued: 0,
                            },
                        );
                    }),
                    _ => unreachable!(),
                }
                // Cross the actual 30/45/100-day bump thresholds without
                // expiring the 31/46/120-day entries. A rejected read must
                // roll back attempted TTL extensions as well as book writes.
                fixture.jump_with_sequence(21 * 24 * 60 * 60);
                fixture
                    .oracle
                    .set_price_stable(&vec![e, 1, 1_0000000, 1, 1_0000000]);
                let before = e.to_ledger_snapshot();
                if settlement {
                    assert_eq!(
                        pool.try_gulp(&asset.address).err(),
                        Some(Ok(Error::from_contract_error(1200))),
                        "{wasm}/{record}"
                    );
                } else {
                    assert_eq!(
                        pool.try_bad_debt(&sam).err(),
                        Some(Ok(Error::from_contract_error(1200))),
                        "{wasm}/{record}"
                    );
                }
                assert_eq!(
                    e.to_ledger_snapshot(),
                    before,
                    "{wasm}/{record}/{settlement}"
                );
            }
        }
    }
}

#[test]
fn supplier_loss_reverts_final_fill_then_independent_filler_succeeds() {
    use pool::{AuctionData, PoolState, PositionData};
    use soroban_sdk::{Address, Error, TryFromVal};
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let e = &fixture.env;
        let pool = &fixture.pools[0].pool;
        let stable = &fixture.tokens[TokenIndex::STABLE];
        let xlm = &fixture.tokens[TokenIndex::XLM];
        let weth = &fixture.tokens[TokenIndex::WETH];
        let victim = Address::generate(e);
        let exposed = Address::generate(e);
        let funded = Address::generate(e);
        let stable_index = pool.get_reserve(&stable.address).config.index;
        let weth_index = pool.get_reserve(&weth.address).config.index;
        let healthy = |user: &Address| {
            e.as_contract(&pool.address, || {
                let positions = e
                    .storage()
                    .persistent()
                    .get(&PoolDataKey::Positions(user.clone()))
                    .unwrap();
                let mut state = PoolState::load(e);
                PositionData::calculate_from_positions(e, &mut state, &positions)
                    .is_hf_over(e, 1_0000100)
            })
        };
        e.mock_all_auths();
        fixture
            .oracle
            .set_price_stable(&vec![e, 1_0000000, 1_0000000, 1_000000, 1_0000000]);
        stable.mint(&funded, &(6_000 * 1_000000));
        pool.submit(
            &funded,
            &funded,
            &funded,
            &vec![
                e,
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: stable.address.clone(),
                    amount: 5_000 * 1_000000,
                },
            ],
        );
        xlm.mint(&victim, &(100_000 * 10_0000000));
        weth.mint(&victim, &2);
        pool.submit(
            &victim,
            &victim,
            &victim,
            &vec![
                e,
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: xlm.address.clone(),
                    amount: 100_000 * 10_0000000,
                },
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: weth.address.clone(),
                    amount: 2,
                },
                Request {
                    request_type: RequestType::Borrow as u32,
                    address: stable.address.clone(),
                    amount: 5_000 * 1_000000,
                },
            ],
        );
        stable.mint(&exposed, &(7 * 1_000000));
        pool.submit(
            &exposed,
            &exposed,
            &exposed,
            &vec![
                e,
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: stable.address.clone(),
                    amount: 7 * 1_000000,
                },
                Request {
                    request_type: RequestType::Borrow as u32,
                    address: weth.address.clone(),
                    amount: 4 * 1_000000000,
                },
            ],
        );
        fixture
            .oracle
            .set_price_stable(&vec![e, 1_0000000, 1_0000000, 1, 1_0000000]);
        let auction = pool.new_auction(
            &0,
            &victim,
            &vec![e, stable.address.clone()],
            &vec![e, xlm.address.clone()],
            &100,
        );
        // At 400 blocks the bid is zero: filler debt assumption cannot
        // explain the failure. The omitted WETH collateral has raw value zero.
        fixture.jump_with_sequence(401 * 5);
        let victim_before = pool.get_positions(&victim);
        let exposed_before = pool.get_positions(&exposed);
        let reserve_before = pool.get_reserve(&stable.address).data;
        assert!(healthy(&exposed));
        assert!(healthy(&funded));
        let before = e.to_ledger_snapshot();
        let fill = vec![
            e,
            Request {
                request_type: RequestType::FillUserLiquidationAuction as u32,
                address: victim.clone(),
                amount: 100,
            },
        ];
        assert_eq!(
            pool.try_submit(&exposed, &exposed, &exposed, &fill).err(),
            Some(Ok(Error::from_contract_error(1205))),
            "{wasm}"
        );
        assert_eq!(e.to_ledger_snapshot(), before, "{wasm}");
        assert_eq!(pool.get_auction(&0, &victim).to_xdr(e), auction.to_xdr(e));

        pool.submit(&funded, &funded, &funded, &fill);
        let events = e.events().all();
        let topics: soroban_sdk::Vec<Val> =
            (Symbol::new(e, "fill_auction"), 0u32, victim.clone()).into_val(e);
        let data = events
            .iter()
            .find(|(contract, event_topics, _)| {
                contract == &pool.address && event_topics == &topics
            })
            .unwrap()
            .2;
        let (filler, percent, filled) =
            <(Address, i128, AuctionData)>::try_from_val(e, &data).unwrap();
        assert_eq!(filler, funded);
        assert_eq!(percent, 100);
        assert!(filled.bid.is_empty());
        let victim_after = pool.get_positions(&victim);
        assert!(victim_after.liabilities.is_empty());
        assert!(victim_after.collateral.is_empty());
        assert_eq!(
            pool.get_positions(&pool.address).supply.get(weth_index),
            victim_before.collateral.get(weth_index)
        );
        assert!(pool.try_get_auction(&0, &victim).is_err());
        let reserve_after = pool.get_reserve(&stable.address).data;
        assert_eq!(
            reserve_after.d_supply,
            reserve_before.d_supply - victim_before.liabilities.get(stable_index).unwrap()
        );
        assert_eq!(reserve_after.b_supply, reserve_before.b_supply);
        assert!(reserve_after.b_rate < reserve_before.b_rate);
        assert_eq!(
            pool.get_positions(&exposed).to_xdr(e),
            exposed_before.to_xdr(e)
        );
        assert!(!healthy(&exposed));
        assert!(healthy(&funded));
    }
}
