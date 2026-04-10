#[cfg(test)]
pub(crate) mod manager;
#[cfg(not(test))]
mod manager;
pub use manager::{gulp_emissions, set_pool_emissions, ReserveEmissionMetadata};

#[cfg(test)]
pub(crate) mod distributor;
#[cfg(not(test))]
mod distributor;
pub use distributor::{execute_claim, update_emissions};
