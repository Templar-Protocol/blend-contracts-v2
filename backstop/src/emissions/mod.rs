#[cfg(test)]
pub(crate) mod claim;
#[cfg(not(test))]
mod claim;
pub use claim::execute_claim;

#[cfg(test)]
pub(crate) mod distributor;
#[cfg(not(test))]
mod distributor;
pub use distributor::update_emissions;

#[cfg(test)]
pub(crate) mod manager;
#[cfg(not(test))]
mod manager;
pub use manager::{add_to_reward_zone, distribute, gulp_emissions, remove_from_reward_zone};
