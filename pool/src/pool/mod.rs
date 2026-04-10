#[cfg(test)]
pub(crate) mod actions;
#[cfg(not(test))]
mod actions;
pub use actions::{FlashLoan, Request, RequestType};

#[cfg(test)]
pub(crate) mod bad_debt;
#[cfg(not(test))]
mod bad_debt;
pub use bad_debt::{bad_debt, check_and_handle_backstop_bad_debt, check_and_handle_user_bad_debt};

#[cfg(test)]
pub(crate) mod config;
#[cfg(not(test))]
mod config;
pub use config::{
    execute_cancel_queued_set_reserve, execute_initialize, execute_queue_set_reserve,
    execute_set_reserve, execute_update_pool,
};

#[cfg(test)]
pub(crate) mod health_factor;
#[cfg(not(test))]
mod health_factor;
pub use health_factor::PositionData;

#[cfg(test)]
pub(crate) mod interest;
#[cfg(not(test))]
mod interest;

#[cfg(test)]
pub(crate) mod submit;
#[cfg(not(test))]
mod submit;

pub use submit::{execute_submit, execute_submit_with_flash_loan};

#[allow(clippy::module_inception)]
#[cfg(test)]
pub(crate) mod pool;
#[allow(clippy::module_inception)]
#[cfg(not(test))]
mod pool;
pub use pool::Pool;

#[cfg(test)]
pub(crate) mod reserve;
#[cfg(not(test))]
mod reserve;
pub use reserve::Reserve;

#[cfg(test)]
pub(crate) mod user;
#[cfg(not(test))]
mod user;
pub use user::{Positions, User};

#[cfg(test)]
pub(crate) mod status;
#[cfg(not(test))]
mod status;
pub use status::{
    calc_pool_backstop_threshold, execute_set_pool_status, execute_update_pool_status,
};

#[cfg(test)]
pub(crate) mod gulp;
#[cfg(not(test))]
mod gulp;
pub use gulp::execute_gulp;
