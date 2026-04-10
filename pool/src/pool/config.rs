use crate::{
    constants::{MAX_RESERVES, SCALAR_12, SCALAR_7, SECONDS_PER_WEEK},
    errors::PoolError,
    storage::{
        self, has_queued_reserve_set, PoolConfig, QueuedReserveInit, ReserveConfig, ReserveData,
    },
};
use soroban_sdk::{panic_with_error, Address, Env, String};

use super::{pool::Pool, Reserve};

/// Initialize the pool
///
/// Panics if the pool is already initialized or the arguments are invalid
#[allow(clippy::too_many_arguments)]
pub fn execute_initialize(
    e: &Env,
    admin: &Address,
    name: &String,
    oracle: &Address,
    bstop_rate: &u32,
    max_positions: &u32,
    min_collateral: &i128,
    backstop_address: &Address,
    blnd_id: &Address,
) {
    let pool_config = PoolConfig {
        oracle: oracle.clone(),
        min_collateral: *min_collateral,
        bstop_rate: *bstop_rate,
        status: 6,
        max_positions: *max_positions,
    };
    require_valid_pool_config(e, &pool_config);

    storage::set_admin(e, admin);
    storage::set_name(e, name);
    storage::set_backstop(e, backstop_address);
    storage::set_pool_config(e, &pool_config);
    storage::set_blnd_token(e, blnd_id);
}

/// Update the pool
pub fn execute_update_pool(
    e: &Env,
    backstop_take_rate: u32,
    max_positions: u32,
    min_collateral: i128,
) {
    let mut pool_config = storage::get_pool_config(e);
    let res_list = storage::get_res_list(e);
    if pool_config.bstop_rate != backstop_take_rate {
        for res in res_list {
            let reserve = Reserve::load(e, &pool_config, &res);
            reserve.store(e);
        }
    }
    pool_config.bstop_rate = backstop_take_rate;
    pool_config.max_positions = max_positions;
    pool_config.min_collateral = min_collateral;

    require_valid_pool_config(e, &pool_config);
    storage::set_pool_config(e, &pool_config);
}

/// Execute a queueing a reserve initialization for the pool
pub fn execute_queue_set_reserve(e: &Env, asset: &Address, metadata: &ReserveConfig) {
    if has_queued_reserve_set(e, asset) {
        panic_with_error!(&e, PoolError::BadRequest)
    }
    require_valid_reserve_metadata(e, metadata);

    // if the reserve config exists, ensure there are no invalid changes
    if storage::has_res(e, asset) {
        require_valid_reserve_metadata_changes(e, &storage::get_res_config(e, asset), metadata);
    }

    let mut unlock_time = e.ledger().timestamp();
    // require a timelock if pool status is not setup
    if storage::get_pool_config(e).status != 6 {
        unlock_time += SECONDS_PER_WEEK;
    }
    storage::set_queued_reserve_set(
        &e,
        &QueuedReserveInit {
            new_config: metadata.clone(),
            unlock_time,
        },
        &asset,
    );
}

/// Execute cancelling a queueing a reserve initialization for the pool
pub fn execute_cancel_queued_set_reserve(e: &Env, asset: &Address) {
    storage::del_queued_reserve_set(&e, &asset);
}

/// Execute a queued reserve initialization for the pool
pub fn execute_set_reserve(e: &Env, asset: &Address) -> u32 {
    let queued_init = storage::get_queued_reserve_set(e, asset);

    if queued_init.unlock_time > e.ledger().timestamp() {
        panic_with_error!(e, PoolError::InitNotUnlocked);
    }

    // remove queued reserve
    storage::del_queued_reserve_set(e, asset);

    // initialize reserve
    initialize_reserve(e, asset, &queued_init.new_config)
}

/// sets reserve data for the pool
fn initialize_reserve(e: &Env, asset: &Address, config: &ReserveConfig) -> u32 {
    let index: u32;
    // if reserve already exists, ensure index and scalar do not change
    if storage::has_res(e, asset) {
        // accrue and store reserve data to the ledger
        let mut pool = Pool::load(e);
        // @dev: Store the reserve to ledger manually
        let mut reserve = pool.load_reserve(e, asset, false);
        index = reserve.config.index;
        let reserve_config = storage::get_res_config(e, asset);
        require_valid_reserve_metadata_changes(e, &reserve_config, config);
        // if any of the IR parameters were changed reset the IR modifier
        if reserve_config.r_base != config.r_base
            || reserve_config.r_one != config.r_one
            || reserve_config.r_two != config.r_two
            || reserve_config.r_three != config.r_three
            || reserve_config.util != config.util
        {
            reserve.data.ir_mod = SCALAR_7;
        }
        reserve.store(e);
    } else {
        index = storage::push_res_list(e, asset);
        let init_data = ReserveData {
            b_rate: SCALAR_12,
            d_rate: SCALAR_12,
            ir_mod: SCALAR_7,
            d_supply: 0,
            b_supply: 0,
            last_time: e.ledger().timestamp(),
            backstop_credit: 0,
        };
        storage::set_res_data(e, asset, &init_data);
    }

    let reserve_config = ReserveConfig {
        index,
        decimals: config.decimals,
        c_factor: config.c_factor,
        l_factor: config.l_factor,
        util: config.util,
        max_util: config.max_util,
        r_base: config.r_base,
        r_one: config.r_one,
        r_two: config.r_two,
        r_three: config.r_three,
        reactivity: config.reactivity,
        supply_cap: config.supply_cap,
        enabled: config.enabled,
    };
    storage::set_res_config(e, asset, &reserve_config);

    index
}

#[allow(clippy::zero_prefixed_literal)]
fn require_valid_reserve_metadata(e: &Env, metadata: &ReserveConfig) {
    const SCALAR_7_U32: u32 = SCALAR_7 as u32;
    if metadata.decimals > 18
        || metadata.c_factor > SCALAR_7_U32
        || metadata.l_factor > SCALAR_7_U32
        || metadata.util > 0_9000000
        || (metadata.max_util > SCALAR_7_U32 || metadata.max_util <= metadata.util)
        || metadata.r_base >= 1_0000000
        || metadata.r_base < 0_0001000
        || (metadata.r_one > metadata.r_two || metadata.r_two > metadata.r_three)
        || (metadata.reactivity > 0_0001000)
    {
        panic_with_error!(e, PoolError::InvalidReserveMetadata);
    }
}

fn require_valid_reserve_metadata_changes(
    e: &Env,
    cur_config: &ReserveConfig,
    metadata: &ReserveConfig,
) {
    if cur_config.decimals != metadata.decimals
        || (cur_config.l_factor != 0 && metadata.l_factor == 0)
    {
        panic_with_error!(e, PoolError::InvalidReserveMetadata);
    }
}

fn require_valid_pool_config(e: &Env, config: &PoolConfig) {
    // ensure backstop is [0,1)
    if config.bstop_rate >= SCALAR_7 as u32 {
        panic_with_error!(e, PoolError::InvalidPoolConfigArgs);
    }

    // verify max positions is at least 2 and less than 2 * max reserves
    if config.max_positions < 2 || config.max_positions > 2 * MAX_RESERVES {
        panic_with_error!(&e, PoolError::InvalidPoolConfigArgs);
    }

    // verify min collateral is at least 0
    if config.min_collateral < 0 {
        panic_with_error!(&e, PoolError::InvalidPoolConfigArgs);
    }
}
