#![allow(clippy::all)]
pub mod backstop;
pub mod emitter;
pub mod liquidity_pool;
pub mod oracle;
pub mod pool;
pub mod pool_factory;
#[cfg(test)]
pub(crate) mod setup;
#[cfg(not(test))]
mod setup;
pub use setup::create_fixture_with_data;
pub mod assertions;
pub mod moderc3156;
pub mod snapshot;
pub mod test_fixture;
pub mod token;

#[cfg(test)]
mod tests;
