use super::{assert_pool_invariants, create_actor, scalar};
use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use pool::{FlashLoan, Request, RequestType};
use soroban_sdk::vec;
use test_suites::create_fixture_with_data;
use test_suites::moderc3156::create_flashloan_receiver;
use test_suites::test_fixture::TokenIndex;

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let mut fixture = create_fixture_with_data(mode.uses_wasm());
    let actor = create_actor(&mut fixture);
    let (receiver, _) = create_flashloan_receiver(&fixture.env);
    let pool = &fixture.pools[0].pool;
    pool.submit(
        &actor,
        &actor,
        &actor,
        &vec![
            &fixture.env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: fixture.tokens[TokenIndex::STABLE].address.clone(),
                amount: 10_000 * scalar(TokenIndex::STABLE),
            },
        ],
    );
    let mut report = RunReport::decoded(operations.len());

    for operation in operations {
        match operation.code % 6 {
            0 => {
                fixture.jump(u64::from(operation.amount % 86_401));
                report.applied();
            }
            1..=3 => {
                let token_index = match operation.code % 6 {
                    1 => TokenIndex::XLM,
                    2 => TokenIndex::STABLE,
                    _ => TokenIndex::WETH,
                };
                let token = &fixture.tokens[token_index];
                let loan_amount = i128::from(operation.amount % 101 + 1) * scalar(token_index);
                let flash_loan = FlashLoan {
                    contract: receiver.clone(),
                    asset: token.address.clone(),
                    amount: loan_amount,
                };
                let requests = vec![
                    &fixture.env,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: token.address.clone(),
                        amount: loan_amount,
                    },
                ];
                let pool_before = token.balance(&pool.address);
                let actor_before = token.balance(&actor);
                let result = pool.try_flash_loan(&actor, &flash_loan, &requests);
                let applied = contract_call(&fixture.env, result, &mut report).is_some();
                // A fully repaid zero-fee flash loan is token-conservative; a
                // rejected transaction is atomic and has the same observation.
                assert_eq!(token.balance(&pool.address), pool_before);
                assert_eq!(token.balance(&actor), actor_before);
                report.observe_u64(u64::from(applied));
            }
            4 => {
                let token = &fixture.tokens[TokenIndex::XLM];
                let flash_loan = FlashLoan {
                    contract: receiver.clone(),
                    asset: token.address.clone(),
                    amount: 0,
                };
                let result = pool.try_flash_loan(&actor, &flash_loan, &vec![&fixture.env]);
                assert!(contract_call(&fixture.env, result, &mut report).is_none());
            }
            _ => {
                let _ = pool.get_positions(&actor);
                report.noop();
            }
        }
        assert_pool_invariants(&fixture, &mut report);
    }

    report
}
