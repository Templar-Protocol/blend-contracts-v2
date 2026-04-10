use cast::i128;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{map, panic_with_error, Address, Env, Vec};

use crate::auctions::auction::AuctionData;
use crate::pool::{check_and_handle_user_bad_debt, Pool, PositionData, User};
use crate::Positions;
use crate::{errors::PoolError, storage};

use super::AuctionType;

pub fn create_user_liq_auction_data(
    e: &Env,
    user: &Address,
    bid: &Vec<Address>,
    lot: &Vec<Address>,
    percent: u32,
) -> AuctionData {
    if user == &e.current_contract_address() || user == &storage::get_backstop(e) {
        panic_with_error!(e, PoolError::InvalidLiquidation);
    }
    if storage::has_auction(e, &(AuctionType::UserLiquidation as u32), user) {
        panic_with_error!(e, PoolError::AuctionInProgress);
    }
    if percent > 100 || percent == 0 {
        panic_with_error!(e, PoolError::InvalidLiquidation);
    }

    let mut liquidation_quote = AuctionData {
        bid: map![e],
        lot: map![e],
        block: e.ledger().sequence() + 1,
    };
    let mut full_liquidation_quote = AuctionData {
        bid: map![e],
        lot: map![e],
        block: e.ledger().sequence() + 1,
    };
    let mut pool = Pool::load(e);
    if pool.config.max_positions < (lot.len() + bid.len()) {
        panic_with_error!(e, PoolError::MaxPositionsExceeded);
    }

    // this is used for checking the liquidation percent and should NOT be set
    let mut user_state = User::load(e, user);
    let reserve_list = storage::get_res_list(e);
    let position_data = PositionData::calculate_from_positions(e, &mut pool, &user_state.positions);

    // ensure the user has less collateral than liabilities
    if position_data.liability_base <= position_data.collateral_base {
        panic_with_error!(e, PoolError::InvalidLiquidation);
    }

    // build position data from included assets
    let mut positions_auctioned = Positions::env_default(e);
    for bid_asset in bid {
        // these will be cached if the bid is valid
        let reserve = pool.load_reserve(e, &bid_asset, false);
        match user_state.positions.liabilities.get(reserve.config.index) {
            Some(amount) => {
                positions_auctioned
                    .liabilities
                    .set(reserve.config.index, amount);
            }
            None => {
                panic_with_error!(e, PoolError::InvalidBid);
            }
        }
    }
    if positions_auctioned.liabilities.len() == 0 {
        panic_with_error!(e, PoolError::InvalidBid);
    }
    for lot_asset in lot {
        // these will be cached if the lot is valid
        let reserve = pool.load_reserve(e, &lot_asset, false);
        match user_state.positions.collateral.get(reserve.config.index) {
            Some(amount) => {
                positions_auctioned
                    .collateral
                    .set(reserve.config.index, amount);
            }
            None => {
                panic_with_error!(e, PoolError::InvalidLot);
            }
        }
    }
    if positions_auctioned.collateral.len() == 0 {
        panic_with_error!(e, PoolError::InvalidLot);
    }
    let position_data_inc =
        PositionData::calculate_from_positions(e, &mut pool, &positions_auctioned);
    let is_all_collateral = position_data_inc.collateral_raw == position_data.collateral_raw;
    let is_all_positions =
        is_all_collateral && position_data_inc.liability_raw == position_data.liability_raw;

    // a full liquidation is when all positions are liquidated and the liquidation percent is >95
    let is_full_liquidation = is_all_positions && percent > 95;

    // Full liquidations default to 100% liquidations.
    // To safely check this, calculate the liquidation at 95%, and verify the liquidation
    // is too small.
    let percent_liquidated_to_check = if is_full_liquidation { 95u32 } else { percent };

    let percent_liquidated_i128_scaled =
        i128(percent_liquidated_to_check) * position_data.scalar / 100; // scale to decimal form with scalar decimals

    // ensure liquidation size is fair and the collateral is large enough to allow for the auction to price the liquidation
    let avg_cf = position_data_inc.collateral_base.fixed_div_floor(
        e,
        &position_data_inc.collateral_raw,
        &position_data_inc.scalar,
    );
    // avg_lf is the inverse of the average liability factor
    let avg_lf = position_data_inc.liability_base.fixed_div_floor(
        e,
        &position_data_inc.liability_raw,
        &position_data_inc.scalar,
    );
    let est_incentive = (position_data_inc.scalar
        - avg_cf.fixed_div_ceil(e, &avg_lf, &position_data_inc.scalar))
    .fixed_div_ceil(
        e,
        &(2 * position_data_inc.scalar),
        &position_data_inc.scalar,
    ) + position_data_inc.scalar;

    let est_withdrawn_collateral = position_data_inc
        .liability_raw
        .fixed_mul_floor(
            e,
            &percent_liquidated_i128_scaled,
            &position_data_inc.scalar,
        )
        .fixed_mul_floor(e, &est_incentive, &position_data_inc.scalar);
    let mut est_withdrawn_collateral_pct = est_withdrawn_collateral.fixed_div_ceil(
        e,
        &position_data_inc.collateral_raw,
        &position_data_inc.scalar,
    );

    // estimated lot exceedes the collateral available in the included positions
    if est_withdrawn_collateral_pct > position_data_inc.scalar {
        est_withdrawn_collateral_pct = position_data_inc.scalar;
        // if the included collateral is not all of the users collateral, panic,
        // as the missing collateral should be included in the liquidation to avoid
        // potentially bad liquidations
        if !is_all_collateral {
            panic_with_error!(e, PoolError::InvalidLiquidation);
        }
    }

    for (asset, amount) in positions_auctioned.collateral.iter() {
        let res_asset_address = reserve_list.get_unchecked(asset);
        let b_tokens_removed =
            amount.fixed_mul_ceil(e, &est_withdrawn_collateral_pct, &position_data.scalar);
        liquidation_quote
            .lot
            .set(res_asset_address.clone(), b_tokens_removed);
        full_liquidation_quote.lot.set(res_asset_address, amount);
    }

    for (asset, amount) in positions_auctioned.liabilities.iter() {
        let res_asset_address = reserve_list.get_unchecked(asset);
        let d_tokens_removed =
            amount.fixed_mul_ceil(e, &percent_liquidated_i128_scaled, &position_data.scalar);
        liquidation_quote
            .bid
            .set(res_asset_address.clone(), d_tokens_removed);
        full_liquidation_quote.bid.set(res_asset_address, amount);
    }

    user_state.rm_positions(
        e,
        &mut pool,
        liquidation_quote.lot.clone(),
        liquidation_quote.bid.clone(),
    );
    let new_data = PositionData::calculate_from_positions(e, &mut pool, &user_state.positions);

    if is_full_liquidation {
        // A full user liquidation was requested, validate that a full liquidation is not too large.
        // If the user has enough collateral to create the liquidation auction, validate that the
        // 95% liquidation is not too large. That is, if a user can be liquidated to 95%, they can
        // be liquidated fully. This helps prevent edge cases due to liquidation percentages
        // being harder to calculate between as it approaches 100.
        if est_withdrawn_collateral < position_data.collateral_raw
            && new_data.is_hf_over(e, 1_1500000)
        {
            panic_with_error!(e, PoolError::InvalidLiqTooLarge)
        };
        full_liquidation_quote
    } else {
        // Post-liq health factor must be under 1.15
        if new_data.is_hf_over(e, 1_1500000) {
            panic_with_error!(e, PoolError::InvalidLiqTooLarge)
        };

        // Post-liq heath factor must be over 1.03
        if new_data.is_hf_under(e, 1_0300000) {
            panic_with_error!(e, PoolError::InvalidLiqTooSmall)
        };
        liquidation_quote
    }
}

pub fn fill_user_liq_auction(
    e: &Env,
    pool: &mut Pool,
    auction_data: &AuctionData,
    user: &Address,
    filler_state: &mut User,
    is_full_fill: bool,
) {
    let mut user_state = User::load(e, user);
    user_state.rm_positions(e, pool, auction_data.lot.clone(), auction_data.bid.clone());
    filler_state.add_positions(e, pool, auction_data.lot.clone(), auction_data.bid.clone());

    if is_full_fill {
        check_and_handle_user_bad_debt(e, pool, user, &mut user_state);
    }
    user_state.store(e);
}
