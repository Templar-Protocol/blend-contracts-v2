use soroban_sdk::{Address, Env};
use crate::spec::model;
use crate::Positions;
use crate::pool::{actions::Actions, Pool};

pub(crate) fn positions_hf_under(_e: &Env, _pool: &mut Pool, _positions: &Positions, _hf: i128) -> bool {
    let check_result: bool = cvlr::nondet();

    // If this function returns `true` then the health factor is too low
    // so we set the checked flag if we're returning `false`
    if !check_result {
        model::set_checked();
    }
    check_result
}

pub(crate) fn handle_transfer_with_allowance(_e: &Env, _actions: &Actions, _spender: &Address, _to: &Address) { }
pub(crate) fn handle_transfers(_e: &Env, _actions: &Actions, _spender: &Address, _to: &Address) { }
