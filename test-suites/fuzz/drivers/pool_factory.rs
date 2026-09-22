use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use pool::PoolClient;
use pool_factory::{PoolFactoryClient, PoolFactoryContract, PoolInitMeta};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, String};
use test_suites::pool::POOL_WASM;
use test_suites::test_fixture::{TestFixture, TokenIndex};

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let fixture = TestFixture::create(mode.uses_wasm());
    let factory_address = if mode.uses_wasm() {
        fixture.pool_factory.address.clone()
    } else {
        let address = Address::generate(&fixture.env);
        let pool_hash = fixture.env.deployer().upload_contract_wasm(POOL_WASM);
        fixture.env.register_at(
            &address,
            PoolFactoryContract {},
            (PoolInitMeta {
                backstop: fixture.backstop.address.clone(),
                pool_hash,
                blnd_id: fixture.tokens[TokenIndex::BLND].address.clone(),
            },),
        );
        address
    };
    let pool_factory = PoolFactoryClient::new(&fixture.env, &factory_address);
    let mut report = RunReport::decoded(operations.len());

    for (index, operation) in operations.iter().enumerate() {
        if operation.code % 4 == 0 {
            let unrelated = Address::generate(&fixture.env);
            assert!(!pool_factory.is_pool(&unrelated));
            report.noop();
            continue;
        }

        let take_rate = match operation.flags & 3 {
            0 => operation.amount % 10_000_000,
            1 => 9_999_999,
            2 => 10_000_000,
            _ => 10_000_001,
        };
        let max_positions = match operation.actor & 3 {
            0 => 1,
            1 => 2,
            2 => 60,
            _ => 61,
        };
        let min_collateral = if operation.asset & 1 == 0 {
            i128::from(operation.amount)
        } else {
            -i128::from(operation.amount) - 1
        };
        let expected =
            take_rate < 10_000_000 && (2..=60).contains(&max_positions) && min_collateral >= 0;
        let mut salt = [0u8; 32];
        salt[0] = index as u8;
        salt[1] = operation.code;
        salt[2] = operation.actor;
        salt[3] = operation.asset;
        salt[4] = operation.flags;
        salt[5..9].copy_from_slice(&operation.amount.to_le_bytes());
        let salt = BytesN::from_array(&fixture.env, &salt);
        let name = String::from_str(&fixture.env, "fuzz-pool");
        let result = pool_factory.try_deploy(
            &fixture.bombadil,
            &name,
            &salt,
            &fixture.oracle.address,
            &take_rate,
            &max_positions,
            &min_collateral,
        );
        let deployed = contract_call(&fixture.env, result, &mut report);
        // MUTATION-CHECK: this independent boundary oracle kills <=/>= changes
        // in both factory deployment and pool construction validation.
        assert_eq!(deployed.is_some(), expected);
        if let Some(address) = deployed {
            assert!(pool_factory.is_pool(&address));
            let pool = PoolClient::new(&fixture.env, &address);
            let config = pool.get_config();
            assert_eq!(config.bstop_rate, take_rate);
            assert_eq!(config.max_positions, max_positions);
            assert_eq!(config.min_collateral, min_collateral);
            report.observe_u64(u64::from(config.max_positions));
        }
    }

    report
}
