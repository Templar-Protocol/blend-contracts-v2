use cast::i128;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{contracttype, panic_with_error, Address, Env};

use crate::{
    constants::{SCALAR_12, SCALAR_7},
    errors::PoolError,
    pool::actions::RequestType,
    storage::{self, PoolConfig, ReserveConfig, ReserveData},
};

use super::interest::calc_accrual;

#[derive(Clone, Debug)]
#[contracttype]
pub struct Reserve {
    pub asset: Address,        // the underlying asset address
    pub config: ReserveConfig, // the reserve configuration
    pub data: ReserveData,     // the reserve data
    pub scalar: i128,
}

impl Reserve {
    /// Load a Reserve from the ledger and update to the current ledger timestamp.
    ///
    /// **NOTE**: This function is not cached, and should be called from the Pool.
    ///
    /// ### Arguments
    /// * pool_config - The pool configuration
    /// * asset - The address of the underlying asset
    ///
    /// ### Panics
    /// Panics if the asset is not supported, if emissions cannot be updated, or if the reserve
    /// cannot be updated to the current ledger timestamp.
    pub fn load(e: &Env, pool_config: &PoolConfig, asset: &Address) -> Reserve {
        let reserve_config = storage::get_res_config(e, asset);
        let reserve_data = storage::get_res_data(e, asset);
        let mut reserve = Reserve {
            asset: asset.clone(),
            scalar: 10i128.pow(reserve_config.decimals),
            config: reserve_config,
            data: reserve_data,
        };

        // short circuit if the reserve has already been updated this ledger
        if e.ledger().timestamp() == reserve.data.last_time {
            return reserve;
        }

        if reserve.data.b_supply == 0 {
            reserve.data.last_time = e.ledger().timestamp();
            return reserve;
        }

        let cur_util = reserve.utilization(e);
        if cur_util == 0 {
            // if there are no assets borrowed, we don't need to update the reserve
            reserve.data.last_time = e.ledger().timestamp();
            return reserve;
        }

        let (loan_accrual, new_ir_mod) = calc_accrual(
            e,
            &reserve.config,
            cur_util,
            reserve.data.ir_mod,
            reserve.data.last_time,
        );
        reserve.data.ir_mod = new_ir_mod;

        let pre_update_liabilities = reserve.total_liabilities(e);
        reserve.data.d_rate = loan_accrual.fixed_mul_ceil(e, &reserve.data.d_rate, &SCALAR_12);
        let accrued_interest = reserve.total_liabilities(e) - pre_update_liabilities;

        reserve.accrue(e, pool_config.bstop_rate, accrued_interest);

        reserve.data.last_time = e.ledger().timestamp();
        reserve
    }

    /// Store the updated reserve to the ledger.
    pub fn store(&self, e: &Env) {
        storage::set_res_data(e, &self.asset, &self.data);
    }

    /// Accrue tokens to the reserve supply. This issues any `backstop_credit` required and updates the reserve's bRate to account for the additional tokens.
    ///
    /// ### Arguments
    /// * bstop_rate - The backstop take rate for the pool
    /// * accrued - The amount of additional underlying tokens
    pub(crate) fn accrue(&mut self, e: &Env, bstop_rate: u32, accrued: i128) {
        let pre_update_supply = self.total_supply(e);

        if accrued > 0 {
            // credit the backstop underlying from the accrued interest based on the backstop rate
            // update the accrued interest to reflect the amount the pool accrued
            let mut new_backstop_credit: i128 = 0;
            if bstop_rate > 0 {
                new_backstop_credit = accrued.fixed_mul_floor(e, &i128(bstop_rate), &SCALAR_7);
                self.data.backstop_credit += new_backstop_credit;
            }
            self.data.b_rate = (pre_update_supply + accrued - new_backstop_credit).fixed_div_floor(
                e,
                &self.data.b_supply,
                &SCALAR_12,
            );
        }
    }

    /// Fetch the current utilization rate for the reserve normalized to 7 decimals
    ///
    /// This is capped at 100% to ensure interest calculations are fair.
    pub fn utilization(&self, e: &Env) -> i128 {
        let liabilities = self.total_liabilities(e);
        let supply = self.total_supply(e);
        if liabilities == 0 {
            return 0;
        } else if liabilities >= supply {
            return SCALAR_7;
        }
        self.total_liabilities(e)
            .fixed_div_ceil(e, &self.total_supply(e), &SCALAR_7)
    }

    /// Require that the utilization rate is at or below the maximum allowed, or panic.
    pub fn require_utilization_below_max(&self, e: &Env) {
        if self.utilization(e) > i128(self.config.max_util) {
            panic_with_error!(e, PoolError::InvalidUtilRate)
        }
    }

