//! Genesis no-effect checks; stock-positive prepared-state controls live in the differential harness.
use pool::ReserveEmissionMetadata;
use soroban_sdk::{
    map,
    testutils::{Address as _, Events as _, Ledger as _},
    vec, Address, Error, InvokeError,
};
use test_suites::{
    create_fixture_with_data,
    test_fixture::{TokenIndex, SCALAR_7},
};

fn expect_trap<T, E>(result: Result<Result<T, E>, Result<Error, InvokeError>>) {
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(1000))));
}

#[test]
fn six_traps_preserve_complete_ledger_ttls_and_events() {
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let e = &fixture.env;
        let user = &fixture.users[0];
        let pool = &fixture.pools[0].pool.address;
        for entry in 0..6 {
            let before = e.to_ledger_snapshot();
            match entry {
                0 => expect_trap(fixture.backstop.try_distribute()),
                1 => expect_trap(fixture.backstop.try_gulp_emissions(pool)),
                2 => expect_trap(fixture.backstop.try_add_reward(pool, &None)),
                3 => expect_trap(fixture.backstop.try_remove_reward(pool)),
                4 => expect_trap(fixture.backstop.try_claim(user, &vec![e, pool.clone()], &0)),
                _ => expect_trap(fixture.backstop.try_drop()),
            }
            assert_eq!(
                e.to_ledger_snapshot(),
                before,
                "wasm={wasm}, export={entry}"
            );
            assert!(e.events().all().is_empty());
            assert!(e.auths().is_empty());
        }
    }
}

#[test]
fn unsolicited_blnd_stays_custodied() {
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let blnd = &fixture.tokens[TokenIndex::BLND];
        let backstop = &fixture.backstop.address;
        let before = blnd.balance(backstop);
        blnd.mint(&fixture.users[0], &(100 * SCALAR_7));
        blnd.transfer(&fixture.users[0], backstop, &(100 * SCALAR_7));
        let snapshot = fixture.env.to_ledger_snapshot();
        expect_trap(fixture.backstop.try_distribute());
        expect_trap(
            fixture
                .backstop
                .try_gulp_emissions(&fixture.pools[0].pool.address),
        );
        expect_trap(fixture.backstop.try_drop());
        assert_eq!(fixture.env.to_ledger_snapshot(), snapshot);
        assert_eq!(blnd.balance(backstop), before + 100 * SCALAR_7);
    }
}

#[test]
fn deposit_queue_and_withdraw_survive_trap_interleavings() {
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let e = &fixture.env;
        let user = Address::generate(e);
        let pool = &fixture.pools[0].pool.address;
        let amount = 100 * SCALAR_7;
        fixture.lp.transfer(&fixture.users[0], &user, &amount);
        let backstop_lp = fixture.lp.balance(&fixture.backstop.address);
        let shares = fixture.backstop.deposit(&user, pool, &amount);
        assert!(shares > 0);
        assert_eq!(fixture.backstop.user_balance(pool, &user).shares, shares);
        expect_trap(fixture.backstop.try_gulp_emissions(pool));
        let queued = fixture.backstop.queue_withdrawal(&user, pool, &shares);
        assert_eq!(queued.amount, shares);
        expect_trap(fixture.backstop.try_distribute());
        fixture.jump(queued.exp - e.ledger().timestamp());
        assert_eq!(fixture.backstop.withdraw(&user, pool, &shares), amount);
        let after = fixture.backstop.user_balance(pool, &user);
        assert_eq!(after.shares, 0);
        assert!(after.q4w.is_empty());
        assert_eq!(fixture.lp.balance(&user), amount);
        assert_eq!(fixture.lp.balance(&fixture.backstop.address), backstop_lp);
        expect_trap(fixture.backstop.try_add_reward(pool, &None));
        assert!(fixture.backstop.reward_zone().is_empty());
    }
}

#[test]
fn live_pool_config_cannot_create_emission_state() {
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let e = &fixture.env;
        let pool = &fixture.pools[0].pool;
        let stable = fixture.pools[0].reserves[&TokenIndex::STABLE];
        let xlm = fixture.pools[0].reserves[&TokenIndex::XLM];
        let before_config_changes = e.to_ledger_snapshot();
        // The admin emissions route is trapped (ADR 0008 restriction 7).
        assert_eq!(
            pool.try_set_emissions_config(&vec![
                e,
                ReserveEmissionMetadata {
                    res_index: stable,
                    res_type: 0,
                    share: 4_000000,
                },
                ReserveEmissionMetadata {
                    res_index: xlm,
                    res_type: 1,
                    share: 6_000000,
                },
            ])
            .err(),
            Some(Ok(Error::from_contract_error(1200)))
        );
        assert_eq!(e.to_ledger_snapshot(), before_config_changes);
        assert_eq!(
            pool.try_update_pool(&0u32, &3u32, &(10 * SCALAR_7)).err(),
            Some(Ok(Error::from_contract_error(1200)))
        );
        assert_eq!(e.to_ledger_snapshot(), before_config_changes);
        let before = e.to_ledger_snapshot();
        expect_trap(pool.try_gulp_emissions());
        assert_eq!(e.to_ledger_snapshot(), before);
        for index in [stable * 2, xlm * 2 + 1] {
            assert!(pool.get_reserve_emissions(&index).is_none());
            assert!(pool.get_user_emissions(&fixture.users[0], &index).is_none());
        }
    }
}
