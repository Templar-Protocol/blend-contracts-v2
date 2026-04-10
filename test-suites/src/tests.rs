#![cfg(test)]

mod test_suites_src_setup {
    use pool::{Request, RequestType, ReserveEmissionMetadata};

    use soroban_sdk::{vec as svec, String, Vec as SVec};

    use crate::{
        pool::default_reserve_metadata,
        test_fixture::{TestFixture, TokenIndex, SCALAR_7},
    };

    pub(crate) use crate::setup::*;

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

            // validate emissions are turned on
            let emis_data = fixture.read_reserve_emissions(0, TokenIndex::STABLE, 0);
            assert_eq!(
                emis_data.last_time,
                fixture.env.ledger().timestamp() - 60 * 61
            );
            assert_eq!(emis_data.index, 0);
            assert_eq!(0_180_0000_0000000, emis_data.eps);
            assert_eq!(
                fixture.env.ledger().timestamp() + 7 * 24 * 60 * 60 - 60 * 61,
                emis_data.expiration
            )
        }

        #[test]
        fn test_create_fixture_with_data_rlib() {
            let fixture = create_fixture_with_data(false);
            let frodo = fixture.users.get(0).unwrap();
            let pool_fixture: &PoolFixture = fixture.pools.get(0).unwrap();

            // validate backstop deposit
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

            // validate emissions are turned on
            let emis_data = fixture.read_reserve_emissions(0, TokenIndex::STABLE, 0);
            assert_eq!(
                emis_data.last_time,
                fixture.env.ledger().timestamp() - 60 * 61
            );
            assert_eq!(emis_data.index, 0);
            assert_eq!(0_180_0000_0000000, emis_data.eps);
            assert_eq!(
                fixture.env.ledger().timestamp() + 7 * 24 * 60 * 60 - 60 * 61,
                emis_data.expiration
            )
        }
    }
}