    /// Require that the utilization rate is below 100%, or panic.
    ///
    /// Used to validate that the reserve has enough liquidity to support the requested action,
    /// as some tokens held by the pool are reserved for the backstop.
    pub fn require_utilization_below_100(&self, e: &Env) {
        if self.utilization(e) >= SCALAR_7 {
            panic_with_error!(e, PoolError::InvalidUtilRate)
        }
    }

    /// Check the action is allowed according to the reserve status, or panic.
    ///
    /// ### Arguments
    /// * `action_type` - The type of action being performed
    pub fn require_action_allowed(&self, e: &Env, action_type: u32) {
        // disable borrowing or auction cancellation for any non-active pool and disable supplying for any frozen pool
        if !self.config.enabled {
            if action_type == RequestType::Supply as u32
                || action_type == RequestType::SupplyCollateral as u32
                || action_type == RequestType::Borrow as u32
            {
                panic_with_error!(e, PoolError::ReserveDisabled);
            }
        }
    }

    /// Fetch the total liabilities for the reserve in underlying tokens
    pub fn total_liabilities(&self, e: &Env) -> i128 {
        self.to_asset_from_d_token(e, self.data.d_supply)
    }

    /// Fetch the total supply for the reserve in underlying tokens
    pub fn total_supply(&self, e: &Env) -> i128 {
        self.to_asset_from_b_token(e, self.data.b_supply)
    }

    /********** Conversion Functions **********/

    /// Convert d_tokens to the corresponding asset value
    ///
    /// ### Arguments
    /// * `d_tokens` - The amount of tokens to convert
    pub fn to_asset_from_d_token(&self, e: &Env, d_tokens: i128) -> i128 {
        d_tokens.fixed_mul_ceil(e, &self.data.d_rate, &SCALAR_12)
    }

    /// Convert b_tokens to the corresponding asset value
    ///
    /// ### Arguments
    /// * `b_tokens` - The amount of tokens to convert
    pub fn to_asset_from_b_token(&self, e: &Env, b_tokens: i128) -> i128 {
        b_tokens.fixed_mul_floor(e, &self.data.b_rate, &SCALAR_12)
    }

    /// Convert d_tokens to their corresponding effective asset value. This
    /// takes into account the liability factor.
    ///
    /// ### Arguments
    /// * `d_tokens` - The amount of tokens to convert
    pub fn to_effective_asset_from_d_token(&self, e: &Env, d_tokens: i128) -> i128 {
        let assets = self.to_asset_from_d_token(e, d_tokens);
        assets.fixed_div_ceil(e, &i128(self.config.l_factor), &SCALAR_7)
    }

    /// Convert b_tokens to the corresponding effective asset value. This
    /// takes into account the collateral factor.
    ///
    /// ### Arguments
    /// * `b_tokens` - The amount of tokens to convert
    pub fn to_effective_asset_from_b_token(&self, e: &Env, b_tokens: i128) -> i128 {
        let assets = self.to_asset_from_b_token(e, b_tokens);
        assets.fixed_mul_floor(e, &i128(self.config.c_factor), &SCALAR_7)
    }

    /// Convert asset tokens to the corresponding d token value - rounding up
    ///
    /// ### Arguments
    /// * `amount` - The amount of tokens to convert
    pub fn to_d_token_up(&self, e: &Env, amount: i128) -> i128 {
        amount.fixed_div_ceil(e, &self.data.d_rate, &SCALAR_12)
    }

    /// Convert asset tokens to the corresponding d token value - rounding down
    ///
    /// ### Arguments
    /// * `amount` - The amount of tokens to convert
    pub fn to_d_token_down(&self, e: &Env, amount: i128) -> i128 {
        amount.fixed_div_floor(e, &self.data.d_rate, &SCALAR_12)
    }

    /// Convert asset tokens to the corresponding b token value - round up
    ///
    /// ### Arguments
    /// * `amount` - The amount of tokens to convert
    pub fn to_b_token_up(&self, e: &Env, amount: i128) -> i128 {
        amount.fixed_div_ceil(e, &self.data.b_rate, &SCALAR_12)
    }

    /// Convert asset tokens to the corresponding b token value - round down
    ///
    /// ### Arguments
    /// * `amount` - The amount of tokens to convert
    pub fn to_b_token_down(&self, e: &Env, amount: i128) -> i128 {
        amount.fixed_div_floor(e, &self.data.b_rate, &SCALAR_12)
    }
}
