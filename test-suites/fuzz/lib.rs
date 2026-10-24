//! Common code for fuzzing test suites.

pub mod pool_general;
mod valuation;

use pool::{Request, RequestType};
use serde::{Deserialize, Serialize};
use soroban_sdk::testutils::arbitrary::arbitrary::{self, Arbitrary, Unstructured};
use soroban_sdk::vec;
use test_suites::{all_events, test_fixture::TestFixture};

#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct NatI128(
    #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=(i64::MAX as i128)))] pub i128,
);

type ContractResult<T> = Result<T, Result<soroban_sdk::Error, soroban_sdk::InvokeError>>;

/// Panic if a contract call result might have been the result of an unexpected panic.
///
/// Calls that return an error with type `ScErrorType::WasmVm` and code `ScErrorCode::InvalidAction`
/// are assumed to be unintended errors. These are the codes that result from plain `panic!` invocations,
/// thus contracts should never simply call `panic!`, but instead use `panic_with_error!`.
///
/// Other rare types of internal exception can return `InvalidAction`.
#[track_caller]
pub fn verify_contract_result<T>(env: &soroban_sdk::Env, r: &ContractResult<T>) {
    use soroban_sdk::xdr::{ScErrorCode, ScErrorType};
    if let Err(Ok(e)) = r {
        if e.is_type(ScErrorType::WasmVm) && e.is_code(ScErrorCode::InvalidAction) {
            let msg = "contract failed with InvalidAction - unexpected panic?";
            eprintln!("{msg}");
            eprintln!("recent events (10):");
            for (i, event) in all_events(env).iter().rev().take(10).enumerate() {
                eprintln!("{i}: {event:?}");
            }
            panic!("{msg}");
        }
    }
}

/// The set of tokens that the pool supports.
#[derive(Arbitrary, Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolReserveToken {
    WETH = 2,
    XLM = 3,
    STABLE = 4,
}

/// Jump the Env `timestamp` forward by `amount` seconds.
///
/// Does not change the `sequence` to prevent any ledger expirations.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct PassTime {
    pub amount: u64,
}

/// Supply `amount` of `token` into the pool.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct Supply {
    pub amount: i128,
    pub token: PoolReserveToken,
}

/// Withdraw `amount` of `token` out of the pool for `user`.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct Withdraw {
    pub amount: i128,
    pub token: PoolReserveToken,
}

/// Borrow `amount` of `token` out of the pool for `user`.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct Borrow {
    pub amount: i128,
    pub token: PoolReserveToken,
}

/// Repay `amount` of `token` into the pool for `user`.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct Repay {
    pub amount: i128,
    pub token: PoolReserveToken,
}

/// Claim emissions from the pool for `user`.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
pub struct ClaimPool {}

impl PassTime {
    pub fn run(&self, fixture: &TestFixture) {
        fixture.jump(self.amount);
    }
}

impl Supply {
    pub fn run(&self, fixture: &TestFixture, user_index: usize) -> bool {
        let pool_fixture = fixture.pools.first().unwrap();
        let token = fixture
            .tokens
            .get(self.token as usize)
            .unwrap()
            .address
            .clone();
        let user = fixture.users.get(user_index).unwrap();
        let r = pool_fixture.pool.try_submit(
            user,
            user,
            user,
            &vec![
                &fixture.env,
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: token,
                    amount: self.amount,
                },
            ],
        );
        verify_contract_result(&fixture.env, &r);
        matches!(r, Ok(Ok(_)))
    }
}

impl Withdraw {
    pub fn run(&self, fixture: &TestFixture, user_index: usize) -> bool {
        let pool_fixture = fixture.pools.first().unwrap();
        let token = fixture
            .tokens
            .get(self.token as usize)
            .unwrap()
            .address
            .clone();
        let user = fixture.users.get(user_index).unwrap();
        let r = pool_fixture.pool.try_submit(
            user,
            user,
            user,
            &vec![
                &fixture.env,
                Request {
                    request_type: RequestType::WithdrawCollateral as u32,
                    address: token,
                    amount: self.amount,
                },
            ],
        );
        verify_contract_result(&fixture.env, &r);
        matches!(r, Ok(Ok(_)))
    }
}

impl Borrow {
    pub fn run(&self, fixture: &TestFixture, user_index: usize) -> bool {
        let pool_fixture = fixture.pools.first().unwrap();
        let token = fixture
            .tokens
            .get(self.token as usize)
            .unwrap()
            .address
            .clone();
        let user = fixture.users.get(user_index).unwrap();
        let r = pool_fixture.pool.try_submit(
            user,
            user,
            user,
            &vec![
                &fixture.env,
                Request {
                    request_type: RequestType::Borrow as u32,
                    address: token,
                    amount: self.amount,
                },
            ],
        );
        verify_contract_result(&fixture.env, &r);
        matches!(r, Ok(Ok(_)))
    }
}

impl Repay {
    pub fn run(&self, fixture: &TestFixture, user_index: usize) -> bool {
        let pool_fixture = fixture.pools.first().unwrap();
        let token = fixture
            .tokens
            .get(self.token as usize)
            .unwrap()
            .address
            .clone();
        let user = fixture.users.get(user_index).unwrap();
        let r = pool_fixture.pool.try_submit(
            user,
            user,
            user,
            &vec![
                &fixture.env,
                Request {
                    request_type: RequestType::Repay as u32,
                    address: token,
                    amount: self.amount,
                },
            ],
        );
        verify_contract_result(&fixture.env, &r);
        matches!(r, Ok(Ok(_)))
    }
}

impl ClaimPool {
    pub fn run(&self, fixture: &TestFixture, user_index: usize) -> bool {
        let pool_fixture = fixture.pools.first().unwrap();
        let user = fixture.users.get(user_index).unwrap();
        let r = pool_fixture
            .pool
            .try_claim(user, &vec![&fixture.env, 0, 3], user);
        verify_contract_result(&fixture.env, &r);
        matches!(r, Ok(Ok(_)))
    }
}
