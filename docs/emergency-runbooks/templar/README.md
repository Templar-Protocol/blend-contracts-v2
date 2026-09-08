# Templar / NEAR Emergency Response Runbooks

This directory contains a public, role-segmented set of runbooks for
responding to emergencies that affect the Templar Protocol on NEAR — its
markets, registry, LST and proxy oracle adapters, vault kernel and
NEAR vault runtime — the Templar counterparties on NEAR (bridge
operators, stablecoin issuers, NEAR Intents team), and the underlying
NEAR network (NEAR Foundation, validators).

These runbooks are inspired by Morpho's curator emergency documentation
([V1 emergency](https://docs.morpho.org/curate/tutorials-v1/emergency/),
[V2 emergency](https://docs.morpho.org/curate/tutorials-v2/emergency/),
[bad debt](https://docs.morpho.org/curate/tutorials-v2/bad-debt/), and
[security considerations](https://docs.morpho.org/curate/concepts/security-considerations/))
and adapted for Templar's NEAR-native architecture, the immutable-markets
governance model documented in
[`docs/src/governance.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/governance.md), and the "NEAR Safe
Chain" coordination model used across protocol, foundation, validator,
bridge, stablecoin, and intent-layer teams during ecosystem-level
incidents.

A companion collection lives in
[`../blend/`](../blend/) for the Stellar / Soroban side (Templar vaults
that supply into Blend pools). Incidents that cross both ecosystems
(e.g. a bridged-asset issue that touches both a NEAR market and a
Soroban vault) route via both collections' Safe Chains.

## Audience

There is one runbook per role. Each is self-contained and assumes the
reader will not have read the others. They are intended to be opened
during an incident and followed top-to-bottom.

| # | Role | File |
|---|------|------|
| 1 | Templar protocol dev team | [`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) |
| 2 | NEAR Intents team | [`02-near-intents-team.md`](./02-near-intents-team.md) |
| 3 | NEAR Foundation | [`03-near-foundation.md`](./03-near-foundation.md) |
| 4 | Vault curators, allocators, sentinels and associated parties | [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md) |
| 5 | Templar registry admin | [`05-templar-registry-admin.md`](./05-templar-registry-admin.md) |
| 6 | Bridge operators | [`06-bridge-operators.md`](./06-bridge-operators.md) |
| 7 | Stablecoin operators | [`07-stablecoin-operators.md`](./07-stablecoin-operators.md) |
| 8 | NEAR validators | [`08-near-validators.md`](./08-near-validators.md) |

## Categories of incident covered

Every runbook addresses, from its own role's perspective, the following
classes of incident:

1. **Protocol hacks** — exploit of a Templar market
   ([`contract/market`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/market)), the registry
   ([`contract/registry`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/registry)), the vault
   NEAR runtime ([`contract/vault/near`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/near))
   or share token, the LST oracle adapter
   ([`contract/lst-oracle`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/lst-oracle)), the proxy
   oracle ([`contract/proxy-oracle`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/proxy-oracle)),
   the Redstone adapter
   ([`contract/redstone-adapter`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/redstone-adapter)),
   or the shared vault kernel; including Hypernative-class alerts
   (see [Hypernative alert taxonomy](#hypernative-alert-taxonomy)
   below).
2. **Bad debt** — uncollateralized debt in a Templar market caused by
   liquidations that did not complete, oracle gaps, or insolvent
   borrowers. See the market-specific liquidation guidance in
   [`docs/src/contract/market/liquidate.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/contract/market/liquidate.md).
3. **Faulty oracles** — stale, manipulated, deviating, or unavailable
   [Pyth Network](https://pyth.network/) feeds (Templar's primary
   oracle per [`docs/src/oracles.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/oracles.md)), the
   LST oracle adapter derivation, the proxy oracle configuration,
   or the Redstone adapter.
4. **Registry admin compromise** — the registry contract's owner
   account is suspected stolen, lost, or misused. Note that Templar
   markets themselves are *immutable* and have no administrative
   functions (per [`docs/src/governance.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/governance.md)),
   so unlike Blend there is no per-pool admin lever; registry
   compromise governs whether new code can be published or new markets
   deployed.
5. **Curator compromise** — a Templar vault `curator` key is
   suspected compromised (the `Role::Curator` in
   [`contract/vault/curator-primitives/src/auth/mod.rs`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/curator-primitives/src/auth/mod.rs);
   the chain-agnostic policy classes are shared with the Soroban
   runtime).
6. **Allocator compromise** — a vault `allocator` key is suspected
   compromised (holds the `Allocator` and `AllocatorEmergency`
   policy classes).
7. **Sentinel compromise** — a vault `sentinel` key is suspected
   compromised (holds the `Sentinel` policy class — `Pause` and
   `SetRestrictions`).
8. **NEAR Intents-layer compromise** — a signer, solver, or
   settlement authority in the NEAR Intents stack that Templar
   depends on for cross-chain flow is suspected compromised or
   misbehaving. This class is new relative to the Blend collection
   because the Templar NEAR context is exposed to the intents layer
   as a settlement counterparty.

## Severity matrix

The same severity scale is used throughout these runbooks and matches
the Hypernative class names:

| Severity | Hypernative class | Definition | Target ack | Target containment |
|----------|-------------------|------------|------------|--------------------|
| **P0** | Critical | Exploit confirmed and funds in motion, or imminent insolvency. | < 5 min | < 30 min |
| **P1** | High | Strong evidence of exploit, key loss, or oracle / bridge / intents failure. Funds at risk but not yet in motion. | < 15 min | < 2 h |
| **P2** | Medium | Anomaly that might become P1 (parameter drift, single failed oracle update, suspicious large transfer that could be legitimate, single intents-layer anomaly). | < 1 h | < 24 h |
| **P3** | Low | Background noise that needs review (e.g. SEVERE-labelled address interaction per [`docs/src/monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md), single liquidation chain stall, isolated solver failure). | < 24 h | best effort |
| **P4** | Info | Telemetry / observability event with no required action. | n/a | n/a |

## Hypernative alert taxonomy

Hypernative (or any equivalent on-chain monitoring product) classifies
alerts into the buckets above. Within each bucket, the alert *family*
drives which runbook section applies. Mapping used throughout these
runbooks:

| Family | Typical signal | Routes to |
|--------|----------------|-----------|
| **Exploit / invariant break** | Market total supply < total borrowed beyond rounding, share-price discontinuity in a curator vault, reentrancy detection, unexpected callsite on registry / market / vault. | Protocol hack section. |
| **Oracle anomaly** | Pyth price deviation across sibling feeds, staleness beyond a market's `price_maximum_age_s`, EMA divergence, LST oracle adapter derivation drift, proxy oracle source-selection change, Redstone push-adapter freshness. | Faulty oracle section. |
| **Privileged-call anomaly** | Registry `add_version` / `deploy` / `remove_version` / `upgrade` from an unusual signer, vault `submit_set_curator` / `submit_set_sentinel` proposals, Templar tooling execution against production accounts, unexpected NEAR access-key changes on Templar-owned accounts. | Registry / curator / allocator / sentinel compromise sections. |
| **Liquidity / liquidation stress** | Market utilisation near saturation, liquidator bot backlog, LST peg deviation, withdrawal queue stalls. | Bad debt section. |
| **Counterparty signal** | Bridge halt, peg deviation on a stablecoin, sanctioned-address interaction (SEVERE label per [`monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md)), intents-layer solver failure, validator misbehaviour. | Bridge / stablecoin / intents / validator runbooks. |
| **Operational** | Cron / bot failure (liquidator, accumulator per [`monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md)), RPC outage, NEAR access-key rotation event on Templar-owned accounts, RPC provider status pages degraded. | Dev team / registry admin runbooks. |

## NEAR Safe Chain coordination model

Several incidents are too large to be handled by one party. The "NEAR
Safe Chain" model is the convention these runbooks follow:

1. **Detect locally, broadcast quickly.** The party that detects the
   incident raises severity (usually via Hypernative or internal
   monitoring — see [`docs/src/monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md)),
   opens a war room, and pages the NEAR Foundation security contact
   plus any counterparties whose surface is implicated (bridge,
   stablecoin, intents team, validator set, other integrators sharing
   the affected market / oracle / asset).
2. **Use the smallest reversible mitigation first.** Each role has
   graduated responses. Templar markets themselves have *no* on-chain
   pause lever (they are immutable per
   [`docs/src/governance.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/governance.md)); mitigation
   relies on stopping bots, informing users, coordinating with oracle
   providers and bridges, and deploying a patched market via registry
   so users can migrate voluntarily.
3. **Containment before forensics.** Stop the bleed first; preserve
   evidence second; root-cause third. All public communication is
   funnelled through the lead role for that incident class.
4. **Stand-down requires sign-off from at least two NEAR Safe Chain
   roles.** Every runbook documents which other role is required for
   stand-down of each incident class.

The NEAR Safe Chain is a *coordination* layer, not a custody layer.
No single Safe Chain role can move user funds; it only coordinates the
communicate / patch / migrate / unwind workflow. Note that NEAR
validators do *not* provide a transaction-censorship surface — the
NEAR Safe Chain intentionally does not ask validators to censor.

## War room template

Every P0 / P1 spawns a war room. Use this template:

```text
Incident: <one-line description>
Severity: P0 | P1 | P2
Detected: <ISO-8601 UTC>
Detector: <human or alert source>
On-call lead: <name / handle>
Comms lead:  <name / handle>
Scribe:      <name / handle>

Affected:
  - Templar market(s): <account IDs>
  - Vault(s): <account IDs>
  - Oracle feed(s): <price identifiers>
  - Bridge / stablecoin / intents surface: <names>
  - NEAR account(s): <account IDs>

Hypothesis: <current best guess at cause>

Containment actions taken:
  - <ts> <role> <action> <tx hash>

Pending decisions:
  - <decision> — owner: <name>

Comms log:
  - <ts> <channel> <message>
```

Keep this in a shared, append-only document (war room tool, encrypted
collaborative editor, or pinned chat thread). Do not rely on memory.

## Glossary

- **Market** — an immutable Templar money market on NEAR (see
  [`contract/market`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/market) and
  [`docs/src/contract/market/index.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/contract/market/index.md)).
  Once deployed and locked, configuration cannot be changed. There is
  no admin pause.
- **Registry** — the singleton contract at
  [`contract/registry`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/registry) that catalogs
  Templar contract code versions and deploys new markets. Its owner
  can `add_version` (publish new code), `remove_version`, `deploy`
  new instances, and `upgrade` the registry itself. Immutable once
  the owner is renounced (not the current standing state — see
  [`05-templar-registry-admin.md`](./05-templar-registry-admin.md)).
- **Vault** — a Templar curator vault
  ([`contract/vault/near`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/near) NEAR runtime
  atop the shared kernel) that supplies into one or more markets.
  Uses the same `Role::Curator` / `Role::Sentinel` /
  `Role::Allocator` model as the Soroban runtime.
- **LST oracle adapter** — [`contract/lst-oracle`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/lst-oracle);
  derives Liquid Staking Token prices from underlying-asset Pyth
  prices (see [`docs/src/oracles.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/oracles.md)).
- **Proxy oracle** — [`contract/proxy-oracle`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/proxy-oracle);
  configurable aggregator with mutable source selection and freshness
  filters. Mutable via its own governance, unlike the market's
  reserve config.
- **Redstone adapter** — [`contract/redstone-adapter`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/redstone-adapter);
  bridge from Redstone push feeds into the Templar oracle interface.
- **SEVERE address monitoring** — Telegram-notified detection of
  SEVERE-labelled accounts interacting with Templar (see
  [`docs/src/monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md) and the
  [`templar-monitoring` repo](https://github.com/Templar-Protocol/templar-monitoring)).
- **NEAR Intents** — the NEAR-ecosystem cross-chain intent-based
  settlement stack (canonical intents contract, solvers, settlement
  authorities). Templar is exposed to it as a liquidity source and
  settlement counterparty; see the
  [`02-near-intents-team.md`](./02-near-intents-team.md) runbook.
- **NEAR Foundation** — the ecosystem-coordination role played
  during multi-party incidents; equivalent role to Stellar Foundation
  in the Blend collection.

## Reading order for new responders

1. This `README.md`.
2. The runbook for your role.
3. The runbook for any adjacent role you may need to coordinate with
   during a NEAR Safe Chain incident.

## Maintenance

These runbooks are versioned with the protocol. When the on-chain
interface changes (market entrypoints, registry semantics, oracle
adapter surfaces, vault role names), update all affected runbooks in
the same PR. Contact details and key fingerprints are deliberately
not stored here — keep those in private operational systems.
