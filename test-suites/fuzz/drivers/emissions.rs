use super::assert_pool_invariants;
use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use pool::ReserveEmissionMetadata;
use soroban_sdk::vec;
use test_suites::create_fixture_with_data;

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let fixture = create_fixture_with_data(mode.uses_wasm());
    let pool = &fixture.pools[0].pool;
    let pool_address = pool.address.clone();
    let user = fixture.users[0].clone();
    let mut report = RunReport::decoded(operations.len());

    for operation in operations {
        match operation.code % 9 {
            0 => {
                fixture.jump(u64::from(operation.amount % (7 * 24 * 60 * 60 + 1)));
                report.applied();
            }
            1 => {
                fixture.emitter.distribute();
                let result = fixture.backstop.try_distribute();
                if let Some(distributed) = contract_call(&fixture.env, result, &mut report) {
                    assert!(distributed >= 0);
                    report.observe_i128(distributed);
                }
            }
            2 => {
                let result = pool.try_gulp_emissions();
                if let Some(gulped) = contract_call(&fixture.env, result, &mut report) {
                    assert!(gulped >= 0);
                    report.observe_i128(gulped);
                }
            }
            3 => {
                let reserve_ids = vec![&fixture.env, 0, 3];
                let result = pool.try_claim(&user, &reserve_ids, &user);
                if let Some(claimed) = contract_call(&fixture.env, result, &mut report) {
                    assert!(claimed >= 0);
                    let second = pool.try_claim(&user, &reserve_ids, &user);
                    let second_claim = contract_call(&fixture.env, second, &mut report)
                        .expect("a repeated valid claim must remain callable");
                    // MUTATION-CHECK: claiming clears accrued user emissions;
                    // a second claim in the same ledger must transfer nothing.
                    assert_eq!(second_claim, 0);
                    report.observe_i128(claimed);
                }
            }
            4 => {
                let result = fixture.backstop.try_claim(
                    &user,
                    &vec![&fixture.env, pool_address.clone()],
                    &0,
                );
                if let Some(claimed) = contract_call(&fixture.env, result, &mut report) {
                    assert!(claimed >= 0);
                    report.observe_i128(claimed);
                }
            }
            5 => {
                let first_share = u64::from(operation.amount % 10_000_001);
                let second_share = if operation.flags & 1 == 0 {
                    10_000_000 - first_share
                } else {
                    u64::from(operation.amount.rotate_left(7) % 10_000_001)
                };
                let metadata = vec![
                    &fixture.env,
                    ReserveEmissionMetadata {
                        res_index: 0,
                        res_type: 0,
                        share: first_share,
                    },
                    ReserveEmissionMetadata {
                        res_index: 1,
                        res_type: 1,
                        share: second_share,
                    },
                ];
                let result = pool.try_set_emissions_config(&metadata);
                let applied = contract_call(&fixture.env, result, &mut report).is_some();
                assert_eq!(applied, first_share > 0 && second_share > 0);
            }
            6 => {
                let result = fixture.backstop.try_add_reward(&pool_address, &None);
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            7 => {
                let result = if operation.flags & 1 == 0 {
                    fixture.backstop.try_remove_reward(&pool_address)
                } else {
                    fixture.backstop.try_drop()
                };
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            _ => {
                let reserve_id = u32::from(operation.asset % 6);
                let reserve = pool.get_reserve_emissions(&reserve_id);
                let user_data = pool.get_user_emissions(&user, &reserve_id);
                report.observe_u64(u64::from(reserve.is_some()));
                report.observe_u64(u64::from(user_data.is_some()));
                report.noop();
            }
        }
        assert_pool_invariants(&fixture, &mut report);
        assert!(fixture.backstop.reward_zone().contains(&pool_address));
    }

    report
}
