# Emergency Response Runbooks

This directory hosts two role-segmented collections of public emergency
response runbooks:

- [`blend/`](./blend/) — for the Blend lending protocol on Stellar,
  covering the Blend pools defined in this repository plus the vaults
  in [`Templar-Protocol/contracts`](https://github.com/Templar-Protocol/contracts)
  that supply into them via the Soroban Blend adapter, plus the
  surrounding Stellar ecosystem (Stellar Foundation, bridges,
  stablecoins, validators).
- [`templar/`](./templar/) — for the Templar Protocol on NEAR,
  covering the immutable markets, registry, LST and proxy oracle
  adapters, NEAR vault runtime, plus the surrounding NEAR ecosystem
  (NEAR Foundation, NEAR Intents team, bridges, stablecoins,
  validators).

Both collections share the same severity scale, response targets, and
Hypernative alert families. Each collection adds ecosystem-specific
signals and incident guidance — for example, the Templar collection's
Hypernative taxonomy and severity rows explicitly reference the NEAR
Intents team's signals, while the Blend collection does not. Both use
a "Safe Chain" cross-role coordination model, and both address the
same category set: protocol hacks, bad debt, faulty oracles,
privileged-role compromise, and counterparty-layer compromise.

Incidents that cross both ecosystems — for example, a bridged-asset
issue that touches both a NEAR Templar market and a Soroban Templar
vault that supplies into a Blend pool — should route via both
collections' Safe Chains, with the affected roles from each collection
present in the shared war room.

Start with the `README.md` in the collection that matches your role
and jurisdiction. Then read the runbook for your role. Then read the
runbook for any adjacent role you may need to coordinate with during
a Safe Chain incident.
