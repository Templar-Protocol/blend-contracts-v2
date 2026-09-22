#![cfg(test)]
//! In the ADR 0008 fork the pool's `flash_loan` export always fails first-thing with
//! `PoolError::BadRequest` (#1200). Ledger comparisons prove rollback of token
//! balances, allowances, positions, reserves and TTLs; events and auth are
//! checked separately.

use pool::{FlashLoan, Request, RequestType};
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    vec, Address, Error,
};
use test_suites::{
    create_fixture_with_data,
    test_fixture::{TestFixture, TokenIndex, SCALAR_7},
};

/// Well-formed requests; stock-runtime positive controls live in the differential harness.
fn executable_requests(fixture: &TestFixture) -> (FlashLoan, soroban_sdk::Vec<Request>) {
    let xlm_address = fixture.tokens[TokenIndex::XLM].address.clone();
    let stable_address = fixture.tokens[TokenIndex::STABLE].address.clone();
    let flash_loan = FlashLoan {
        contract: Address::generate(&fixture.env),
        asset: xlm_address.clone(),
        amount: 100 * SCALAR_7,
    };
    let requests: soroban_sdk::Vec<Request> = vec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: stable_address.clone(),
            amount: 50 * SCALAR_7,
        },
        Request {
            request_type: RequestType::Repay as u32,
            address: xlm_address.clone(),
            amount: 90 * SCALAR_7,
        },
    ];
    (flash_loan, requests)
}

#[test]
fn test_flashloan_rejected_atomically() {
    for wasm in [false, true] {
        let fixture = create_fixture_with_data(wasm);
        let pool_fixture = &fixture.pools[0];
        let samwise = Address::generate(&fixture.env);

        // Caller holds approvals and balances; rejection comes from the endpoint itself.
        fixture.tokens[TokenIndex::XLM].mint(&samwise, &(200 * SCALAR_7));
        fixture.tokens[TokenIndex::STABLE].mint(&samwise, &(200 * SCALAR_7));
        fixture.tokens[TokenIndex::XLM].approve(
            &samwise,
            &pool_fixture.pool.address,
            &i128::MAX,
            &(fixture.env.ledger().sequence() + 17280),
        );

        let before = fixture.env.to_ledger_snapshot();
        let (flash_loan, requests) = executable_requests(&fixture);

        let result = pool_fixture
            .pool
            .try_flash_loan(&samwise, &flash_loan, &requests);
        assert_eq!(
            result.err(),
            Some(Ok(Error::from_contract_error(1200))),
            "fork requires flash_loan to fail first-thing with BadRequest #1200"
        );

        // Atomicity witness: byte-identical ledger afterward (every token balance and
        // allowance, reserve data, instance TTL, and user keys).
        assert_eq!(fixture.env.to_ledger_snapshot(), before);
        assert!(fixture.env.auths().is_empty());
        assert!(fixture.env.events().all().is_empty());
    }
}

#[test]
fn test_flashloan_rejected_without_any_permissions() {
    let fixture = create_fixture_with_data(false);
    let pool_fixture = &fixture.pools[0];
    let stranger = Address::generate(&fixture.env);

    let before = fixture.env.to_ledger_snapshot();
    let (flash_loan, requests) = executable_requests(&fixture);
    fixture.env.mock_auths(&[]);

    let result = pool_fixture
        .pool
        .try_flash_loan(&stranger, &flash_loan, &requests);
    assert_eq!(
        result.err(),
        Some(Ok(Error::from_contract_error(1200))),
        "trap fires before any authorization requirement"
    );

    assert_eq!(fixture.env.to_ledger_snapshot(), before);
    assert!(fixture.env.auths().is_empty());
    assert!(fixture.env.events().all().is_empty());
}
