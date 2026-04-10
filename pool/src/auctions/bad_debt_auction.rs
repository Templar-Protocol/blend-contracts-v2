use crate::{
    constants::SCALAR_7,
    dependencies::BackstopClient,
    errors::PoolError,
    pool::{check_and_handle_backstop_bad_debt, Pool, User},
    storage,
};
use cast::i128;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{map, panic_with_error, Address, Env, Vec};

use super::{AuctionData, AuctionType};

pub fn create_bad_debt_auction_data(
    e: &Env,
    user: &Address,
    bid: &Vec<Address>,
    lot: &Vec<Address>,
    percent: u32,
) -> AuctionData {
    let backstop = storage::get_backstop(e);
    if user != &backstop {
        panic_with_error!(e, PoolError::BadRequest);
    }
    if percent != 100 {
        panic_with_error!(e, PoolError::BadRequest);
    }
    if storage::has_auction(e, &(AuctionType::BadDebtAuction as u32), &backstop) {
        panic_with_error!(e, PoolError::AuctionInProgress);
    }

    let mut auction_data = AuctionData {
        bid: map![e],
        lot: map![e],
        block: e.ledger().sequence() + 1,
    };

    // validate and create bid auction data
    let mut pool = Pool::load(e);
    // lot is required to have 1 entry, so require bid to have less than max_positions entries
    if pool.config.max_positions <= bid.len() {
        panic_with_error!(e, PoolError::MaxPositionsExceeded);
    }

    let oracle_scalar = 10i128.pow(pool.load_price_decimals(e));
    let backstop_positions = storage::get_user_positions(e, &backstop);
    let mut debt_value = 0;
    for bid_asset in bid {
        let reserve = pool.load_reserve(e, &bid_asset, false);
        let liability_balance = backstop_positions
            .liabilities
            .get(reserve.config.index)
            .unwrap_or(0);
        if liability_balance > 0 {
            let asset_to_base = pool.load_price(e, &reserve.asset);
            let asset_balance = reserve.to_asset_from_d_token(e, liability_balance);
            debt_value += i128(asset_to_base).fixed_mul_floor(e, &asset_balance, &reserve.scalar);
            auction_data.bid.set(reserve.asset, liability_balance);
        } else {
            panic_with_error!(e, PoolError::InvalidBid);
        }
    }

    if auction_data.bid.is_empty() || debt_value <= 0 {
        panic_with_error!(e, PoolError::InvalidBid);
    }

    // validate and create lot auction data
    let backstop_client = BackstopClient::new(e, &backstop);
    let backstop_token = backstop_client.backstop_token();
    if lot.len() != 1 || lot.get_unchecked(0) != backstop_token {
        panic_with_error!(e, PoolError::InvalidLot);
    }

    // get value of backstop_token (BLND-USDC LP token) to base
    let pool_backstop_data = backstop_client.pool_data(&e.current_contract_address());

    if pool_backstop_data.tokens <= 0 {
        // no tokens left in backstop to auction off
        panic_with_error!(e, PoolError::InvalidLot);
    }

    // determine lot amount of backstop tokens needed to safely cover bad debt, or post
    // all backstop tokens if there isn't enough to cover the bad debt. backstop tokens use 7 decimals
    let mut lot_amount =
        debt_value // oracle_scalar
            .fixed_mul_floor(e, &1_2000000, &oracle_scalar) // denom of oracle_scalar means result is SCALAR_7
            .fixed_div_floor(e, &pool_backstop_data.token_spot_price, &SCALAR_7); // token_spot_price is SCALAR_7
    lot_amount = pool_backstop_data.tokens.min(lot_amount);
    auction_data.lot.set(backstop_token, lot_amount);

    auction_data
}

#[allow(clippy::inconsistent_digit_grouping)]
pub fn fill_bad_debt_auction(
    e: &Env,
    pool: &mut Pool,
    auction_data: &AuctionData,
    filler_state: &mut User,
    is_full_fill: bool,
) {
    let backstop_address = storage::get_backstop(e);
    if filler_state.address == backstop_address {
        panic_with_error!(e, PoolError::BadRequest);
    }
    let mut backstop_state = User::load(e, &backstop_address);

    // bid only contains d_token asset amounts
    backstop_state.rm_positions(e, pool, map![e], auction_data.bid.clone());
    filler_state.add_positions(e, pool, map![e], auction_data.bid.clone());

    let backstop_client = BackstopClient::new(e, &backstop_address);
    let backstop_token_id = backstop_client.backstop_token();
    let lot_amount = auction_data.lot.get(backstop_token_id).unwrap_or(0);
    if lot_amount > 0 {
        backstop_client.draw(
            &e.current_contract_address(),
            &lot_amount,
            &filler_state.address,
        );
    }

    if is_full_fill {
        // defaults rest of bad debt if insufficient backstop tokens remain in the backstop
        check_and_handle_backstop_bad_debt(e, pool, &backstop_address, &mut backstop_state);
    }
    backstop_state.store(e);
}
