use crate::{dependencies::CometClient, errors::BackstopError, events::BackstopEvents, storage};
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    panic_with_error,
    unwrap::UnwrapOptimized,
    vec, Address, Env, IntoVal, Map, Symbol, Val, Vec,
};

use super::distributor::claim_emissions;

/// Perform a claim for backstop deposit emissions by a user from the backstop module
pub fn execute_claim(
    e: &Env,
    from: &Address,
    pool_addresses: &Vec<Address>,
    min_lp_tokens_out: &i128,
) -> i128 {
    if pool_addresses.is_empty() {
        panic_with_error!(e, BackstopError::BadRequest);
    }

    let mut claimed: i128 = 0;
    let mut claims: Map<Address, i128> = Map::new(e);
    for pool_id in pool_addresses.iter() {
        let pool_balance = storage::get_pool_balance(e, &pool_id);
        let user_balance = storage::get_user_balance(e, &pool_id, from);
        let claim_amt = claim_emissions(e, &pool_id, &pool_balance, from, &user_balance);
        claimed += claim_amt;
        // panic if the user has already claimed for this pool
        // or if the claim amount is 0
        if claims.get(pool_id.clone()).is_some() {
            panic_with_error!(e, BackstopError::BadRequest);
        }
        claims.set(pool_id.clone(), claim_amt);
    }

    if claimed > 0 {
        let blnd_id = storage::get_blnd_token(e);
        let lp_id = storage::get_backstop_token(e);
        let approval_ledger = (e.ledger().sequence() / 100000 + 1) * 100000;
        let args: Vec<Val> = vec![
            e,
            (&e.current_contract_address()).into_val(e),
            (&lp_id).into_val(e),
            (&claimed).into_val(e),
            (&approval_ledger).into_val(e),
        ];
        e.authorize_as_current_contract(vec![
            &e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: blnd_id.clone(),
                    fn_name: Symbol::new(e, "approve"),
                    args: args.clone(),
                },
                sub_invocations: vec![e],
            }),
        ]);
        let lp_tokens_out = CometClient::new(e, &lp_id).dep_tokn_amt_in_get_lp_tokns_out(
            &blnd_id,
            &claimed,
            &min_lp_tokens_out,
            &e.current_contract_address(),
        );
        for pool_id in pool_addresses.iter() {
            let claim_amount = claims.get(pool_id.clone()).unwrap_optimized();
            let deposit_amount = lp_tokens_out
                .fixed_mul_floor(claim_amount, claimed)
                .unwrap_optimized();
            if deposit_amount > 0 {
                let mut pool_balance = storage::get_pool_balance(e, &pool_id);
                let mut user_balance = storage::get_user_balance(e, &pool_id, from);

                // Deposit LP tokens into pool backstop
                let to_mint = pool_balance.convert_to_shares(deposit_amount);
                pool_balance.deposit(deposit_amount, to_mint);
                user_balance.add_shares(to_mint);

                storage::set_pool_balance(e, &pool_id, &pool_balance);
                storage::set_user_balance(e, &pool_id, from, &user_balance);

                BackstopEvents::deposit(e, pool_id, from.clone(), deposit_amount, to_mint);
            }
        }
        lp_tokens_out
    } else {
        0
    }
}
