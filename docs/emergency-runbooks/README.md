# Blend Emergency Response Runbooks

This directory contains a public, role-segmented set of runbooks for responding to
emergencies that affect the Blend lending protocol on Stellar, the curator vaults
that build on top of it (e.g. the Templar vaults that integrate with Blend via
the `blend-adapter` in
[`Templar-Protocol/contracts`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/soroban)),
and the surrounding ecosystem (bridges, stablecoins, validators).

These runbooks are inspired by the Morpho curator emergency procedures (see
[Morpho Vaults V1 emergency](https://docs.morpho.org/curate/tutorials-v1/emergency/),
[Vaults V2 emergency](https://docs.morpho.org/curate/tutorials-v2/emergency/),
[bad debt](https://docs.morpho.org/curate/tutorials-v2/bad-debt/), and
[security considerations](https://docs.morpho.org/curate/concepts/security-considerations/))
and adapted for Blend's pool architecture, Stellar's Soroban runtime, and the
"Stellar Safe Chain" coordination model used to coordinate across protocol,
foundation, validator and counterparty teams during ecosystem-level incidents.

## Audience

There is one runbook per role. Each runbook is self-contained and assumes the
reader will not have read the others. They are intended to be opened during an
incident and followed top-to-bottom.

| # | Role | File |
|---|------|------|
| 1 | Blend protocol dev team | [`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md) |
| 2 | Blend pool admins | [`02-blend-pool-admins.md`](./02-blend-pool-admins.md) |
| 3 | Stellar Foundation | [`03-stellar-foundation.md`](./03-stellar-foundation.md) |
| 4 | Vault curators, allocators, sentinels and associated parties | [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md) |
| 5 | Bridge operators | [`05-bridge-operators.md`](./05-bridge-operators.md) |
| 6 | Stablecoin operators | [`06-stablecoin-operators.md`](./06-stablecoin-operators.md) |
| 7 | Stellar validators | [`07-stellar-validators.md`](./07-stellar-validators.md) |

## Categories of incident covered

Every runbook addresses, from its own role's perspective, the following classes
of incident:

1. **Protocol hacks** — exploit of the Blend pool / backstop / emitter / pool factory,
   or of a curator vault / governance / share token / Blend adapter, including
   Hypernative-class alerts (see [Hypernative alert taxonomy](#hypernative-alert-taxonomy)
   below).
2. **Bad debt** — uncollateralized debt produced by liquidations that did not
   complete, oracle gaps, or insolvent borrowers.
3. **Faulty oracles** — stale, manipulated, deviating, or
   unavailable Pyth / Stellar Reflector / SEP-40 / Redstone price feeds.
4. **Pool admin compromise** — Blend pool `admin` key (set via `propose_admin` /
   `accept_admin`) is suspected stolen, lost, or misused.
5. **Curator compromise** — a vault `curator` key is suspected compromised
   (Templar's `Role::Curator` in
   [`curator-primitives/src/auth/mod.rs`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/curator-primitives/src/auth/mod.rs)
   ).
6. **Allocator compromise** — a vault `allocator` key is suspected compromised
   (Templar's `Role::Allocator`, holds the `Allocator` and `AllocatorEmergency`
   policy classes).
7. **Sentinel compromise** — a vault `sentinel` key is suspected compromised
   (Templar's `Role::Sentinel`, holds the `Sentinel` policy class — `Pause` and
   `SetRestrictions`).

## Severity matrix

The same severity scale is used throughout these runbooks and matches the
Hypernative class names:

| Severity | Hypernative class | Definition | Target ack | Target containment |
|----------|-------------------|------------|------------|--------------------|
| **P0** | Critical | Exploit confirmed and funds in motion, or imminent insolvency. | < 5 min | < 30 min |
| **P1** | High | Strong evidence of exploit, key loss, or oracle/bridge failure. Funds at risk but not yet in motion. | < 15 min | < 2 h |
| **P2** | Medium | Anomaly that might become P1 (parameter drift, single failed oracle update, suspicious large transfer that could be legitimate). | < 1 h | < 24 h |
| **P3** | Low | Background noise that needs review (e.g. SEVERE-labelled address interaction, single liquidation chain stall). | < 24 h | best effort |
| **P4** | Info | Telemetry / observability event with no required action. | n/a | n/a |

## Hypernative alert taxonomy

Hypernative (or any equivalent on-chain monitoring product) classifies alerts
into the buckets above. Within each bucket, the alert *family* drives which
runbook section applies. Mapping used throughout these runbooks:

| Family | Typical signal | Routes to |
|--------|----------------|-----------|
| **Exploit / invariant break** | Pool `total_supply` < `total_borrowed`, share price drops > threshold, unexpected `gulp` callsite, reentrancy detection. | Protocol hack section. |
| **Oracle anomaly** | Price deviation across feeds, staleness > `price_maximum_age_s`, confidence band collapse, EMA divergence. | Faulty oracle section. |
| **Privileged-call anomaly** | `set_admin` / `accept_admin`, `set_status` to 4 (admin-frozen), `queue_set_reserve` with extreme params, vault `submit_set_curator` / `submit_set_sentinel` proposals. | Pool / curator / allocator / sentinel compromise sections. |
| **Liquidity / liquidation stress** | Backstop `q4w_pct` rising above thresholds (30% / 50% / 60% / 75%), liquidation queue stalls, utilisation rate near `max_util`. | Bad debt section. |
| **Counterparty signal** | Bridge halt, peg deviation on a stablecoin, sanctioned-address interaction, validator misbehaviour. | Bridge / stablecoin / validator runbooks. |
| **Operational** | Cron / bot failure, RPC outage, ledger TTL near expiration on critical contracts. | Pool admin / dev team runbooks. |

## Stellar Safe Chain coordination model

Several incidents are too large to be handled by one party. The "Stellar Safe
Chain" model is the convention these runbooks follow:

1. **Detect locally, broadcast quickly.** The party that detects the incident
   raises severity (usually via Hypernative or internal monitoring), opens a
   war room, and pages the Stellar Foundation security contact and any
   counterparties whose surface is implicated (bridge, stablecoin, validator
   set, other curators sharing the affected pool / oracle / asset).
2. **Use the smallest reversible mitigation first.** Each role has graduated
   responses (e.g. for Blend pool admins: `set_status(2)` admin on-ice →
   `set_status(4)` admin frozen). The runbooks document those step ladders
   so responders do not jump straight to the most invasive action.
3. **Containment before forensics.** Stop the bleed first; preserve evidence
   second; root-cause third. All public communication is funnelled through
   the lead role for that incident class.
4. **Stand-down requires sign-off from at least two Safe Chain roles.** Even
   if the on-chain action was taken by a single key (e.g. an admin-frozen
   pool), unfreezing requires a second role to corroborate that the cause is
   resolved (e.g. dev team confirms patched contract, or curator confirms
   vault deallocation completed).

The Safe Chain is a *coordination* layer, not a custody layer. No single Safe
Chain role can move user funds; it only coordinates the pause / unwind / patch
/ communicate workflow.

## War room template

Every P0/P1 spawns a war room. Use this template:

```
Incident: <one-line description>
Severity: P0 | P1 | P2
Detected: <ISO-8601 UTC>
Detector: <human or alert source>
On-call lead: <name / handle>
Comms lead:  <name / handle>
Scribe:      <name / handle>

Affected:
  - Blend pool(s): <addresses>
  - Reserve(s) / asset(s): <addresses>
  - Vault(s): <addresses>
  - Bridge / stablecoin: <names>
  - Oracle feed(s): <ids>

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

- **Backstop** — Blend's per-pool insurance module; receives bad debt when a
  user is liquidated to zero collateral. Defined in
  [`blend-contracts-v2/backstop`](../../backstop).
- **Pool status** — integer 0–6 that controls which user actions are allowed.
  Even numbers are admin-set, odd numbers are backstop-driven, and 4
  (admin-frozen) supersedes everything else. See
  [`pool/src/pool/status.rs`](../../pool/src/pool/status.rs).
- **q4w_pct** — fraction of backstop deposits queued for withdrawal. Drives
  status transitions at 30% / 50% / 60% / 75%.
- **Reserve config** — per-asset risk parameters (`c_factor`, `l_factor`,
  `max_util`, etc.) on a pool. Queued via `queue_set_reserve`.
- **Curator vault** — an ERC-4626-style vault that supplies into one or more
  Blend pool reserves on behalf of depositors.
- **Adapter** — chain- or pool-specific bridge between a vault and the venue
  it allocates into. Templar's Blend adapter lives at
  [`contract/vault/soroban/blend-adapter`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/soroban/blend-adapter).
- **Timelock** — delay (configured at vault deployment) between proposing a
  governance change and executing it. Used by
  [`SorobanVaultGovernanceContract`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/governance/src/lib.rs).

## Reading order for new responders

1. This `README.md`.
2. The runbook for your role.
3. The runbook for any adjacent role you may need to coordinate with during a
   Safe Chain incident.

## Maintenance

These runbooks are versioned with the protocol. When the on-chain interface
changes (status codes, role names, governance actions), update all affected
runbooks in the same PR. Contact details and key fingerprints are deliberately
not stored here — keep those in private operational systems.
