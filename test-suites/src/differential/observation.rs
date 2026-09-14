//! Per-step observation envelope for the ADR0008 Step8 differential.
//!
//! The unit of comparison is the SDK's owned `env.to_snapshot()` snapshot —
//! the ONLY complete carrier verified in sdk 22.0.7 (events.rs:104–128
//! `env.events().all()` filters out System events and per-event failed_call
//! flags; env.rs:1562/1626–1645 Snapshot preserves full auth history, full
//! ledger with explicit TTLs, and ALL events, Contract AND System, each with
//! its failed-call flag). Captures happen immediately after each invocation,
//! before any getter.

use soroban_sdk::{testutils::Snapshot, Env};

/// Complete observed state after one invocation (or after setup for `init`).
#[derive(Clone)]
pub struct Observation {
    /// Owned full-state snapshot from `Env::to_snapshot()` at capture time:
    /// generators, complete auth history, ALL events (System+Contract) with
    /// per-event failed_call flags, full ledger incl. explicit live_untils.
    pub snap: Snapshot,
    /// Recorded top-level return/error repr of the observed invocation;
    /// None = success with no value, Some(repr) otherwise. This is the
    /// top-level outcome only — nested per-event failures live inside
    /// `snap.events` failed_call flags and are compared verbatim there.
    pub result_repr: std::option::Option<std::string::String>,
}

impl Observation {
    /// Capture whole state. MUST be called immediately after the observed
    /// invocation and BEFORE any getter.
    ///
    /// Canonical XDR ordering preserves every key, value and TTL.
    pub fn capture(env: &Env, result_repr: std::option::Option<std::string::String>) -> Self {
        let mut snap = env.to_snapshot();
        snap.ledger.ledger_entries.sort();
        Self { snap, result_repr }
    }
}

/// A replayed invocation step on one side of the comparison.
#[derive(Clone)]
pub struct ObservedStep {
    pub label: &'static str,
    pub observation: Observation,
}

/// Full side capture across all replay prefixes.
#[derive(Clone)]
pub struct Capture {
    pub label: &'static str,
    /// Whole state right after fixture setup, before any scenario step.
    pub initial: std::option::Option<Observation>,
    pub steps: std::vec::Vec<ObservedStep>,
}

impl Capture {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            initial: None,
            steps: std::vec::Vec::new(),
        }
    }

    /// Record `init` once during setup, then one entry per replay step.
    pub fn observe(
        &mut self,
        env: &Env,
        label: &'static str,
        result_repr: std::option::Option<std::string::String>,
    ) {
        let obs = Observation::capture(env, result_repr);
        if label == "init" {
            assert!(
                self.initial.is_none(),
                "differential harness: duplicate init capture for {}",
                self.label
            );
            self.initial = Some(obs);
        } else {
            self.steps.push(ObservedStep {
                label,
                observation: obs,
            });
        }
    }

    pub fn initial_obs(&self) -> &Observation {
        self.initial
            .as_ref()
            .expect("differential harness: missing init capture")
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// XDR encodes returned contract values, never host-local Val handles.
    pub fn value<T: soroban_sdk::xdr::ToXdr>(&mut self, env: &Env, label: &'static str, value: T) {
        self.observe(
            env,
            label,
            Some(format!(
                "{:?}",
                value.to_xdr(env).iter().collect::<std::vec::Vec<_>>()
            )),
        );
    }
}
