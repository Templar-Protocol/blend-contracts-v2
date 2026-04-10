#[cfg(test)]
pub(crate) mod auction;
#[cfg(not(test))]
mod auction;
#[cfg(test)]
pub(crate) mod backstop_interest_auction;
#[cfg(not(test))]
mod backstop_interest_auction;
#[cfg(test)]
pub(crate) mod bad_debt_auction;
#[cfg(not(test))]
mod bad_debt_auction;
#[cfg(test)]
pub(crate) mod user_liquidation_auction;
#[cfg(not(test))]
mod user_liquidation_auction;

pub use auction::*;
