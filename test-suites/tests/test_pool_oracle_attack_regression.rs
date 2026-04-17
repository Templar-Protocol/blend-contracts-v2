#![cfg(test)]

use pool::{Request, RequestType};
use soroban_sdk::{testutils::Address as _, vec, Address};
use test_suites::{
    create_fixture_with_data,
    test_fixture::{TokenIndex, SCALAR_7},
};

const STABLE_SCALAR: i128 = 1_000_000;
const STABLE_CLEAN_PRICE: i128 = 1_0000000;
const STABLE_MIN_ATTACK_PRICE: i128 = 92_0000000;
const STABLE_EXPLOIT_PRICE: i128 = 106_7373000;
const STABLE_UNDER_BREAKER_PRICE: i128 = 1_4900000;
const ATTACK_COLLATERAL_AMOUNT: i128 = 1_000 * STABLE_SCALAR;
const ATTACK_BORROW_AMOUNT: i128 = 50_000 * SCALAR_7;
const HEALTHY_BORROW_AMOUNT: i128 = 6_000 * SCALAR_7;
const EXTRA_XLM_LIQUIDITY: i128 = 100_000 * SCALAR_7;
const MANIPULATION_WINDOW_SECS: u64 = 300;

fn set_prices(fixture: &test_suites::test_fixture::TestFixture<'_>, stable_price: i128) {
    fixture.oracle.set_price_stable(&vec![
        &fixture.env,
        2000_0000000,
        1_0000000,
        0_1000000,
        stable_price,
    ]);
}

fn attack_requests(
    fixture: &test_suites::test_fixture::TestFixture<'_>,
    borrow_amount: i128,
) -> soroban_sdk::Vec<Request> {
    vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: ATTACK_COLLATERAL_AMOUNT,
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: borrow_amount,
        },
    ]
}

fn supply_xlm_liquidity(
    fixture: &test_suites::test_fixture::TestFixture<'_>,
    supplier: &Address,
    amount: i128,
) {
    fixture.tokens[TokenIndex::XLM].mint(supplier, &amount);
    fixture.pools[0].pool.submit(
        supplier,
        supplier,
        supplier,
        &vec![
            &fixture.env,
            Request {
                request_type: RequestType::Supply as u32,
                address: fixture.tokens[TokenIndex::XLM].address.clone(),
                amount,
            },
        ],
    );
}

#[test]
fn test_clean_price_allows_reasonable_borrow() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];
    let stable_pool_index = pool_fixture.reserves[&TokenIndex::STABLE];
    let xlm_pool_index = pool_fixture.reserves[&TokenIndex::XLM];
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    set_prices(&fixture, STABLE_CLEAN_PRICE);

    let positions = pool_fixture.pool.submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, HEALTHY_BORROW_AMOUNT),
    );

    assert!(positions.collateral.get(stable_pool_index).unwrap_or(0) > 0);
    assert!(positions.liabilities.get(xlm_pool_index).unwrap_or(0) > 0);
}

#[test]
fn test_rejects_borrow_against_minimum_attack_price_threshold() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    set_prices(&fixture, STABLE_MIN_ATTACK_PRICE);

    let result = pool_fixture.pool.try_submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, ATTACK_BORROW_AMOUNT),
    );

    assert!(
        result.is_err(),
        "borrow against a 92x manipulated collateral price should be rejected before the position opens"
    );
    let positions = pool_fixture.pool.get_positions(&attacker);
    assert_eq!(positions.collateral.len(), 0);
    assert_eq!(positions.liabilities.len(), 0);
}

#[test]
fn test_rejects_borrow_against_exact_exploit_price() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    set_prices(&fixture, STABLE_EXPLOIT_PRICE);

    let result = pool_fixture.pool.try_submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, ATTACK_BORROW_AMOUNT),
    );

    assert!(
        result.is_err(),
        "borrow against the exact exploit-style collateral price should be rejected before the position opens"
    );
    let positions = pool_fixture.pool.get_positions(&attacker);
    assert_eq!(positions.collateral.len(), 0);
    assert_eq!(positions.liabilities.len(), 0);
}

#[test]
fn test_rejects_borrow_after_sustained_multi_window_manipulation() {
    let fixture = create_fixture_with_data(false);
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    for _ in 0..4 {
        set_prices(&fixture, STABLE_EXPLOIT_PRICE);
        fixture.jump_with_sequence(MANIPULATION_WINDOW_SECS);
    }

    let result = fixture.pools[0].pool.try_submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, ATTACK_BORROW_AMOUNT),
    );

    assert!(
        result.is_err(),
        "borrow should be rejected even after sustaining the manipulated collateral price across multiple windows"
    );
}

#[test]
fn test_rejects_borrow_after_slightly_varied_manipulated_prices() {
    let fixture = create_fixture_with_data(false);
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    for bump in [0i128, 1_000, 2_000, 3_000] {
        set_prices(&fixture, STABLE_EXPLOIT_PRICE + bump);
        fixture.jump_with_sequence(MANIPULATION_WINDOW_SECS);
    }

    let result = fixture.pools[0].pool.try_submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, ATTACK_BORROW_AMOUNT),
    );

    assert!(
        result.is_err(),
        "borrow should be rejected when the manipulated collateral price drifts slightly across windows"
    );
}

#[test]
fn test_rejects_borrow_after_sandwiched_manipulation() {
    let fixture = create_fixture_with_data(false);
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    set_prices(&fixture, STABLE_CLEAN_PRICE);
    fixture.jump_with_sequence(MANIPULATION_WINDOW_SECS);
    set_prices(&fixture, STABLE_EXPLOIT_PRICE);

    let result = fixture.pools[0].pool.try_submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, ATTACK_BORROW_AMOUNT),
    );

    assert!(
        result.is_err(),
        "borrow should be rejected when a manipulated price is sandwiched behind an otherwise clean window"
    );
}

#[test]
fn test_rejects_borrow_just_under_circuit_breaker_style_price() {
    let fixture = create_fixture_with_data(false);
    let attacker = Address::generate(&fixture.env);
    let liquidity_supplier = Address::generate(&fixture.env);

    supply_xlm_liquidity(&fixture, &liquidity_supplier, EXTRA_XLM_LIQUIDITY);
    fixture.tokens[TokenIndex::STABLE].mint(&attacker, &ATTACK_COLLATERAL_AMOUNT);
    set_prices(&fixture, STABLE_UNDER_BREAKER_PRICE);

    let result = fixture.pools[0].pool.try_submit(
        &attacker,
        &attacker,
        &attacker,
        &attack_requests(&fixture, ATTACK_BORROW_AMOUNT),
    );

    assert!(
        result.is_err(),
        "borrow should be rejected even when the manipulated collateral price stays just under a circuit-breaker-style threshold"
    );
}
