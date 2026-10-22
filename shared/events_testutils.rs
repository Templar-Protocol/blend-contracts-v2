//! Test-only bridge back to the pre-soroban-sdk-25 event inspection API.
//!
//! soroban-sdk 25 changed `Env::events().all()` from
//! `Vec<(Address, Vec<Val>, Val)>` to `testutils::ContractEvents`, which only
//! compares against a whole event list. The audited tests assert on the last
//! event of an invocation and on event counts, so they need the tuple form
//! back; the topics and data returned here are the event's own values, so the
//! assertions still compare the same bytes the audited tests did.

use soroban_sdk::{testutils::Events as _, xdr, Address, Env, TryFromVal, Val, Vec};

pub fn all_events(e: &Env) -> Vec<(Address, Vec<Val>, Val)> {
    let mut events = Vec::new(e);
    for event in e.events().all().events() {
        let contract_id = event
            .contract_id
            .clone()
            .expect("contract events are emitted by a contract");
        let address = Address::try_from_val(e, &xdr::ScAddress::Contract(contract_id)).unwrap();
        let body = match &event.body {
            xdr::ContractEventBody::V0(body) => body,
        };
        events.push_back((
            address,
            Vec::<Val>::try_from_val(e, &body.topics).unwrap(),
            Val::try_from_val(e, &body.data).unwrap(),
        ));
    }
    events
}
