use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::Env;

use crate::{constants::SCALAR_7, storage};

use super::{pool::Pool, Positions};

pub struct PositionData {
    /// The effective collateral balance denominated in the base asset
    pub collateral_base: i128,
    // The raw collateral balance denominated in the base asset
    pub collateral_raw: i128,
    /// The effective liability balance denominated in the base asset
    pub liability_base: i128,
    // The raw liability balance denominated in the base asset
    pub liability_raw: i128,
    /// The scalar for the base asset
    pub scalar: i128,
}

impl PositionData {
    /// Calculate the position data for a given set of of positions
    ///
    /// ### Arguments
    /// * pool - The pool
    /// * positions - The positions to calculate the health factor for
    pub fn calculate_from_positions(e: &Env, pool: &mut Pool, positions: &Positions) -> Self {
        let oracle_scalar = 10i128.pow(pool.load_price_decimals(e));

        let reserve_list = storage::get_res_list(e);
        let mut collateral_base = 0;
        let mut liability_base = 0;
        let mut collateral_raw = 0;
        let mut liability_raw = 0;
        for i in 0..reserve_list.len() {
            let b_token_balance = positions.collateral.get(i).unwrap_or(0);
            let d_token_balance = positions.liabilities.get(i).unwrap_or(0);
            if b_token_balance == 0 && d_token_balance == 0 {
                continue;
            }
            let reserve = pool.load_reserve(e, &reserve_list.get_unchecked(i), false);
            let asset_to_base = pool.load_price(e, &reserve.asset);

            if b_token_balance > 0 {
                // append users effective collateral to collateral_base
                let asset_collateral = reserve.to_effective_asset_from_b_token(e, b_token_balance);
                collateral_base +=
                    asset_to_base.fixed_mul_floor(e, &asset_collateral, &reserve.scalar);
                collateral_raw += asset_to_base.fixed_mul_floor(
                    e,
                    &reserve.to_asset_from_b_token(e, b_token_balance),
                    &reserve.scalar,
                );
            }

            if d_token_balance > 0 {
                // append users effective liability to liability_base
                let asset_liability = reserve.to_effective_asset_from_d_token(e, d_token_balance);
                liability_base +=
                    asset_to_base.fixed_mul_ceil(e, &asset_liability, &reserve.scalar);
                liability_raw += asset_to_base.fixed_mul_ceil(
                    e,
                    &reserve.to_asset_from_d_token(e, d_token_balance),
                    &reserve.scalar,
                );
            }

            pool.cache_reserve(reserve);
        }

        PositionData {
            collateral_base,
            collateral_raw,
            liability_base,
            liability_raw,
            scalar: oracle_scalar,
        }
    }

    /// Return the health factor as a ratio
    pub fn as_health_factor(&self, e: &Env) -> i128 {
        self.collateral_base
            .fixed_div_floor(e, &self.liability_base, &self.scalar)
    }

    // Check if the position data is over a maximum health factor
    // Note: max must be 7 decimals
    pub fn is_hf_over(&self, e: &Env, max: i128) -> bool {
        if self.liability_base == 0 {
            return true;
        }
        let min_health_factor = self.scalar.fixed_mul_ceil(e, &max, &SCALAR_7);
        if self.as_health_factor(e) > min_health_factor {
            return true;
        }
        false
    }

    /// Check if the position data is under a minimum health factor
    /// Note: min must be 7 decimals
    pub fn is_hf_under(&self, e: &Env, min: i128) -> bool {
        if self.liability_base == 0 {
            return false;
        }
        let min_health_factor = self.scalar.fixed_mul_floor(e, &min, &SCALAR_7);
        if self.as_health_factor(e) < min_health_factor {
            return true;
        }
        false
    }
}
