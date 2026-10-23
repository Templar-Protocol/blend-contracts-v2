//! Unproved actual-initializer probe: Kani 0.68.0 ICEs before verification.
use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};

#[kani::proof]
fn prove_actual_initializer_base_case() {
    let env = Env::default();
    env.ledger().set_timestamp(kani::any());
    let contract = Address::generate(&env);
    let asset = Address::generate(&env);
    let user = Address::generate(&env);
    let config = arbitrary_config();
    env.as_contract(&contract, || {
        assert!(!storage::has_res(&env, &asset));
        let index = initialize_reserve(&env, &asset, &config);
        let stored = storage::get_res_data(&env, &asset);
        assert_eq!(index, 0);
        assert_eq!(stored.b_supply, 0);
        assert_eq!(stored.d_supply, 0);
        assert_eq!(stored.backstop_credit, 0);
        assert_eq!(stored.b_rate, SCALAR_12);
        assert_eq!(stored.d_rate, SCALAR_12);
        assert_eq!(stored.last_time, env.ledger().timestamp());
        let positions = storage::get_user_positions(&env, &user);
        assert!(positions.supply.is_empty());
        assert!(positions.collateral.is_empty());
        assert!(positions.liabilities.is_empty());
        kani::cover!(stored.b_supply == 0 && positions.liabilities.is_empty());
    });
}
