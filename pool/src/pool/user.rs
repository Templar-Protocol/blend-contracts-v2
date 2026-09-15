use crate::{
    constants::SCALAR_12, emissions, math::FixedMath, storage, storage::ReserveData,
    validator::require_nonnegative, PoolError,
};
use soroban_sdk::{contracttype, panic_with_error, Address, Env, Map};

use super::{Pool, Reserve};

/// A user / contracts position's with the pool, stored in the Reserve's decimals
#[derive(Clone)]
#[contracttype]
pub struct Positions {
    pub liabilities: Map<u32, i128>, // Map of Reserve Index to liability share balance
    pub collateral: Map<u32, i128>,  // Map of Reserve Index to collateral supply share balance
    pub supply: Map<u32, i128>,      // Map of Reserve Index to non-collateral supply share balance
}

impl Positions {
    /// Create an empty Positions object in the environment
    pub fn env_default(e: &Env) -> Self {
        Positions {
            liabilities: Map::new(e),
            collateral: Map::new(e),
            supply: Map::new(e),
        }
    }

    /// Get the number of effective (impacts health factor) posiitons the user holds.
    ///
    /// This function ignores non-collateralized supply positions, as they are not relevant to the
    /// max number of allowed positions by the pool.
    pub fn effective_count(&self) -> u32 {
        self.liabilities.len() + self.collateral.len()
    }
}

/// A user / contracts position's with the pool
#[derive(Clone)]
pub struct User {
    pub address: Address,
    pub positions: Positions,
}

impl User {
    /// Create an empty User object in the environment
    pub fn load(e: &Env, address: &Address) -> Self {
        User {
            address: address.clone(),
            positions: storage::get_user_positions(e, address),
        }
    }

    /// Store the user's positions to the ledger
    pub fn store(&self, e: &Env) {
        storage::set_user_positions(e, &self.address, &self.positions);
    }

    /// Check if the user has liabilities
    pub fn has_liabilities(&self) -> bool {
        !self.positions.liabilities.is_empty()
    }

    /// Get the debtToken position for the reserve at the given index
    pub fn get_liabilities(&self, reserve_index: u32) -> i128 {
        self.positions.liabilities.get(reserve_index).unwrap_or(0)
    }

    /// Add liabilities to the position expressed in debtTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's d_supply.
    pub fn add_liabilities(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        if amount <= 0 {
            panic_with_error!(e, PoolError::InvalidDTokenMintAmount)
        }
        let balance = self.get_liabilities(reserve.config.index);
        self.update_d_emissions(e, reserve, balance);
        self.positions
            .liabilities
            .set(reserve.config.index, balance + amount);
        reserve.data.d_supply += amount;
    }

    /// Remove liabilities from the position expressed in debtTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's d_supply.
    pub fn remove_liabilities(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        if amount <= 0 {
            panic_with_error!(e, PoolError::InvalidDTokenBurnAmount)
        }
        let balance = self.get_liabilities(reserve.config.index);
        self.update_d_emissions(e, reserve, balance);
        let new_balance = balance - amount;
        require_nonnegative(e, &new_balance);
        if new_balance == 0 {
            self.positions.liabilities.remove(reserve.config.index);
        } else {
            self.positions
                .liabilities
                .set(reserve.config.index, new_balance);
        }
        reserve.data.d_supply -= amount;
    }

    /// Default on liabilities from the position expressed in debtTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's b_rate and d_supply.
    ///
    /// This should only be called if the liabilities are being defaulted on. The liability will
    /// be forgiven and outstanding suppliers will lose funds. With no supplier claims,
    /// preserve b_rate while clearing the debt.
    pub fn default_liabilities(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        self.remove_liabilities(e, reserve, amount);
        // Only outstanding supplier claims absorb the default.
        let new_rate = default_b_rate(e, &reserve.data, amount);
        reserve.data.b_rate = new_rate;
    }

    /// Check if the user has collateral
    pub fn has_collateral(&self) -> bool {
        !self.positions.collateral.is_empty()
    }

    /// Get the collateralized blendToken position for the reserve at the given index
    pub fn get_collateral(&self, reserve_index: u32) -> i128 {
        self.positions.collateral.get(reserve_index).unwrap_or(0)
    }

    /// Add collateral to the position expressed in blendTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's b_supply.
    pub fn add_collateral(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        if amount <= 0 {
            panic_with_error!(e, PoolError::InvalidBTokenMintAmount)
        }
        let balance = self.get_collateral(reserve.config.index);
        self.update_b_emissions(e, reserve, self.get_total_supply(reserve.config.index));
        self.positions
            .collateral
            .set(reserve.config.index, balance + amount);
        reserve.data.b_supply += amount;
    }

    /// Remove collateral from the position expressed in blendTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's d_supply.
    pub fn remove_collateral(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        if amount <= 0 {
            panic_with_error!(e, PoolError::InvalidBTokenBurnAmount)
        }
        let balance = self.get_collateral(reserve.config.index);
        self.update_b_emissions(e, reserve, self.get_total_supply(reserve.config.index));
        let new_balance = balance - amount;
        require_nonnegative(e, &new_balance);
        if new_balance == 0 {
            self.positions.collateral.remove(reserve.config.index);
        } else {
            self.positions
                .collateral
                .set(reserve.config.index, new_balance);
        }
        reserve.data.burn_supply(amount);
    }

    /// Get the uncollateralized blendToken position for the reserve at the given index
    pub fn get_supply(&self, reserve_index: u32) -> i128 {
        self.positions.supply.get(reserve_index).unwrap_or(0)
    }

    /// Add supply to the position expressed in blendTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's b_supply.
    pub fn add_supply(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        if amount <= 0 {
            panic_with_error!(e, PoolError::InvalidBTokenMintAmount)
        }
        let balance = self.get_supply(reserve.config.index);
        self.update_b_emissions(e, reserve, self.get_total_supply(reserve.config.index));
        self.positions
            .supply
            .set(reserve.config.index, balance + amount);
        reserve.data.b_supply += amount;
    }

    /// Remove supply from the position expressed in blendTokens. Accrues emissions
    /// against the balance if necessary and updates the reserve's b_supply.
    pub fn remove_supply(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        if amount <= 0 {
            panic_with_error!(e, PoolError::InvalidBTokenBurnAmount)
        }
        let balance = self.get_supply(reserve.config.index);
        self.update_b_emissions(e, reserve, self.get_total_supply(reserve.config.index));
        let new_balance = balance - amount;
        require_nonnegative(e, &new_balance);
        if new_balance == 0 {
            self.positions.supply.remove(reserve.config.index);
        } else {
            self.positions.supply.set(reserve.config.index, new_balance);
        }
        reserve.data.burn_supply(amount);
    }

    /// Get the total supply and collateral of blendTokens for the user at the given index
    pub fn get_total_supply(&self, reserve_index: u32) -> i128 {
        self.get_collateral(reserve_index) + self.get_supply(reserve_index)
    }

    /// Removes positions from a user - does not consider supply
    pub fn rm_positions(
        &mut self,
        e: &Env,
        pool: &mut Pool,
        collateral_amounts: Map<Address, i128>,
        liability_amounts: Map<Address, i128>,
    ) {
        for (asset, amount) in collateral_amounts.iter() {
            if amount > 0 {
                let mut reserve = pool.load_reserve(e, &asset, true);
                self.remove_collateral(e, &mut reserve, amount);
                pool.cache_reserve(reserve);
            }
        }
        for (asset, amount) in liability_amounts.iter() {
            if amount > 0 {
                let mut reserve = pool.load_reserve(e, &asset, true);
                self.remove_liabilities(e, &mut reserve, amount);
                pool.cache_reserve(reserve);
            }
        }
    }

    /// Adds positions to a user - does not consider supply
    pub fn add_positions(
        &mut self,
        e: &Env,
        pool: &mut Pool,
        collateral_amounts: Map<Address, i128>,
        liability_amounts: Map<Address, i128>,
    ) {
        for (asset, amount) in collateral_amounts.iter() {
            if amount > 0 {
                let mut reserve = pool.load_reserve(e, &asset, true);
                self.add_collateral(e, &mut reserve, amount);
                pool.cache_reserve(reserve);
            }
        }
        for (asset, amount) in liability_amounts.iter() {
            if amount > 0 {
                let mut reserve = pool.load_reserve(e, &asset, true);
                self.add_liabilities(e, &mut reserve, amount);
                pool.cache_reserve(reserve);
            }
        }
    }

    fn update_d_emissions(&self, e: &Env, reserve: &Reserve, amount: i128) {
        emissions::update_emissions(
            e,
            reserve.config.index * 2,
            reserve.data.d_supply,
            reserve.scalar,
            &self.address,
            amount,
        );
    }

