use soroban_sdk::{map, panic_with_error, unwrap::UnwrapOptimized, vec, Address, Env, Map, Vec};

use sep_40_oracle::{Asset, PriceFeedClient};

use crate::{
    errors::PoolError,
    storage::{self, PoolConfig},
    Positions,
};

use super::reserve::Reserve;

pub struct Pool {
    pub config: PoolConfig,
    pub reserves: Map<Address, Reserve>,
    reserves_to_store: Vec<Address>,
    price_decimals: Option<u32>,
    prices: Map<Address, i128>,
}

impl Pool {
    /// Load the Pool from the ledger
    pub fn load(e: &Env) -> Self {
        let pool_config = storage::get_pool_config(e);
        Pool {
            config: pool_config,
            reserves: map![e],
            reserves_to_store: vec![e],
            price_decimals: None,
            prices: map![e],
        }
    }

    /// Load a Reserve from the ledger and update to the current ledger timestamp. Returns
    /// a cached version if it exists.
    ///
    /// ### Arguments
    /// * asset - The address of the underlying asset
    /// * store - If the reserve is expected to be stored to the ledger
    pub fn load_reserve(&mut self, e: &Env, asset: &Address, store: bool) -> Reserve {
        if store && !self.reserves_to_store.contains(asset) {
            self.reserves_to_store.push_back(asset.clone());
        }

        if let Some(reserve) = self.reserves.get(asset.clone()) {
            return reserve;
        } else {
            Reserve::load(e, &self.config, asset)
        }
    }

    /// Cache the updated reserve in the pool.
    ///
    /// ### Arguments
    /// * reserve - The updated reserve
    pub fn cache_reserve(&mut self, reserve: Reserve) {
        self.reserves.set(reserve.asset.clone(), reserve);
    }

    /// Store the cached reserves to the ledger that need to be written.
    pub fn store_cached_reserves(&self, e: &Env) {
        for address in self.reserves_to_store.iter() {
            let reserve = self
                .reserves
                .get(address)
                .unwrap_or_else(|| panic_with_error!(e, PoolError::InternalReserveNotFound));
            reserve.store(e);
        }
    }

    /// Require that the action does not violate the pool status, or panic.
    ///
    /// ### Arguments
    /// * `action_type` - The type of action being performed
    pub fn require_action_allowed(&self, e: &Env, action_type: u32) {
        // disable borrowing or auction cancellation for any non-active pool and disable supplying for any frozen pool
        if (self.config.status > 1 && (action_type == 4 || action_type == 9))
            || (self.config.status > 3 && (action_type == 2 || action_type == 0))
        {
            panic_with_error!(e, PoolError::InvalidPoolStatus);
        }
    }

    /// Require that a position does not violate the maximum number of positions, or panic.
    ///
    /// ### Arguments
    /// * `positions` - The user's positions
    /// * `previous_num` - The number of positions the user previously had
    ///
    /// ### Panics
    /// If the user has more positions than the maximum allowed and they are not
    /// decreasing their number of positions
    pub fn require_under_max(&self, e: &Env, positions: &Positions, previous_num: u32) {
        let new_num = positions.effective_count();
        if new_num > previous_num && self.config.max_positions < new_num {
            panic_with_error!(e, PoolError::MaxPositionsExceeded)
        }
    }

    /// Load the decimals of the prices for the Pool's oracle. Returns a cached version if one
    /// already exists.
    pub fn load_price_decimals(&mut self, e: &Env) -> u32 {
        if let Some(decimals) = self.price_decimals {
            return decimals;
        }
        let oracle_client = PriceFeedClient::new(e, &self.config.oracle);
        let decimals = oracle_client.decimals();
        self.price_decimals = Some(decimals);
        decimals
    }

    /// Load a price from the Pool's oracle. Returns a cached version if one already exists.
    ///
    /// ### Arguments
    /// * asset - The address of the underlying asset
    ///
    /// ### Panics
    /// If the price is invalid due to being over a day old or being less than or equal to 0
    pub fn load_price(&mut self, e: &Env, asset: &Address) -> i128 {
        if let Some(price) = self.prices.get(asset.clone()) {
            return price;
        }
        let oracle_client = PriceFeedClient::new(e, &self.config.oracle);
        let oracle_asset = Asset::Stellar(asset.clone());
        let price_data = oracle_client.lastprice(&oracle_asset).unwrap_optimized();
        if price_data.timestamp + 24 * 60 * 60 < e.ledger().timestamp() || price_data.price <= 0 {
            panic_with_error!(e, PoolError::InvalidPrice);
        }
        self.prices.set(asset.clone(), price_data.price);
        price_data.price
    }
}
