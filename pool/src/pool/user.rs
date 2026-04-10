use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{contracttype, panic_with_error, Address, Env, Map};

use crate::{constants::SCALAR_12, emissions, storage, validator::require_nonnegative, PoolError};

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
    /// be forgiven and suppliers will lose funds.
    pub fn default_liabilities(&mut self, e: &Env, reserve: &mut Reserve, amount: i128) {
        self.remove_liabilities(e, reserve, amount);
        // determine amount of funds in underlying that have defaulted
        // and deduct them from the b_rate
        let default_amount = reserve.to_asset_from_d_token(e, amount);
        let b_rate_loss = default_amount.fixed_div_ceil(&e, &reserve.data.b_supply, &SCALAR_12);
        reserve.data.b_rate -= b_rate_loss;
        if reserve.data.b_rate < 0 {
            reserve.data.b_rate = 0;
        }
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
        reserve.data.b_supply -= amount;
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
        reserve.data.b_supply -= amount;
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
