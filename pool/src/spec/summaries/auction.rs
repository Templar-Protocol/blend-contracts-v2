use soroban_sdk::{Env, Address};
use crate::{AuctionType, AuctionData};
use crate::pool::{
    User,
    Pool,
};

/// N.B. ignores auction storage
pub fn fill(
    e: &Env,
    _pool: &mut Pool,
    auction_type: u32,
    user: &Address,
    filler_state: &mut User,
    _percent_filled: u64,
) -> AuctionData {
    match AuctionType::from_u32(e, auction_type) {
        AuctionType::InterestAuction => {
        }
        _ => {
            filler_state.positions.collateral = cvlr_soroban::nondet_map();
            filler_state.positions.liabilities = cvlr_soroban::nondet_map();
            let user_state = User { address: user.clone(), positions: cvlr::nondet() };
            user_state.store(e);
        }
    }

    cvlr::nondet()
}

/// N.B. ignores auction storage
pub fn delete_liquidation(_e: &Env, _user: &Address) {
}
