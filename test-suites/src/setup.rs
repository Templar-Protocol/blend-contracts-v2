use pool::{Request, RequestType};
use soroban_sdk::{vec as svec, String, Vec as SVec};

use crate::{
    pool::default_reserve_metadata,
    test_fixture::{TestFixture, TokenIndex, SCALAR_7},
};

/// Create a test fixture with a pool and a whale depositing and borrowing all assets
pub fn create_fixture_with_data<'a>(wasm: bool) -> TestFixture<'a> {
    populate_fixture(TestFixture::create(wasm), None)
}

/// Load the actual supplied pool/backstop artifacts; economics match the ordinary fixture.
pub fn create_fixture_with_wasm<'a>(pool_wasm: &[u8], backstop_wasm: &[u8]) -> TestFixture<'a> {
    populate_fixture(
        TestFixture::create_with_artifacts(true, pool_wasm, Some(backstop_wasm)),
        Some(crate::test_fixture::RUNTIME_POOL_SALT),
    )
}

fn populate_fixture<'a>(mut fixture: TestFixture<'a>, salt: Option<[u8; 32]>) -> TestFixture<'a> {
    // mint whale tokens
    let frodo = fixture.users[0].clone();
    fixture.tokens[TokenIndex::STABLE].mint(&frodo, &(100_000 * 10i128.pow(6)));
    fixture.tokens[TokenIndex::XLM].mint(&frodo, &(1_000_000 * SCALAR_7));
    fixture.tokens[TokenIndex::WETH].mint(&frodo, &(100 * 10i128.pow(9)));

    // mint LP tokens with whale
    // Frodo has 30m BLND from the constructor-equivalent test funding.
    fixture.tokens[TokenIndex::BLND].mint(&frodo, &(70_000_000 * SCALAR_7));
    fixture.tokens[TokenIndex::USDC].mint(&frodo, &(2_600_000 * SCALAR_7));
    fixture.lp.join_pool(
        &(10_000_000 * SCALAR_7),
        &svec![&fixture.env, 110_000_000 * SCALAR_7, 2_600_000 * SCALAR_7,],
        &frodo,
    );

    // create pool
    if let Some(salt) = salt {
        fixture.create_pool_with_salt(
            String::from_str(&fixture.env, "Teapot"),
            0,
            6,
            1_0000000,
            soroban_sdk::BytesN::from_array(&fixture.env, &salt),
        );
    } else {
        fixture.create_pool(String::from_str(&fixture.env, "Teapot"), 0, 6, 1_0000000);
    }

    let mut stable_config = default_reserve_metadata();
    stable_config.decimals = 6;
    stable_config.c_factor = 0_900_0000;
    stable_config.l_factor = 0_950_0000;
    stable_config.util = 0_850_0000;
    fixture.create_pool_reserve(0, TokenIndex::STABLE, &stable_config);

    let mut xlm_config = default_reserve_metadata();
    xlm_config.c_factor = 0_750_0000;
    xlm_config.l_factor = 0_750_0000;
    xlm_config.util = 0_500_0000;
    fixture.create_pool_reserve(0, TokenIndex::XLM, &xlm_config);

    let mut weth_config = default_reserve_metadata();
    weth_config.decimals = 9;
    weth_config.c_factor = 0_800_0000;
    weth_config.l_factor = 0_800_0000;
    weth_config.util = 0_700_0000;
    weth_config.supply_cap = i128::MAX;
    fixture.create_pool_reserve(0, TokenIndex::WETH, &weth_config);

    let pool_fixture = &fixture.pools[0];

    // Retain the public LP deposit and ordinary activation without emissions.
    fixture
        .backstop
        .deposit(&frodo, &pool_fixture.pool.address, &(50_000 * SCALAR_7));
    pool_fixture.pool.set_status(&3);
    pool_fixture.pool.update_status();

    fixture.jump(60);

    // supply and borrow STABLE for 80% utilization (close to target)
    let requests: SVec<Request> = svec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 10_000 * 10i128.pow(6),
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::STABLE].address.clone(),
            amount: 8_000 * 10i128.pow(6),
        },
    ];
    pool_fixture.pool.submit(&frodo, &frodo, &frodo, &requests);

    // supply and borrow WETH for 50% utilization (below target)
    let requests: SVec<Request> = svec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::WETH].address.clone(),
            amount: 10 * 10i128.pow(9),
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::WETH].address.clone(),
            amount: 5 * 10i128.pow(9),
        },
    ];
    pool_fixture.pool.submit(&frodo, &frodo, &frodo, &requests);

    // supply and borrow XLM for 65% utilization (above target)
    let requests: SVec<Request> = svec![
        &fixture.env,
        Request {
            request_type: RequestType::SupplyCollateral as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 100_000 * SCALAR_7,
        },
        Request {
            request_type: RequestType::Borrow as u32,
            address: fixture.tokens[TokenIndex::XLM].address.clone(),
            amount: 65_000 * SCALAR_7,
        },
    ];
    pool_fixture.pool.submit(&frodo, &frodo, &frodo, &requests);

    fixture.jump(60 * 60); // 1 hr

    fixture.env.cost_estimate().budget().reset_unlimited();
    fixture
}

#[cfg(test)]
mod tests {

    use crate::test_fixture::PoolFixture;

    use super::*;

    #[test]
    fn test_create_fixture_with_data_wasm() {
        let fixture: TestFixture<'_> = create_fixture_with_data(true);
        let frodo = fixture.users.get(0).unwrap();
        let pool_fixture: &PoolFixture = fixture.pools.get(0).unwrap();

        // validate backstop deposit and drop
        assert_eq!(
            50_000 * SCALAR_7,
            fixture.lp.balance(&fixture.backstop.address)
        );
        assert_eq!(
            10_000_000 * SCALAR_7,
            fixture.tokens[TokenIndex::BLND].balance(&fixture.bombadil)
        );

        // validate pool actions
        assert_eq!(
            2_000 * 10i128.pow(6),
            fixture.tokens[TokenIndex::STABLE].balance(&pool_fixture.pool.address)
        );
        assert_eq!(
            35_000 * SCALAR_7,
            fixture.tokens[TokenIndex::XLM].balance(&pool_fixture.pool.address)
        );
        assert_eq!(
            5 * 10i128.pow(9),
            fixture.tokens[TokenIndex::WETH].balance(&pool_fixture.pool.address)
        );

        assert_eq!(
            98_000 * 10i128.pow(6),
            fixture.tokens[TokenIndex::STABLE].balance(&frodo)
        );
        assert_eq!(
            965_000 * SCALAR_7,
            fixture.tokens[TokenIndex::XLM].balance(&frodo)
        );
        assert_eq!(
            95 * 10i128.pow(9),
            fixture.tokens[TokenIndex::WETH].balance(&frodo)
        );

        // emissions are disabled in this fork cohort
        assert_eq!(fixture.read_pool_config(0).status, 1);
        assert!(fixture.backstop.reward_zone().is_empty());
    }
}
