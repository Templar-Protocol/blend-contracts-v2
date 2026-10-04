#![allow(unused)]

use soroban_sdk::{Env, Address};

use crate::storage::{get_res_emis_data, set_res_emis_data, ReserveEmissionData};
use crate::emissions::set_user_emissions;

pub fn update_emissions_summary(
    e: &Env,
    res_token_id: u32,
    supply: i128,
    supply_scalar: i128,
    user: &Address,
    balance: i128,
) {
    if let Some(res_emis_data) = update_emission_data_summary(e, res_token_id, supply, supply_scalar) {
        set_user_emissions(e, user, res_token_id, res_emis_data.index, cvlr::nondet(), false);
    }
}

pub(super) fn update_emission_data_summary(
    e: &Env,
    res_token_id: u32,
    supply: i128,
    supply_scalar: i128,
) -> Option<ReserveEmissionData> {
    match get_res_emis_data(e, &res_token_id) {
        Some(res_emission_data) => {
            let new_res_emission_data = cvlr::nondet();
            set_res_emis_data(e, &res_token_id, &new_res_emission_data);
            Some(new_res_emission_data)
        }
        None => return None,
    }
}