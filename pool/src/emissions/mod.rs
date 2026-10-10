mod manager;
pub use manager::{gulp_emissions, set_pool_emissions, ReserveEmissionMetadata};

mod distributor;
pub use distributor::{execute_claim, update_emissions};
#[cfg(feature = "certora")]
pub use distributor::set_user_emissions;