    fn update_b_emissions(&self, e: &Env, reserve: &Reserve, amount: i128) {
        emissions::update_emissions(
            e,
            reserve.config.index * 2 + 1,
            reserve.data.b_supply,
            reserve.scalar,
            &self.address,
            amount,
        );
    }
}

/// The b_rate after a default of `defaulted` d-tokens: outstanding suppliers
/// absorb the ceil-rounded underlying loss; with no supplier claims the
/// existing rate is preserved without reaching the division.
fn default_b_rate(math: &impl FixedMath, reserve: &ReserveData, defaulted: i128) -> i128 {
    if reserve.b_supply > 0 {
        let default_amount = reserve.to_asset_from_d_token(math, defaulted);
        let b_rate_loss = math.ceil(default_amount, SCALAR_12, reserve.b_supply);
        let new_rate = reserve.b_rate - b_rate_loss;
        if new_rate < 0 {
            return 0;
        }
        return new_rate;
    }
    reserve.b_rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{constants::SCALAR_7, storage, testutils, ReserveEmissionData, UserEmissionData};
    use soroban_fixed_point_math::SorobanFixedPoint;
    use soroban_sdk::{
        map,
        testutils::{Address as _, Ledger, LedgerInfo},
    };

    #[test]
    fn test_load_and_store() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let user = User {
            address: samwise.clone(),
            positions: Positions {
                collateral: map![&e, (0, 10000)],
                liabilities: map![&e],
                supply: map![&e],
            },
        };
        e.as_contract(&pool, || {
            user.store(&e);
            let loaded_user = User::load(&e, &samwise);
            assert_eq!(loaded_user.address, samwise);
            assert_eq!(loaded_user.positions.collateral.len(), 1);
            assert_eq!(loaded_user.positions.collateral.get_unchecked(0), 10000);
            assert_eq!(loaded_user.positions.liabilities.len(), 0);
            assert_eq!(loaded_user.positions.supply.len(), 0);
        });
    }

    #[test]
    fn test_liabilities() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_d_supply_0 = reserve_0.data.d_supply;

        let mut reserve_1 = testutils::default_reserve(&e);
        reserve_1.config.index = 1;
        let starting_d_supply_1 = reserve_1.data.d_supply;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_liabilities(0), 0);
            assert_eq!(user.has_liabilities(), false);

            user.add_liabilities(&e, &mut reserve_0, 123);
            assert_eq!(user.get_liabilities(0), 123);
            assert_eq!(reserve_0.data.d_supply, starting_d_supply_0 + 123);
            assert_eq!(user.has_liabilities(), true);

            user.add_liabilities(&e, &mut reserve_1, 456);
            assert_eq!(user.get_liabilities(0), 123);
            assert_eq!(user.get_liabilities(1), 456);
            assert_eq!(reserve_1.data.d_supply, starting_d_supply_1 + 456);

            user.remove_liabilities(&e, &mut reserve_1, 100);
            assert_eq!(user.get_liabilities(1), 356);
            assert_eq!(reserve_1.data.d_supply, starting_d_supply_1 + 356);

            user.remove_liabilities(&e, &mut reserve_1, 356);
            assert_eq!(user.get_liabilities(1), 0);
            assert_eq!(user.positions.liabilities.len(), 1);
            assert_eq!(reserve_1.data.d_supply, starting_d_supply_1);

            user.remove_liabilities(&e, &mut reserve_0, 123);
            assert_eq!(user.get_liabilities(0), 0);
            assert_eq!(user.positions.liabilities.len(), 0);
            assert_eq!(reserve_0.data.d_supply, starting_d_supply_0);
            assert_eq!(user.has_liabilities(), false);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1218)")]
    fn test_add_liabilities_zero_mint() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_liabilities(0), 0);

            user.add_liabilities(&e, &mut reserve_0, 0);
        });
    }

    #[test]
    fn test_add_liabilities_accrues_emissions() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        e.ledger().set(LedgerInfo {
            protocol_version: 22,
            sequence_number: 1,
            timestamp: 10001000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_d_supply_0 = reserve_0.data.d_supply;

        let emis_res_data = ReserveEmissionData {
            expiration: 20000000,
            eps: 0_10000000000000,
            index: 10000000000,
            last_time: 10000000, // 1000s elapsed
        };
        let emis_user_data = UserEmissionData {
            index: 9000000000,
            accrued: 0,
        };

        let mut user = User {
            address: samwise.clone(),
            positions: Positions {
                liabilities: map![&e, (reserve_0.config.index, 1000)],
                collateral: map![&e],
                supply: map![&e],
            },
        };

        e.as_contract(&pool, || {
            let res_0_d_token_index = reserve_0.config.index * 2 + 0;
            storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
            storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

            user.add_liabilities(&e, &mut reserve_0, 123);
            assert_eq!(user.get_liabilities(0), 1123);
            assert_eq!(reserve_0.data.d_supply, starting_d_supply_0 + 123);

            let new_emis_res_data = storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
            let new_index = 10000000000
                + (1000i128 * 0_10000000000000).fixed_div_floor(
                    &e,
                    &starting_d_supply_0,
                    &SCALAR_7,
                );
            assert_eq!(new_emis_res_data.last_time, 10001000);
            assert_eq!(new_emis_res_data.index, new_index);
            let user_emis_data =
                storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
            let new_accrual = 0
                + (new_index - emis_user_data.index).fixed_mul_floor(
                    &e,
                    &1000,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(user_emis_data.accrued, new_accrual);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1219)")]
    fn test_remove_liabilities_zero_burn() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_liabilities(0), 0);

            user.add_liabilities(&e, &mut reserve_0, 123);
            assert_eq!(user.get_liabilities(0), 123);

            user.remove_liabilities(&e, &mut reserve_0, 0);
        });
    }

    #[test]
    fn test_remove_liabilities_accrues_emissions() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        e.ledger().set(LedgerInfo {
            protocol_version: 22,
            sequence_number: 1,
            timestamp: 10001000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_d_supply_0 = reserve_0.data.d_supply;

        let emis_res_data = ReserveEmissionData {
            expiration: 20000000,
            eps: 0_10000000000000,
            index: 10000000000,
            last_time: 10000000, // 1000s elapsed
        };
        let emis_user_data = UserEmissionData {
            index: 9000000000,
            accrued: 0,
        };
        let mut user = User {
            address: samwise.clone(),
            positions: Positions {
                liabilities: map![&e, (reserve_0.config.index, 1000)],
                collateral: map![&e],
                supply: map![&e],
            },
        };
        e.as_contract(&pool, || {
            let res_0_d_token_index = reserve_0.config.index * 2 + 0;
            storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
            storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

            user.remove_liabilities(&e, &mut reserve_0, 123);
            assert_eq!(user.get_liabilities(0), 877);
            assert_eq!(reserve_0.data.d_supply, starting_d_supply_0 - 123);

            let new_emis_res_data = storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
            let new_index = 10000000000
                + (1000i128 * 0_1000000).fixed_div_floor(
                    &e,
                    &starting_d_supply_0,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(new_emis_res_data.last_time, 10001000);
            assert_eq!(new_emis_res_data.index, new_index);
            let user_emis_data =
                storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
            let new_accrual = 0
                + (new_index - emis_user_data.index).fixed_mul_floor(
                    &e,
                    &1000,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(user_emis_data.accrued, new_accrual);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_remove_liabilities_over_balance_panics() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            user.add_liabilities(&e, &mut reserve_0, 123);
            assert_eq!(user.get_liabilities(0), 123);

            user.remove_liabilities(&e, &mut reserve_0, 124);
        });
    }

    #[test]
    fn test_default_liabilities_reduces_b_rate() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        reserve_0.data.d_rate = 1_500_000_000_000;
        reserve_0.data.d_supply = 500_0000000;
        reserve_0.data.b_rate = 1_250_000_000_000;
        reserve_0.data.b_supply = 750_0000000;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_liabilities(0), 0);

            user.add_liabilities(&e, &mut reserve_0, 20_0000000);
            assert_eq!(user.get_liabilities(0), 20_0000000);

            let d_supply = reserve_0.data.d_supply;
            let total_supply = reserve_0.total_supply(&e);
            let underlying_default_amount = reserve_0.to_asset_from_d_token(&e, 20_0000000);
            user.default_liabilities(&e, &mut reserve_0, 20_0000000);

            assert_eq!(user.get_liabilities(0), 0);
            assert_eq!(reserve_0.data.d_supply, d_supply - 20_0000000);
            assert_eq!(
                reserve_0.total_supply(&e),
                total_supply - underlying_default_amount
            );
            assert_eq!(reserve_0.data.b_rate, 1_210_000_000_000);
            assert_eq!(reserve_0.data.b_supply, 750_0000000);
        });
    }

    #[test]
    fn test_default_liabilities_without_suppliers_preserves_b_rate() {
        let e = Env::default();
        e.mock_all_auths();
        let pool = testutils::create_pool(&e);
        let mut reserve = testutils::default_reserve(&e);
        reserve.data.d_supply = 0;
        reserve.data.b_supply = 0;
        reserve.data.b_rate = 1_250_000_000_000;
        let mut user = User {
            address: Address::generate(&e),
            positions: Positions::env_default(&e),
        };

        e.as_contract(&pool, || {
            user.add_liabilities(&e, &mut reserve, 20_0000000);
            user.default_liabilities(&e, &mut reserve, 20_0000000);

            assert!(!user.has_liabilities());
            assert_eq!(reserve.data.d_supply, 0);
            assert_eq!(reserve.data.b_supply, 0);
            assert_eq!(reserve.data.b_rate, 1_250_000_000_000);
        });
    }

    #[test]
    fn test_default_liabilities_reduces_b_rate_to_zero() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        reserve_0.data.d_rate = 1_500_000_000_000;
        reserve_0.data.d_supply = 500_0000000;
        reserve_0.data.b_rate = 0_100_000_000_000;
        reserve_0.data.b_supply = 750_0000000;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_liabilities(0), 0);

            user.add_liabilities(&e, &mut reserve_0, 100_0000000);
            assert_eq!(user.get_liabilities(0), 100_0000000);

            let d_supply = reserve_0.data.d_supply;
            user.default_liabilities(&e, &mut reserve_0, 100_0000000);

            assert_eq!(user.get_liabilities(0), 0);
            assert_eq!(reserve_0.data.d_supply, d_supply - 100_0000000);
            assert_eq!(reserve_0.total_supply(&e), 0);
            assert_eq!(reserve_0.data.b_rate, 0);
            assert_eq!(reserve_0.data.b_supply, 750_0000000);
        });
    }

    #[test]
    fn test_default_liabilities_reduces_b_rate_rounds_ceil() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        reserve_0.data.d_rate = 1_500_000_000_000;
        reserve_0.data.d_supply = 500_0000000;
        reserve_0.data.b_rate = 1_250_000_000_000;
        reserve_0.data.b_supply = 750_0000000;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_liabilities(0), 0);

            user.add_liabilities(&e, &mut reserve_0, 20_0000001);
            assert_eq!(user.get_liabilities(0), 20_0000001);

            let d_supply = reserve_0.data.d_supply;
            let total_supply = reserve_0.total_supply(&e);
            let underlying_default_amount = reserve_0.to_asset_from_d_token(&e, 20_0000001);
            user.default_liabilities(&e, &mut reserve_0, 20_0000001);

            // rounding loss of 1 stroop for resulting total supply
            assert_eq!(user.get_liabilities(0), 0);
            assert_eq!(reserve_0.data.d_supply, d_supply - 20_0000001);
            assert_eq!(
                reserve_0.total_supply(&e),
                total_supply - underlying_default_amount - 1
            );
            assert_eq!(reserve_0.data.b_rate, 1_209_999_999_733);
            assert_eq!(reserve_0.data.b_supply, 750_0000000);
        });
    }

    #[test]
    fn test_collateral() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_b_supply_0 = reserve_0.data.b_supply;

        let mut reserve_1 = testutils::default_reserve(&e);
        reserve_1.config.index = 1;
        let starting_b_supply_1 = reserve_1.data.b_supply;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_collateral(0), 0);
            assert_eq!(user.has_collateral(), false);

            user.add_collateral(&e, &mut reserve_0, 123);
            assert_eq!(user.get_collateral(0), 123);
            assert_eq!(reserve_0.data.b_supply, starting_b_supply_0 + 123);
            assert_eq!(user.has_collateral(), true);

            user.add_collateral(&e, &mut reserve_1, 456);
            assert_eq!(user.get_collateral(0), 123);
            assert_eq!(user.get_collateral(1), 456);
            assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 456);
            assert_eq!(user.has_collateral(), true);

            user.remove_collateral(&e, &mut reserve_1, 100);
            assert_eq!(user.get_collateral(1), 356);
            assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 356);
            assert_eq!(user.has_collateral(), true);

            user.remove_collateral(&e, &mut reserve_1, 356);
            assert_eq!(user.get_collateral(1), 0);
            assert_eq!(user.positions.collateral.len(), 1);
            assert_eq!(reserve_1.data.b_supply, starting_b_supply_1);
            assert_eq!(user.has_collateral(), true);

            user.remove_collateral(&e, &mut reserve_0, 123);
            assert_eq!(user.get_collateral(0), 0);
            assert_eq!(user.positions.collateral.len(), 0);
            assert_eq!(reserve_0.data.b_supply, starting_b_supply_0);
            assert_eq!(user.has_collateral(), false);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1216)")]
    fn test_add_collateral_zero_mint() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_collateral(0), 0);

            user.add_collateral(&e, &mut reserve_0, 0);
        });
    }

    #[test]
    fn test_add_collateral_accrues_emissions() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        e.ledger().set(LedgerInfo {
            protocol_version: 22,
            sequence_number: 1,
            timestamp: 10001000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_b_token_supply = reserve_0.data.b_supply;

        let emis_res_data = ReserveEmissionData {
            expiration: 20000000,
            eps: 0_10000000000000,
            index: 10000000000,
            last_time: 10000000, // 1000s elapsed
        };
        let emis_user_data = UserEmissionData {
            index: 9000000000,
            accrued: 0,
        };

        let mut user = User {
            address: samwise.clone(),
            positions: Positions {
                liabilities: map![&e],
                collateral: map![&e, (reserve_0.config.index, 700)],
                supply: map![&e, (reserve_0.config.index, 300)],
            },
        };
        e.as_contract(&pool, || {
            let res_0_d_token_index = reserve_0.config.index * 2 + 1;
            storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
            storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

            user.add_collateral(&e, &mut reserve_0, 123);
            assert_eq!(user.get_collateral(0), 823);
            assert_eq!(reserve_0.data.b_supply, starting_b_token_supply + 123);

            let new_emis_res_data = storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
            let new_index = 10000000000
                + (1000i128 * 0_1000000).fixed_div_floor(
                    &e,
                    &starting_b_token_supply,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(new_emis_res_data.last_time, 10001000);
            assert_eq!(new_emis_res_data.index, new_index);
            let user_emis_data =
                storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
            let new_accrual = 0
                + (new_index - emis_user_data.index).fixed_mul_floor(
                    &e,
                    &1000,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(user_emis_data.accrued, new_accrual);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1217)")]
    fn test_remove_collateral_zero_burn() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_collateral(0), 0);

            user.add_collateral(&e, &mut reserve_0, 123);
            assert_eq!(user.get_collateral(0), 123);

            user.remove_collateral(&e, &mut reserve_0, 0);
        });
    }

    #[test]
    fn test_remove_collateral_accrues_emissions() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        e.ledger().set(LedgerInfo {
            protocol_version: 22,
            sequence_number: 1,
            timestamp: 10001000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_b_token_supply = reserve_0.data.b_supply;

        let emis_res_data = ReserveEmissionData {
            expiration: 20000000,
            eps: 0_10000000000000,
            index: 10000000000,
            last_time: 10000000, // 1000s elapsed
        };
        let emis_user_data = UserEmissionData {
            index: 9000000000,
            accrued: 0,
        };

        let mut user = User {
            address: samwise.clone(),
            positions: Positions {
                liabilities: map![&e],
                collateral: map![&e, (reserve_0.config.index, 700)],
                supply: map![&e, (reserve_0.config.index, 300)],
            },
        };
        e.as_contract(&pool, || {
            let res_0_d_token_index = reserve_0.config.index * 2 + 1;
            storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
            storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

            user.remove_collateral(&e, &mut reserve_0, 123);
            assert_eq!(user.get_collateral(0), 577);
            assert_eq!(reserve_0.data.b_supply, starting_b_token_supply - 123);

            let new_emis_res_data = storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
            let new_index = 10000000000
                + (1000i128 * 0_1000000).fixed_div_floor(
                    &e,
                    &starting_b_token_supply,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(new_emis_res_data.last_time, 10001000);
            assert_eq!(new_emis_res_data.index, new_index);
            let user_emis_data =
                storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
            let new_accrual = 0
                + (new_index - emis_user_data.index).fixed_mul_floor(
                    &e,
                    &1000,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(user_emis_data.accrued, new_accrual);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_remove_collateral_over_balance_panics() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            user.add_collateral(&e, &mut reserve_0, 123);
            assert_eq!(user.get_collateral(0), 123);

            user.remove_collateral(&e, &mut reserve_0, 124);
        });
    }

    #[test]
    fn test_supply() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_b_supply_0 = reserve_0.data.b_supply;

        let mut reserve_1 = testutils::default_reserve(&e);
        reserve_1.config.index = 1;
        let starting_b_supply_1 = reserve_1.data.b_supply;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_supply(0), 0);

            user.add_supply(&e, &mut reserve_0, 123);
            assert_eq!(user.get_supply(0), 123);
            assert_eq!(reserve_0.data.b_supply, starting_b_supply_0 + 123);

            user.add_supply(&e, &mut reserve_1, 456);
            assert_eq!(user.get_supply(0), 123);
            assert_eq!(user.get_supply(1), 456);
            assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 456);

            user.remove_supply(&e, &mut reserve_1, 100);
            assert_eq!(user.get_supply(1), 356);
            assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 356);

            user.remove_supply(&e, &mut reserve_1, 356);
            assert_eq!(user.get_supply(2), 0);
            assert_eq!(user.positions.supply.len(), 1);
            assert_eq!(reserve_1.data.b_supply, starting_b_supply_1);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1216)")]
    fn test_add_supply_zero_mint() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_supply(0), 0);

            user.add_supply(&e, &mut reserve_0, 0);
        });
    }

    #[test]
    fn test_add_supply_accrues_emissions() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        e.ledger().set(LedgerInfo {
            protocol_version: 22,
            sequence_number: 1,
            timestamp: 10001000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_b_token_supply = reserve_0.data.b_supply;

        let emis_res_data = ReserveEmissionData {
            expiration: 20000000,
            eps: 0_10000000000000,
            index: 10000000000,
            last_time: 10000000, // 1000s elapsed
        };
        let emis_user_data = UserEmissionData {
            index: 9000000000,
            accrued: 0,
        };

        let mut user = User {
            address: samwise.clone(),
            positions: Positions {
                liabilities: map![&e],
                collateral: map![&e, (reserve_0.config.index, 700)],
                supply: map![&e, (reserve_0.config.index, 300)],
            },
        };
        e.as_contract(&pool, || {
            let res_0_d_token_index = reserve_0.config.index * 2 + 1;
            storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
            storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

            user.add_supply(&e, &mut reserve_0, 123);
            assert_eq!(user.get_supply(0), 423);
            assert_eq!(reserve_0.data.b_supply, starting_b_token_supply + 123);

            let new_emis_res_data = storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
            let new_index = 10000000000
                + (1000i128 * 0_1000000).fixed_div_floor(
                    &e,
                    &starting_b_token_supply,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(new_emis_res_data.last_time, 10001000);
            assert_eq!(new_emis_res_data.index, new_index);
            let user_emis_data =
                storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
            let new_accrual = 0
                + (new_index - emis_user_data.index).fixed_mul_floor(
                    &e,
                    &1000,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(user_emis_data.accrued, new_accrual);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1217)")]
    fn test_remove_supply_zero_burn() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            assert_eq!(user.get_supply(0), 0);

            user.add_supply(&e, &mut reserve_0, 123);
            assert_eq!(user.get_supply(0), 123);

            user.remove_supply(&e, &mut reserve_0, 0);
        });
    }

    #[test]
    fn test_remove_supply_accrues_emissions() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        e.ledger().set(LedgerInfo {
            protocol_version: 22,
            sequence_number: 1,
            timestamp: 10001000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let mut reserve_0 = testutils::default_reserve(&e);
        let starting_b_token_supply = reserve_0.data.b_supply;

        let emis_res_data = ReserveEmissionData {
            expiration: 20000000,
            eps: 0_10000000000000,
            index: 10000000000,
            last_time: 10000000, // 1000s elapsed
        };
        let emis_user_data = UserEmissionData {
            index: 9000000000,
            accrued: 0,
        };

        let mut user = User {
            address: samwise.clone(),
            positions: Positions {
                liabilities: map![&e],
                collateral: map![&e, (reserve_0.config.index, 700)],
                supply: map![&e, (reserve_0.config.index, 300)],
            },
        };
        e.as_contract(&pool, || {
            let res_0_d_token_index = reserve_0.config.index * 2 + 1;
            storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
            storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

            user.remove_supply(&e, &mut reserve_0, 123);
            assert_eq!(user.get_supply(0), 177);
            assert_eq!(reserve_0.data.b_supply, starting_b_token_supply - 123);

            let new_emis_res_data = storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
            let new_index = 10000000000
                + (1000i128 * 0_1000000).fixed_div_floor(
                    &e,
                    &starting_b_token_supply,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(new_emis_res_data.last_time, 10001000);
            assert_eq!(new_emis_res_data.index, new_index);
            let user_emis_data =
                storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
            let new_accrual = 0
                + (new_index - emis_user_data.index).fixed_mul_floor(
                    &e,
                    &1000,
                    &(SCALAR_7 * SCALAR_7),
                );
            assert_eq!(user_emis_data.accrued, new_accrual);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_remove_supply_over_balance_panics() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            user.add_supply(&e, &mut reserve_0, 123);
            assert_eq!(user.get_supply(0), 123);

            user.remove_supply(&e, &mut reserve_0, 124);
        });
    }

    #[test]
    fn test_total_supply() {
        let e = Env::default();
        e.mock_all_auths();
        let samwise = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let mut reserve_0 = testutils::default_reserve(&e);

        let mut reserve_1 = testutils::default_reserve(&e);
        reserve_1.config.index = 1;

        let mut user = User {
            address: samwise.clone(),
            positions: Positions::env_default(&e),
        };
        e.as_contract(&pool, || {
            user.add_supply(&e, &mut reserve_0, 123);
            user.add_supply(&e, &mut reserve_1, 456);
            user.add_collateral(&e, &mut reserve_1, 789);
            assert_eq!(user.get_total_supply(0), 123);
            assert_eq!(user.get_total_supply(1), 456 + 789);
        });
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    /// Rates are symbolic u8 hundredths of SCALAR_12 (0..=2.55 x unity): a
    /// deliberate proof-domain restriction, not a protocol rate bound.
    fn rate_hundredths(u: u8) -> i128 {
        (u as i128) * SCALAR_12 / 100
    }

    fn loss_data(b_rate: i128, b_supply: i128, d_rate: i128) -> ReserveData {
        ReserveData {
            d_rate,
            b_rate,
            ir_mod: 0,
            b_supply,
            d_supply: 0,
            backstop_credit: 0,
            last_time: 0,
        }
    }

    /// C2 (docs/kani.md): the default-loss rate kernel behind
    /// `User::default_liabilities`. The post-default rate stays in
    /// [0, pre-default rate], a zero post-setoff supplier count preserves the
    /// existing rate without reaching the division, and the produced rate is
    /// exactly the ceil-rounded loss applied to the real conversion chain
    /// (independent u64 oracle; every operand and product on this domain is
    /// below 2^63, so Kani's default overflow checks discharge
    /// representability and casts are lossless).
    ///
    /// Domain: b_rate in 0..=2.55 x unity (zero included), b_supply a u8 cast
    /// (zero included), d_rate positive u8 hundredths of unity, defaulted
    /// amount a positive u8 cast. Product fit: numerators are at most
    /// 255 * 2.55e12 and 66,705 * SCALAR_12 (~6.7e16), far inside i128, so the
    /// dependency FixedPoint conversions equal the production
    /// SorobanFixedPoint fast path; the host I256 fallback is never taken on
    /// this domain and is not claimed.
    #[kani::proof]
    fn prove_default_loss_rate_bounds_and_preservation() {
        let b_rate_u: u8 = kani::any();
        let b_supply_u: u8 = kani::any();
        let d_rate_u: u8 = kani::any();
        let default_u: u8 = kani::any();
        // Liabilities and d-rates are positive in every reachable call.
        kani::assume(d_rate_u > 0 && default_u > 0);
        let b_rate = rate_hundredths(b_rate_u);
        let b_supply = b_supply_u as i128;
        let data = loss_data(b_rate, b_supply, rate_hundredths(d_rate_u));

        let after = default_b_rate(&(), &data, default_u as i128);

        // The rate never increases and never becomes negative (clamped at zero).
        assert!(after >= 0 && after <= b_rate);
        // With no supplier claims the existing rate is preserved and the
        // division is unreachable, so a zero denominator cannot be hit.
        if b_supply <= 0 {
            assert!(after == b_rate);
        } else {
            // Exact accepted calculation, computed independently in u64.
            let d_rate_o = d_rate_u as u64 * (SCALAR_12 as u64) / 100;
            let debt_assets = (default_u as u64 * d_rate_o).div_ceil(SCALAR_12 as u64);
            let loss = (debt_assets * SCALAR_12 as u64).div_ceil(b_supply_u as u64);
            let expected = b_rate - loss as i128;
            assert!(after == if expected < 0 { 0 } else { expected });
        }
        kani::cover!(b_supply == 0);
        kani::cover!(b_supply > 0 && after == 0);
        kani::cover!(b_supply > 0 && b_rate == 0 && after == 0);
        kani::cover!(b_supply > 0 && after > 0 && after < b_rate);
    }

    /// C3: the actual outstanding-share subtraction used by remove_supply
    /// (including gulp) and remove_collateral. This proves the scalar body,
    /// conditional on invocation; caller guards and host effects are separate.
    #[kani::proof]
    fn prove_outstanding_supply_burn_delta() {
        let before: i128 = kani::any();
        let amount: i128 = kani::any();
        kani::assume(before >= 0 && amount >= 0 && amount <= before);
        let mut data = loss_data(0, before, 0);

        data.burn_supply(amount);

        assert!(data.b_supply >= 0 && data.b_supply <= before);
        assert!(data.b_supply == before - amount);
        assert!(data.b_supply + amount == before);
        kani::cover!(amount == 0 && before > 0);
        kani::cover!(amount == before && before > 0);
    }

    /// C2 S3 prerequisite: for debt assets at most 651 the actual unit
    /// ceil equals the unchanged C2 u64 loss-oracle division by supplier
    /// count, with 1 <= loss <= 651 * SCALAR_12 and lossless casts.
    #[kani::proof]
    fn c2_s3_actual_unit_ceil_u64() {
        let X: u16 = kani::any();
        let s: u8 = kani::any();
        kani::assume(X >= 1 && X <= 651 && s >= 1);
        // This type-checked binding selects the real () FixedMath ceiling
        // dependency implementation; no proof-local arithmetic replaces it.
        let _: fn(&(), i128, i128, i128) -> i128 = FixedMath::ceil;
        let loss = FixedMath::ceil(&(), X as i128, SCALAR_12, s as i128);

        let numerator = X as u64 * (SCALAR_12 as u64);
        assert_eq!(SCALAR_12 as u64 as i128, SCALAR_12);
        assert_eq!(s as u64 as i128, s as i128);
        assert_eq!(numerator as i128, X as i128 * SCALAR_12);
        assert!(loss >= 1 && loss <= 651 * SCALAR_12);
        assert_eq!(loss as u64, numerator.div_ceil(s as u64));
        assert_eq!(
            numerator.div_ceil(s as u64) as i128 as u64,
            numerator.div_ceil(s as u64)
        );
        kani::cover!(X == 1 && s == 1);
        kani::cover!(X == 651 && s == 255);
    }

    #[kani::proof]
    fn prove_default_loss_rate_witnesses() {
        // (b_supply, b_rate_hundredths, expected_after) over D=100:
        // zero supply preserves the rate; s=1 tuples clamp to zero
        // because ceil(100*S/1)=100*S exceeds each pre-default rate;
        // (200,100) exercises the strict positive decrease, since
        // ceil(100S/200)=5e11 leaves after = S - 5e11 = 5e11.
        let witnesses = [
            (0u8, 100u8, SCALAR_12),
            (1u8, 100u8, 0i128),
            (1u8, 0u8, 0i128),
            (200u8, 100u8, 500_000_000_000i128),
        ];
        for &(b_supply_u, b_rate_h, expected) in witnesses.iter() {
            let data = loss_data(
                rate_hundredths(b_rate_h),
                b_supply_u as i128,
                rate_hundredths(100u8),
            );
            let after = default_b_rate(&(), &data, 100i128);
            assert_eq!(after, expected);
        }
    }

    #[kani::proof]
    fn c2_s4_guarded_loss_clamp() {
        let b_supply_u: u8 = kani::any();
        let d_rate_h: u8 = kani::any();
        let b_rate_h: u8 = kani::any();
        let defaulted_u: u8 = kani::any();
        // Original reachable-domain gate: defaulted liability strictly
        // positive; supplier count and pre-default rate span full reach.
        kani::assume(d_rate_h > 0 && defaulted_u > 0);
        if b_supply_u == 0 {
            // Zero-supplier branch: no arithmetic may be constructed or
            // consumed. This guard rejects every ceil call outright so any
            // accidental kernel division becomes a Kani-observed panic.
            struct RejectAll;
            impl FixedMath for RejectAll {
                fn floor(&self, _: i128, _: i128, _: i128) -> i128 {
                    panic!("zero-supply path must not divide")
                }
                fn ceil(&self, _: i128, _: i128, _: i128) -> i128 {
                    panic!("zero-supply path must not ceil")
                }
            }
            let data = loss_data(rate_hundredths(b_rate_h), 0i128, rate_hundredths(d_rate_h));
            assert_eq!(
                default_b_rate(&RejectAll, &data, defaulted_u as i128),
                rate_hundredths(b_rate_h)
            );
            kani::cover!(b_supply_u == 0);
            return;
        }
        let d_rate = rate_hundredths(d_rate_h);
        let b_rate = rate_hundredths(b_rate_h);
        let data = loss_data(b_rate, b_supply_u as i128, d_rate);
        // Positive-supply branch: bind helper outputs as symbolic
        // composition parameters with proof-local implications to the
        // separately proved forward/unit-ceil numeric forms; nothing is
        // assumed about the zero-supplier branch because it returns above.
        let D: i128 = kani::any();
        kani::assume(D >= 1 && D <= 651);
        // Composition-level binding (proved at c2_s0_d_forward_exact_u64):
        // for this domain D == ceil(defaulted_u*d_rate/S) via the original
        // u64 oracle (default_u*d_rate_o).div_ceil(S) with
        // d_rate_o == d_rate_h*SCALAR_12/100.
        let L: i128 = kani::any();
        kani::assume(L >= 1 && L <= 651 * SCALAR_12);
        // Composition-level binding (proved at c2_s3_actual_unit_ceil_u64):
        // for this domain L == (D*SCALAR_12).div_ceil(b_supply_u).
        // Exact operand sequence guard with seeded tuples/results so every
        // field is a harness-local construct, not an E0434 closure capture.
        struct Guard {
            calls: core::cell::Cell<usize>,
            first_args: (i128, i128, i128),
            second_args: (i128, i128, i128),
            first_result: i128,
            second_result: i128,
        }
        impl FixedMath for Guard {
            fn floor(&self, _: i128, _: i128, _: i128) -> i128 {
                panic!("loss path must not floor")
            }

            fn ceil(&self, x: i128, y: i128, d: i128) -> i128 {
                let idx = self.calls.get();
                if idx == 0 {
                    assert_eq!((x, y, d), self.first_args);
                    self.calls.set(1);
                    self.first_result
                } else if idx == 1 {
                    assert_eq!((x, y, d), self.second_args);
                    self.calls.set(idx + 1);
                    self.second_result
                } else {
                    panic!("unexpected extra ceil call in loss path");
                }
            }
        }
        let math = Guard {
            calls: core::cell::Cell::from(0usize),
            first_args: (defaulted_u as i128, d_rate, SCALAR_12),
            second_args: (D, SCALAR_12, b_supply_u as i128),
            first_result: D,
            second_result: L,
        };
        let after = default_b_rate(&math, &data, defaulted_u as i128);
        assert_eq!(math.calls.get(), 2);
        // Original accepted policy and bounds retained verbatim.
        let expected = b_rate - L;
        assert_eq!(after, if expected < 0 { 0 } else { expected });
        assert!(after >= 0 && after <= b_rate);
        kani::cover!(b_supply_u > 0 && after == 0);
        kani::cover!(b_supply_u > 0 && b_rate == 0 && after == 0);
        kani::cover!(b_supply_u > 0 && after > 0 && after < b_rate);
    }

    // The union over S=1..=255 equals the original S3 domain; X and
    // every arithmetic/oracle operation are unchanged. Three canaries
    // calibrate fixed divisors only and do not close that full union.
    fn c2_s3_fixed_supplier<const S: u8>() {
        let X: u16 = kani::any();
        let s: u8 = S;
        assert!(S >= 1);
        kani::assume(X >= 1 && X <= 651 && s >= 1);
        // This type-checked binding selects the real () FixedMath ceiling
        // dependency implementation; no proof-local arithmetic replaces it.
        let _: fn(&(), i128, i128, i128) -> i128 = FixedMath::ceil;
        let loss = FixedMath::ceil(&(), X as i128, SCALAR_12, s as i128);

        let numerator = X as u64 * (SCALAR_12 as u64);
        assert_eq!(SCALAR_12 as u64 as i128, SCALAR_12);
        assert_eq!(s as u64 as i128, s as i128);
        assert_eq!(numerator as i128, X as i128 * SCALAR_12);
        assert!(loss >= 1 && loss <= 651 * SCALAR_12);
        assert_eq!(loss as u64, numerator.div_ceil(s as u64));
        assert_eq!(
            numerator.div_ceil(s as u64) as i128 as u64,
            numerator.div_ceil(s as u64)
        );
        kani::cover!(X == 1);
        kani::cover!(X == 651);
    }

    macro_rules! proof_case {
        ($name:ident, $helper:ident, $value:literal) => {
            #[kani::proof]
            fn $name() {
                $helper::<$value>();
            }
        };
    }

    proof_case!(c2_s3_fixed_supplier_1, c2_s3_fixed_supplier, 1);

    proof_case!(c2_s3_fixed_supplier_3, c2_s3_fixed_supplier, 3);

    proof_case!(c2_s3_fixed_supplier_255, c2_s3_fixed_supplier, 255);
    proof_case!(c2_s3_fixed_supplier_2, c2_s3_fixed_supplier, 2);

    proof_case!(c2_s3_fixed_supplier_4, c2_s3_fixed_supplier, 4);

    proof_case!(c2_s3_fixed_supplier_5, c2_s3_fixed_supplier, 5);

    proof_case!(c2_s3_fixed_supplier_6, c2_s3_fixed_supplier, 6);

    proof_case!(c2_s3_fixed_supplier_7, c2_s3_fixed_supplier, 7);

    proof_case!(c2_s3_fixed_supplier_8, c2_s3_fixed_supplier, 8);

    proof_case!(c2_s3_fixed_supplier_9, c2_s3_fixed_supplier, 9);

    proof_case!(c2_s3_fixed_supplier_10, c2_s3_fixed_supplier, 10);

    proof_case!(c2_s3_fixed_supplier_11, c2_s3_fixed_supplier, 11);

    proof_case!(c2_s3_fixed_supplier_12, c2_s3_fixed_supplier, 12);

    proof_case!(c2_s3_fixed_supplier_13, c2_s3_fixed_supplier, 13);

    proof_case!(c2_s3_fixed_supplier_14, c2_s3_fixed_supplier, 14);

    proof_case!(c2_s3_fixed_supplier_15, c2_s3_fixed_supplier, 15);

    proof_case!(c2_s3_fixed_supplier_16, c2_s3_fixed_supplier, 16);

    proof_case!(c2_s3_fixed_supplier_17, c2_s3_fixed_supplier, 17);

    proof_case!(c2_s3_fixed_supplier_18, c2_s3_fixed_supplier, 18);

    proof_case!(c2_s3_fixed_supplier_19, c2_s3_fixed_supplier, 19);

    proof_case!(c2_s3_fixed_supplier_20, c2_s3_fixed_supplier, 20);

    proof_case!(c2_s3_fixed_supplier_21, c2_s3_fixed_supplier, 21);

    proof_case!(c2_s3_fixed_supplier_22, c2_s3_fixed_supplier, 22);

    proof_case!(c2_s3_fixed_supplier_23, c2_s3_fixed_supplier, 23);

    proof_case!(c2_s3_fixed_supplier_24, c2_s3_fixed_supplier, 24);

    proof_case!(c2_s3_fixed_supplier_25, c2_s3_fixed_supplier, 25);

    proof_case!(c2_s3_fixed_supplier_26, c2_s3_fixed_supplier, 26);

    proof_case!(c2_s3_fixed_supplier_27, c2_s3_fixed_supplier, 27);

    proof_case!(c2_s3_fixed_supplier_28, c2_s3_fixed_supplier, 28);

    proof_case!(c2_s3_fixed_supplier_29, c2_s3_fixed_supplier, 29);

    proof_case!(c2_s3_fixed_supplier_30, c2_s3_fixed_supplier, 30);

    proof_case!(c2_s3_fixed_supplier_31, c2_s3_fixed_supplier, 31);

    proof_case!(c2_s3_fixed_supplier_32, c2_s3_fixed_supplier, 32);

    proof_case!(c2_s3_fixed_supplier_33, c2_s3_fixed_supplier, 33);

    proof_case!(c2_s3_fixed_supplier_34, c2_s3_fixed_supplier, 34);

    proof_case!(c2_s3_fixed_supplier_35, c2_s3_fixed_supplier, 35);

    proof_case!(c2_s3_fixed_supplier_36, c2_s3_fixed_supplier, 36);

    proof_case!(c2_s3_fixed_supplier_37, c2_s3_fixed_supplier, 37);

    proof_case!(c2_s3_fixed_supplier_38, c2_s3_fixed_supplier, 38);

    proof_case!(c2_s3_fixed_supplier_39, c2_s3_fixed_supplier, 39);

    proof_case!(c2_s3_fixed_supplier_40, c2_s3_fixed_supplier, 40);

    proof_case!(c2_s3_fixed_supplier_41, c2_s3_fixed_supplier, 41);

    proof_case!(c2_s3_fixed_supplier_42, c2_s3_fixed_supplier, 42);

    proof_case!(c2_s3_fixed_supplier_43, c2_s3_fixed_supplier, 43);

    proof_case!(c2_s3_fixed_supplier_44, c2_s3_fixed_supplier, 44);

    proof_case!(c2_s3_fixed_supplier_45, c2_s3_fixed_supplier, 45);

    proof_case!(c2_s3_fixed_supplier_46, c2_s3_fixed_supplier, 46);

    proof_case!(c2_s3_fixed_supplier_47, c2_s3_fixed_supplier, 47);

    proof_case!(c2_s3_fixed_supplier_48, c2_s3_fixed_supplier, 48);

    proof_case!(c2_s3_fixed_supplier_49, c2_s3_fixed_supplier, 49);

    proof_case!(c2_s3_fixed_supplier_50, c2_s3_fixed_supplier, 50);

    proof_case!(c2_s3_fixed_supplier_51, c2_s3_fixed_supplier, 51);

    proof_case!(c2_s3_fixed_supplier_52, c2_s3_fixed_supplier, 52);

    proof_case!(c2_s3_fixed_supplier_53, c2_s3_fixed_supplier, 53);

    proof_case!(c2_s3_fixed_supplier_54, c2_s3_fixed_supplier, 54);

    proof_case!(c2_s3_fixed_supplier_55, c2_s3_fixed_supplier, 55);

    proof_case!(c2_s3_fixed_supplier_56, c2_s3_fixed_supplier, 56);

    proof_case!(c2_s3_fixed_supplier_57, c2_s3_fixed_supplier, 57);

    proof_case!(c2_s3_fixed_supplier_58, c2_s3_fixed_supplier, 58);

    proof_case!(c2_s3_fixed_supplier_59, c2_s3_fixed_supplier, 59);

    proof_case!(c2_s3_fixed_supplier_60, c2_s3_fixed_supplier, 60);

    proof_case!(c2_s3_fixed_supplier_61, c2_s3_fixed_supplier, 61);

    proof_case!(c2_s3_fixed_supplier_62, c2_s3_fixed_supplier, 62);

    proof_case!(c2_s3_fixed_supplier_63, c2_s3_fixed_supplier, 63);

    proof_case!(c2_s3_fixed_supplier_64, c2_s3_fixed_supplier, 64);

    proof_case!(c2_s3_fixed_supplier_65, c2_s3_fixed_supplier, 65);

    proof_case!(c2_s3_fixed_supplier_66, c2_s3_fixed_supplier, 66);

    proof_case!(c2_s3_fixed_supplier_67, c2_s3_fixed_supplier, 67);

    proof_case!(c2_s3_fixed_supplier_68, c2_s3_fixed_supplier, 68);

    proof_case!(c2_s3_fixed_supplier_69, c2_s3_fixed_supplier, 69);

    proof_case!(c2_s3_fixed_supplier_70, c2_s3_fixed_supplier, 70);

    proof_case!(c2_s3_fixed_supplier_71, c2_s3_fixed_supplier, 71);

    proof_case!(c2_s3_fixed_supplier_72, c2_s3_fixed_supplier, 72);

    proof_case!(c2_s3_fixed_supplier_73, c2_s3_fixed_supplier, 73);

    proof_case!(c2_s3_fixed_supplier_74, c2_s3_fixed_supplier, 74);

    proof_case!(c2_s3_fixed_supplier_75, c2_s3_fixed_supplier, 75);

    proof_case!(c2_s3_fixed_supplier_76, c2_s3_fixed_supplier, 76);

    proof_case!(c2_s3_fixed_supplier_77, c2_s3_fixed_supplier, 77);

    proof_case!(c2_s3_fixed_supplier_78, c2_s3_fixed_supplier, 78);

    proof_case!(c2_s3_fixed_supplier_79, c2_s3_fixed_supplier, 79);

    proof_case!(c2_s3_fixed_supplier_80, c2_s3_fixed_supplier, 80);

    proof_case!(c2_s3_fixed_supplier_81, c2_s3_fixed_supplier, 81);

    proof_case!(c2_s3_fixed_supplier_82, c2_s3_fixed_supplier, 82);

    proof_case!(c2_s3_fixed_supplier_83, c2_s3_fixed_supplier, 83);

    proof_case!(c2_s3_fixed_supplier_84, c2_s3_fixed_supplier, 84);

    proof_case!(c2_s3_fixed_supplier_85, c2_s3_fixed_supplier, 85);

    proof_case!(c2_s3_fixed_supplier_86, c2_s3_fixed_supplier, 86);

    proof_case!(c2_s3_fixed_supplier_87, c2_s3_fixed_supplier, 87);

    proof_case!(c2_s3_fixed_supplier_88, c2_s3_fixed_supplier, 88);

    proof_case!(c2_s3_fixed_supplier_89, c2_s3_fixed_supplier, 89);

    proof_case!(c2_s3_fixed_supplier_90, c2_s3_fixed_supplier, 90);

    proof_case!(c2_s3_fixed_supplier_91, c2_s3_fixed_supplier, 91);

    proof_case!(c2_s3_fixed_supplier_92, c2_s3_fixed_supplier, 92);

    proof_case!(c2_s3_fixed_supplier_93, c2_s3_fixed_supplier, 93);

    proof_case!(c2_s3_fixed_supplier_94, c2_s3_fixed_supplier, 94);

    proof_case!(c2_s3_fixed_supplier_95, c2_s3_fixed_supplier, 95);

    proof_case!(c2_s3_fixed_supplier_96, c2_s3_fixed_supplier, 96);

    proof_case!(c2_s3_fixed_supplier_97, c2_s3_fixed_supplier, 97);

    proof_case!(c2_s3_fixed_supplier_98, c2_s3_fixed_supplier, 98);

    proof_case!(c2_s3_fixed_supplier_99, c2_s3_fixed_supplier, 99);

    proof_case!(c2_s3_fixed_supplier_100, c2_s3_fixed_supplier, 100);

    proof_case!(c2_s3_fixed_supplier_101, c2_s3_fixed_supplier, 101);

    proof_case!(c2_s3_fixed_supplier_102, c2_s3_fixed_supplier, 102);

    proof_case!(c2_s3_fixed_supplier_103, c2_s3_fixed_supplier, 103);

    proof_case!(c2_s3_fixed_supplier_104, c2_s3_fixed_supplier, 104);

    proof_case!(c2_s3_fixed_supplier_105, c2_s3_fixed_supplier, 105);

    proof_case!(c2_s3_fixed_supplier_106, c2_s3_fixed_supplier, 106);

    proof_case!(c2_s3_fixed_supplier_107, c2_s3_fixed_supplier, 107);

    proof_case!(c2_s3_fixed_supplier_108, c2_s3_fixed_supplier, 108);

    proof_case!(c2_s3_fixed_supplier_109, c2_s3_fixed_supplier, 109);

    proof_case!(c2_s3_fixed_supplier_110, c2_s3_fixed_supplier, 110);

    proof_case!(c2_s3_fixed_supplier_111, c2_s3_fixed_supplier, 111);

    proof_case!(c2_s3_fixed_supplier_112, c2_s3_fixed_supplier, 112);

    proof_case!(c2_s3_fixed_supplier_113, c2_s3_fixed_supplier, 113);

    proof_case!(c2_s3_fixed_supplier_114, c2_s3_fixed_supplier, 114);

    proof_case!(c2_s3_fixed_supplier_115, c2_s3_fixed_supplier, 115);

    proof_case!(c2_s3_fixed_supplier_116, c2_s3_fixed_supplier, 116);

    proof_case!(c2_s3_fixed_supplier_117, c2_s3_fixed_supplier, 117);

    proof_case!(c2_s3_fixed_supplier_118, c2_s3_fixed_supplier, 118);

    proof_case!(c2_s3_fixed_supplier_119, c2_s3_fixed_supplier, 119);

    proof_case!(c2_s3_fixed_supplier_120, c2_s3_fixed_supplier, 120);

    proof_case!(c2_s3_fixed_supplier_121, c2_s3_fixed_supplier, 121);

    proof_case!(c2_s3_fixed_supplier_122, c2_s3_fixed_supplier, 122);

    proof_case!(c2_s3_fixed_supplier_123, c2_s3_fixed_supplier, 123);

    proof_case!(c2_s3_fixed_supplier_124, c2_s3_fixed_supplier, 124);

    proof_case!(c2_s3_fixed_supplier_125, c2_s3_fixed_supplier, 125);

    proof_case!(c2_s3_fixed_supplier_126, c2_s3_fixed_supplier, 126);

    proof_case!(c2_s3_fixed_supplier_127, c2_s3_fixed_supplier, 127);

    proof_case!(c2_s3_fixed_supplier_128, c2_s3_fixed_supplier, 128);

    proof_case!(c2_s3_fixed_supplier_129, c2_s3_fixed_supplier, 129);

    proof_case!(c2_s3_fixed_supplier_130, c2_s3_fixed_supplier, 130);

    proof_case!(c2_s3_fixed_supplier_131, c2_s3_fixed_supplier, 131);

    proof_case!(c2_s3_fixed_supplier_132, c2_s3_fixed_supplier, 132);

    proof_case!(c2_s3_fixed_supplier_133, c2_s3_fixed_supplier, 133);

    proof_case!(c2_s3_fixed_supplier_134, c2_s3_fixed_supplier, 134);

    proof_case!(c2_s3_fixed_supplier_135, c2_s3_fixed_supplier, 135);

    proof_case!(c2_s3_fixed_supplier_136, c2_s3_fixed_supplier, 136);

    proof_case!(c2_s3_fixed_supplier_137, c2_s3_fixed_supplier, 137);

    proof_case!(c2_s3_fixed_supplier_138, c2_s3_fixed_supplier, 138);

    proof_case!(c2_s3_fixed_supplier_139, c2_s3_fixed_supplier, 139);

    proof_case!(c2_s3_fixed_supplier_140, c2_s3_fixed_supplier, 140);

    proof_case!(c2_s3_fixed_supplier_141, c2_s3_fixed_supplier, 141);

    proof_case!(c2_s3_fixed_supplier_142, c2_s3_fixed_supplier, 142);

    proof_case!(c2_s3_fixed_supplier_143, c2_s3_fixed_supplier, 143);

    proof_case!(c2_s3_fixed_supplier_144, c2_s3_fixed_supplier, 144);

    proof_case!(c2_s3_fixed_supplier_145, c2_s3_fixed_supplier, 145);

    proof_case!(c2_s3_fixed_supplier_146, c2_s3_fixed_supplier, 146);

    proof_case!(c2_s3_fixed_supplier_147, c2_s3_fixed_supplier, 147);

    proof_case!(c2_s3_fixed_supplier_148, c2_s3_fixed_supplier, 148);

    proof_case!(c2_s3_fixed_supplier_149, c2_s3_fixed_supplier, 149);

    proof_case!(c2_s3_fixed_supplier_150, c2_s3_fixed_supplier, 150);

    proof_case!(c2_s3_fixed_supplier_151, c2_s3_fixed_supplier, 151);

    proof_case!(c2_s3_fixed_supplier_152, c2_s3_fixed_supplier, 152);

    proof_case!(c2_s3_fixed_supplier_153, c2_s3_fixed_supplier, 153);

    proof_case!(c2_s3_fixed_supplier_154, c2_s3_fixed_supplier, 154);

    proof_case!(c2_s3_fixed_supplier_155, c2_s3_fixed_supplier, 155);

    proof_case!(c2_s3_fixed_supplier_156, c2_s3_fixed_supplier, 156);

    proof_case!(c2_s3_fixed_supplier_157, c2_s3_fixed_supplier, 157);

    proof_case!(c2_s3_fixed_supplier_158, c2_s3_fixed_supplier, 158);

    proof_case!(c2_s3_fixed_supplier_159, c2_s3_fixed_supplier, 159);

    proof_case!(c2_s3_fixed_supplier_160, c2_s3_fixed_supplier, 160);

    proof_case!(c2_s3_fixed_supplier_161, c2_s3_fixed_supplier, 161);

    proof_case!(c2_s3_fixed_supplier_162, c2_s3_fixed_supplier, 162);

    proof_case!(c2_s3_fixed_supplier_163, c2_s3_fixed_supplier, 163);

    proof_case!(c2_s3_fixed_supplier_164, c2_s3_fixed_supplier, 164);

    proof_case!(c2_s3_fixed_supplier_165, c2_s3_fixed_supplier, 165);

    proof_case!(c2_s3_fixed_supplier_166, c2_s3_fixed_supplier, 166);

    proof_case!(c2_s3_fixed_supplier_167, c2_s3_fixed_supplier, 167);

    proof_case!(c2_s3_fixed_supplier_168, c2_s3_fixed_supplier, 168);

    proof_case!(c2_s3_fixed_supplier_169, c2_s3_fixed_supplier, 169);

    proof_case!(c2_s3_fixed_supplier_170, c2_s3_fixed_supplier, 170);

    proof_case!(c2_s3_fixed_supplier_171, c2_s3_fixed_supplier, 171);

    proof_case!(c2_s3_fixed_supplier_172, c2_s3_fixed_supplier, 172);

    proof_case!(c2_s3_fixed_supplier_173, c2_s3_fixed_supplier, 173);

    proof_case!(c2_s3_fixed_supplier_174, c2_s3_fixed_supplier, 174);

    proof_case!(c2_s3_fixed_supplier_175, c2_s3_fixed_supplier, 175);

    proof_case!(c2_s3_fixed_supplier_176, c2_s3_fixed_supplier, 176);

    proof_case!(c2_s3_fixed_supplier_177, c2_s3_fixed_supplier, 177);

    proof_case!(c2_s3_fixed_supplier_178, c2_s3_fixed_supplier, 178);

    proof_case!(c2_s3_fixed_supplier_179, c2_s3_fixed_supplier, 179);

    proof_case!(c2_s3_fixed_supplier_180, c2_s3_fixed_supplier, 180);

    proof_case!(c2_s3_fixed_supplier_181, c2_s3_fixed_supplier, 181);

    proof_case!(c2_s3_fixed_supplier_182, c2_s3_fixed_supplier, 182);

    proof_case!(c2_s3_fixed_supplier_183, c2_s3_fixed_supplier, 183);

    proof_case!(c2_s3_fixed_supplier_184, c2_s3_fixed_supplier, 184);

    proof_case!(c2_s3_fixed_supplier_185, c2_s3_fixed_supplier, 185);

    proof_case!(c2_s3_fixed_supplier_186, c2_s3_fixed_supplier, 186);

    proof_case!(c2_s3_fixed_supplier_187, c2_s3_fixed_supplier, 187);

    proof_case!(c2_s3_fixed_supplier_188, c2_s3_fixed_supplier, 188);

    proof_case!(c2_s3_fixed_supplier_189, c2_s3_fixed_supplier, 189);

    proof_case!(c2_s3_fixed_supplier_190, c2_s3_fixed_supplier, 190);

    proof_case!(c2_s3_fixed_supplier_191, c2_s3_fixed_supplier, 191);

    proof_case!(c2_s3_fixed_supplier_192, c2_s3_fixed_supplier, 192);

    proof_case!(c2_s3_fixed_supplier_193, c2_s3_fixed_supplier, 193);

    proof_case!(c2_s3_fixed_supplier_194, c2_s3_fixed_supplier, 194);

    proof_case!(c2_s3_fixed_supplier_195, c2_s3_fixed_supplier, 195);

    proof_case!(c2_s3_fixed_supplier_196, c2_s3_fixed_supplier, 196);

    proof_case!(c2_s3_fixed_supplier_197, c2_s3_fixed_supplier, 197);

    proof_case!(c2_s3_fixed_supplier_198, c2_s3_fixed_supplier, 198);

    proof_case!(c2_s3_fixed_supplier_199, c2_s3_fixed_supplier, 199);

    proof_case!(c2_s3_fixed_supplier_200, c2_s3_fixed_supplier, 200);

    proof_case!(c2_s3_fixed_supplier_201, c2_s3_fixed_supplier, 201);

    proof_case!(c2_s3_fixed_supplier_202, c2_s3_fixed_supplier, 202);

    proof_case!(c2_s3_fixed_supplier_203, c2_s3_fixed_supplier, 203);

    proof_case!(c2_s3_fixed_supplier_204, c2_s3_fixed_supplier, 204);

    proof_case!(c2_s3_fixed_supplier_205, c2_s3_fixed_supplier, 205);

    proof_case!(c2_s3_fixed_supplier_206, c2_s3_fixed_supplier, 206);

    proof_case!(c2_s3_fixed_supplier_207, c2_s3_fixed_supplier, 207);

    proof_case!(c2_s3_fixed_supplier_208, c2_s3_fixed_supplier, 208);

    proof_case!(c2_s3_fixed_supplier_209, c2_s3_fixed_supplier, 209);

    proof_case!(c2_s3_fixed_supplier_210, c2_s3_fixed_supplier, 210);

    proof_case!(c2_s3_fixed_supplier_211, c2_s3_fixed_supplier, 211);

    proof_case!(c2_s3_fixed_supplier_212, c2_s3_fixed_supplier, 212);

    proof_case!(c2_s3_fixed_supplier_213, c2_s3_fixed_supplier, 213);

    proof_case!(c2_s3_fixed_supplier_214, c2_s3_fixed_supplier, 214);

    proof_case!(c2_s3_fixed_supplier_215, c2_s3_fixed_supplier, 215);

    proof_case!(c2_s3_fixed_supplier_216, c2_s3_fixed_supplier, 216);

    proof_case!(c2_s3_fixed_supplier_217, c2_s3_fixed_supplier, 217);

    proof_case!(c2_s3_fixed_supplier_218, c2_s3_fixed_supplier, 218);

    proof_case!(c2_s3_fixed_supplier_219, c2_s3_fixed_supplier, 219);

    proof_case!(c2_s3_fixed_supplier_220, c2_s3_fixed_supplier, 220);

    proof_case!(c2_s3_fixed_supplier_221, c2_s3_fixed_supplier, 221);

    proof_case!(c2_s3_fixed_supplier_222, c2_s3_fixed_supplier, 222);

    proof_case!(c2_s3_fixed_supplier_223, c2_s3_fixed_supplier, 223);

    proof_case!(c2_s3_fixed_supplier_224, c2_s3_fixed_supplier, 224);

    proof_case!(c2_s3_fixed_supplier_225, c2_s3_fixed_supplier, 225);

    proof_case!(c2_s3_fixed_supplier_226, c2_s3_fixed_supplier, 226);

    proof_case!(c2_s3_fixed_supplier_227, c2_s3_fixed_supplier, 227);

    proof_case!(c2_s3_fixed_supplier_228, c2_s3_fixed_supplier, 228);

    proof_case!(c2_s3_fixed_supplier_229, c2_s3_fixed_supplier, 229);

    proof_case!(c2_s3_fixed_supplier_230, c2_s3_fixed_supplier, 230);

    proof_case!(c2_s3_fixed_supplier_231, c2_s3_fixed_supplier, 231);

    proof_case!(c2_s3_fixed_supplier_232, c2_s3_fixed_supplier, 232);

    proof_case!(c2_s3_fixed_supplier_233, c2_s3_fixed_supplier, 233);

    proof_case!(c2_s3_fixed_supplier_234, c2_s3_fixed_supplier, 234);

    proof_case!(c2_s3_fixed_supplier_235, c2_s3_fixed_supplier, 235);

    proof_case!(c2_s3_fixed_supplier_236, c2_s3_fixed_supplier, 236);

    proof_case!(c2_s3_fixed_supplier_237, c2_s3_fixed_supplier, 237);

    proof_case!(c2_s3_fixed_supplier_238, c2_s3_fixed_supplier, 238);

    proof_case!(c2_s3_fixed_supplier_239, c2_s3_fixed_supplier, 239);

    proof_case!(c2_s3_fixed_supplier_240, c2_s3_fixed_supplier, 240);

    proof_case!(c2_s3_fixed_supplier_241, c2_s3_fixed_supplier, 241);

    proof_case!(c2_s3_fixed_supplier_242, c2_s3_fixed_supplier, 242);

    proof_case!(c2_s3_fixed_supplier_243, c2_s3_fixed_supplier, 243);

    proof_case!(c2_s3_fixed_supplier_244, c2_s3_fixed_supplier, 244);

    proof_case!(c2_s3_fixed_supplier_245, c2_s3_fixed_supplier, 245);

    proof_case!(c2_s3_fixed_supplier_246, c2_s3_fixed_supplier, 246);

    proof_case!(c2_s3_fixed_supplier_247, c2_s3_fixed_supplier, 247);

    proof_case!(c2_s3_fixed_supplier_248, c2_s3_fixed_supplier, 248);

    proof_case!(c2_s3_fixed_supplier_249, c2_s3_fixed_supplier, 249);

    proof_case!(c2_s3_fixed_supplier_250, c2_s3_fixed_supplier, 250);

    proof_case!(c2_s3_fixed_supplier_251, c2_s3_fixed_supplier, 251);

    proof_case!(c2_s3_fixed_supplier_252, c2_s3_fixed_supplier, 252);

    proof_case!(c2_s3_fixed_supplier_253, c2_s3_fixed_supplier, 253);

    proof_case!(c2_s3_fixed_supplier_254, c2_s3_fixed_supplier, 254);
}
