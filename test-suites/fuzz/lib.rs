//! Deterministic, bounded drivers shared by libFuzzer and seed replay.

mod drivers;
pub mod model;

use model::DecodeOutcome;
use soroban_sdk::xdr::ScErrorType;
use soroban_sdk::{Env, Error, InvokeError};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    PoolGeneral,
    PoolAdmin,
    PoolAuctions,
    PoolFlashLoan,
    BackstopBalances,
    Emissions,
    PoolFactory,
}

impl Target {
    pub const ALL: [Self; 7] = [
        Self::PoolGeneral,
        Self::PoolAdmin,
        Self::PoolAuctions,
        Self::PoolFlashLoan,
        Self::BackstopBalances,
        Self::Emissions,
        Self::PoolFactory,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PoolGeneral => "fuzz_pool_general",
            Self::PoolAdmin => "fuzz_pool_admin",
            Self::PoolAuctions => "fuzz_pool_auctions",
            Self::PoolFlashLoan => "fuzz_pool_flash_loan",
            Self::BackstopBalances => "fuzz_backstop_balances",
            Self::Emissions => "fuzz_emissions",
            Self::PoolFactory => "fuzz_pool_factory",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|target| target.as_str() == value)
            .ok_or_else(|| format!("unknown fuzz target: {value}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Native,
    Wasm,
}

impl Mode {
    pub const fn uses_wasm(self) -> bool {
        matches!(self, Self::Wasm)
    }
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "native" => Ok(Self::Native),
            "wasm" => Ok(Self::Wasm),
            _ => Err(format!("unknown replay mode: {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputClass {
    Executed,
    Empty,
    EmptyProgram,
    TooManyOperations,
    Truncated,
    TrailingBytes,
}

impl From<DecodeOutcome> for InputClass {
    fn from(value: DecodeOutcome) -> Self {
        match value {
            DecodeOutcome::Empty => Self::Empty,
            DecodeOutcome::EmptyProgram => Self::EmptyProgram,
            DecodeOutcome::TooManyOperations => Self::TooManyOperations,
            DecodeOutcome::Truncated => Self::Truncated,
            DecodeOutcome::TrailingBytes => Self::TrailingBytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunReport {
    pub input: InputClass,
    pub operations: u8,
    pub applied: u8,
    pub rejected: u8,
    pub noops: u8,
    pub state: u64,
}

impl RunReport {
    pub const fn decoded(operations: usize) -> Self {
        Self {
            input: InputClass::Executed,
            operations: operations as u8,
            applied: 0,
            rejected: 0,
            noops: 0,
            state: 0xcbf2_9ce4_8422_2325,
        }
    }

    pub const fn decode_failure(outcome: DecodeOutcome) -> Self {
        Self {
            input: match outcome {
                DecodeOutcome::Empty => InputClass::Empty,
                DecodeOutcome::EmptyProgram => InputClass::EmptyProgram,
                DecodeOutcome::TooManyOperations => InputClass::TooManyOperations,
                DecodeOutcome::Truncated => InputClass::Truncated,
                DecodeOutcome::TrailingBytes => InputClass::TrailingBytes,
            },
            operations: 0,
            applied: 0,
            rejected: 0,
            noops: 0,
            state: 0xcbf2_9ce4_8422_2325,
        }
    }

    pub fn applied(&mut self) {
        self.applied = self
            .applied
            .checked_add(1)
            .expect("bounded operation count");
        self.mix(1);
    }

    pub fn rejected(&mut self) {
        self.rejected = self
            .rejected
            .checked_add(1)
            .expect("bounded operation count");
        self.mix(2);
    }

    pub fn noop(&mut self) {
        self.noops = self.noops.checked_add(1).expect("bounded operation count");
        self.mix(3);
    }

    pub fn observe_u64(&mut self, value: u64) {
        self.mix(value);
    }

    pub fn observe_i128(&mut self, value: i128) {
        self.mix(value as u128 as u64);
        self.mix((value as u128 >> 64) as u64);
    }

    fn mix(&mut self, value: u64) {
        self.state ^= value;
        self.state = self.state.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Classify a generated `try_*` client call. Only contract-domain errors are
/// valid rejected transitions. Return conversion failures, invocation aborts,
/// and host faults are harness failures rather than rejected fuzz inputs.
pub fn contract_call<T, E: fmt::Debug>(
    _env: &Env,
    result: Result<Result<T, E>, Result<Error, InvokeError>>,
    report: &mut RunReport,
) -> Option<T> {
    match result {
        Ok(Ok(value)) => {
            report.applied();
            Some(value)
        }
        Ok(Err(error)) => panic!("contract return conversion failed: {error:?}"),
        Err(Ok(error)) => {
            assert!(
                error.is_type(ScErrorType::Contract),
                "unexpected non-contract invocation error: {error:?}"
            );
            report.rejected();
            None
        }
        Err(Err(error)) => panic!("contract invocation aborted: {error:?}"),
    }
}

pub fn run(target: Target, input: &[u8], mode: Mode) -> RunReport {
    let operations = match model::decode(input) {
        Ok(operations) => operations,
        Err(outcome) => return RunReport::decode_failure(outcome),
    };
    drivers::run(target, &operations, mode)
}

pub fn fuzz(target: Target, input: &[u8]) {
    let _ = run(target, input, Mode::Native);
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_suites::create_fixture_with_data;

    fn classify_absent_auction(mode: Mode) {
        let fixture = create_fixture_with_data(mode.uses_wasm());
        let pool = &fixture.pools[0].pool;
        let result = pool.try_get_auction(&2, &fixture.backstop.address);
        let mut report = RunReport::decoded(1);
        let _ = contract_call(&fixture.env, result, &mut report);
    }

    #[test]
    #[should_panic(expected = "unexpected non-contract invocation error")]
    fn native_absent_auction_is_a_host_fault() {
        classify_absent_auction(Mode::Native);
    }

    #[test]
    #[should_panic(expected = "unexpected non-contract invocation error")]
    fn wasm_absent_auction_is_a_host_fault() {
        classify_absent_auction(Mode::Wasm);
    }

    #[test]
    fn typed_contract_rejection_is_counted() {
        for mode in [Mode::Native, Mode::Wasm] {
            let fixture = create_fixture_with_data(mode.uses_wasm());
            let pool = &fixture.pools[0].pool;
            let mut report = RunReport::decoded(1);
            let result = pool.try_set_status(&1);

            assert!(contract_call(&fixture.env, result, &mut report).is_none());
            assert_eq!(report.applied, 0);
            assert_eq!(report.rejected, 1);
        }
    }

    #[test]
    fn absent_auction_getter_is_a_driver_precondition_noop() {
        let input = [1, 2, 0, 0, 0, 0, 0, 0, 0];
        for mode in [Mode::Native, Mode::Wasm] {
            let report = run(Target::PoolAuctions, &input, mode);
            assert_eq!(report.input, InputClass::Executed);
            assert_eq!(report.operations, 1);
            assert_eq!(report.applied, 0);
            assert_eq!(report.rejected, 0);
            assert_eq!(report.noops, 1);
        }
    }
}
