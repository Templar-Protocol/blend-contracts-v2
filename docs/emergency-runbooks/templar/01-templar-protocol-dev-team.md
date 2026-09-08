# Runbook: Templar Protocol Dev Team

**Audience.** Engineers and on-call responders who maintain the Templar
Protocol NEAR contracts and the deployment / monitoring tooling around
them: markets ([`contract/market`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/market)), registry
([`contract/registry`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/registry)), vault kernel and
NEAR runtime ([`contract/vault/near`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/near)),
oracle adapters (LST, proxy, Redstone), operator services, liquidator
and accumulator bots.

**Scope.** You are the *protocol* responder. You do not generally hold
the registry admin key (that is described separately in
[`05-templar-registry-admin.md`](./05-templar-registry-admin.md)) and
you do not hold vault curator / sentinel / allocator keys (those live
with the curator operators, see
[`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)).
You own the contracts, the build, the audit history, and the migration
story. You are the authoritative source of truth on what each market /
vault / oracle / registry call *actually does* on-chain.

Templar markets on NEAR are **immutable** with **no administrative
functions** (per
[`docs/src/governance.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/governance.md)): once a market is
deployed, there is no pause, no upgrade, no parameter change. This is a
deliberate design constraint that shapes every incident response —
mitigation is user-migration-driven, not pause-driven.

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

Before any incident, the dev team must keep the following in a
known-good state:

- A monitored replica of every deployed Templar market's state (per
  [`docs/src/monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md): `list_deployments`
  on the registry gives the current market set) with the ability to
  replay incident transactions in a local sandbox.
- A pre-built, signed, reproducible WASM artifact for every contract
  version currently in production. Builds must match `audits/`-referenced
  commits.
- A list of every deployed market with: version code hash, configured
  oracle account and price identifiers, collateral / borrow asset
  decimals, MCR, interest-rate model, `price_maximum_age_s`, current
  snapshot.
- Hypernative (or equivalent) feeds wired to at least: registry
  `add_version`, `add_version_01_finalize`, `remove_version`,
  `deploy`, `deploy_01_finalize`, `upgrade`; NEAR access-key changes on
  Templar-owned accounts (registry owner, oracle adapter admin, vault
  governance addresses, tooling accounts); vault `submit_*` proposals;
  market events (supply, withdraw, borrow, repay, liquidate,
  bad_debt); oracle staleness against each market's
  `price_maximum_age_s`; liquidator and accumulator bot health.
- A war room channel that can be joined within 5 minutes by:
  - Dev on-call (rotating).
  - Registry admin.
  - Every vault curator operator we publicly support.
  - NEAR Foundation security contact.
  - Contacts at every major bridge, stablecoin issuer, and the NEAR
    Intents team (for cross-stack incidents).
- A pre-drafted public statement template ("we are aware of a potential
  incident on Templar market X, investigating, no action required from
  users yet") so comms is not blocked on writing prose.

---

## 1. Triage

When a P0 / P1 alert fires:

1. **Open the war room** using the template in
   [`README.md`](./README.md#war-room-template). The dev on-call is
   the default *lead* for protocol hacks on Templar contracts. For
   bad-debt and faulty-oracle incidents, the dev on-call acts as
   *technical advisor* — the affected vault curator (for bad debt on
   their supplied reserves) or the oracle provider (for oracle
   faults) is the canonical lead per the dependency map in
   [`03-near-foundation.md`](./03-near-foundation.md) §1. For
   registry-admin / curator / allocator / sentinel / NEAR Intents
   compromise, the dev on-call acts as *technical advisor* and the
   affected role is lead. The stand-down quorum for any
   protocol-hack incident on Templar contracts follows the
   canonical NEAR Safe Chain rule in
   [`README.md`](./README.md#near-safe-chain-coordination-model) —
   sign-off from at least two NEAR Safe Chain roles. Concretely:
   the dev-team lead (this runbook) *plus* one of {registry admin
   (only while registry-admin control remains intact), NEAR
   Foundation}. If the registry-admin key has been captured, the
   second signer must be NEAR Foundation, and Foundation additionally
   pulls in an independent third role per
   [`03-near-foundation.md`](./03-near-foundation.md) §9 ("Registry
   admin compromise — admin captured"). Never accept sign-off from a
   potentially compromised credential.
2. **Classify** the alert family using the table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
3. **Snapshot state.** Before any mitigation, record for every
   affected market: current snapshot (`get_current_snapshot`),
   supply and borrow positions
   (`list_supply_positions`, `get_borrow_asset_metrics`), oracle
   freshness (`pyth-oracle.near` `get_price` for each asset),
   withdrawal queue (`get_supply_withdrawal_queue_status`), and the
   last several finalized snapshots (`list_finalized_snapshots`).
4. **Decide containment** with the relevant role(s). The dev team's
   role in containment is to confirm that the proposed action is
   consistent with contract semantics and will not make things
   worse. Do not advise any action you have not personally
   re-derived from the source. Remember that markets have no on-
   chain pause — most protocol-hack containment is off-chain
   (halt bots, notify users, coordinate with counterparties) plus a
   registry-side patched-version deployment for migration.

---

## 2. Protocol hack

A protocol hack is any state where the on-chain invariants of a
Templar market / registry / vault / oracle adapter are violated, or
where an attacker is extracting value from a contract in a way the
design did not contemplate.

### 2.1 Detection (Hypernative classes)

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Confirmed invariant break (e.g. market supply / borrow accounting mismatch beyond rounding), unexpected outbound transfer from a market or vault, share-price discontinuity in a curator vault, reentrancy detection, unexpected callsite on the registry. |
| **High (P1)** | Unusual call pattern: rapid `submit`-like chains from one address, abnormal `flash_loan`-equivalent flows (Templar markets currently have no flash loan primitive), repeated liquidation-fill patterns from unknown fillers. |
| **Medium (P2)** | Anomaly in oracle inputs combined with unusual borrow growth; large single-borrower position that is close to the MCR while oracle updates are lagging. |

### 2.2 Containment workflow

Because markets are immutable, containment on the Templar side is
mostly **off-chain and user-migration-driven**:

1. **Halt operator bots.** Pause the liquidator bot and accumulator
   bot (see [`docs/src/monitoring.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/monitoring.md)) so
   the protocol does not amplify the exploit through automated
   activity. Any bot pause has to be explicit — a silent stall
   during an incident is worse than a broadcast pause.
2. **Coordinate with the registry admin** (see
   [`05-templar-registry-admin.md`](./05-templar-registry-admin.md)).
   You will likely need `add_version` and `deploy` calls to publish
   a patched market, but do not rush the sign-off — a compromised
   registry admin is a much larger blast radius than an
   individual-market hack.
3. **Coordinate with vault curators** (see
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)).
   Vaults supplying into an affected market can pause activity at
   the vault edge via the Sentinel — Templar governance exposes a
   direct `set_paused(sentinel, true)` entrypoint that executes
   immediately (gated by `require_sentinel`; the timelocked
   `submit_set_paused(true)` path is rejected with `InvalidInput`).
   The Sentinel can also revoke a bounded set of pending proposals
   per `can_revoke_kind`. Even though the market itself cannot be
   paused, vault-edge pause protects vault depositors.
4. **Coordinate with NEAR Foundation** via the security contact (see
   [`03-near-foundation.md`](./03-near-foundation.md)). Foundation
   is responsible for cross-protocol communication with bridges,
   validators, stablecoin issuers, and the NEAR Intents team if the
   incident has wider blast radius.
5. **Preserve evidence.** Once bots are halted, snapshot the market
   state (positions, snapshots, receipts on the exploit
   transactions), full oracle price history around the incident,
   and any Templar-owned account access-key state. Do *not* delete
   any operator-bot logs; they may be evidence.

### 2.3 Recovery

Templar markets have **no upgradeability primitive**. Recovery from a
market-level exploit means:

1. Cut a patched commit. Update `audits/` with a delta note describing
   the issue and the fix.
2. Build reproducibly; publish artifact and source.
3. Coordinate `add_version` and `deploy` with the registry admin (see
   [`05-templar-registry-admin.md`](./05-templar-registry-admin.md)).
4. Coordinate user migration: users withdraw from the affected
   market and re-supply into the new market. Publish precise
   instructions and a migration deadline; the affected market
   remains callable by users indefinitely (no pause exists to
   force migration), so migration comms and vault-side
   deallocation are the levers.
5. Sunset the old market by leaving it out of user-facing tooling
   and vault supply queues, and document the migration in a public
   post-mortem.

Stand-down requires sign-off from the dev on-call lead **and** one
of {registry admin (only while control remains intact), NEAR
Foundation} — the two-signer NEAR Safe Chain rule from
[`README.md`](./README.md#near-safe-chain-coordination-model). If
the registry admin key was captured during the incident, the
second signer must be NEAR Foundation (plus the independent
third-role addendum in
[`03-near-foundation.md`](./03-near-foundation.md) §9).

For registry-level exploits, `contract/registry` itself is deployable
under a patched version *only* if the current registry owner is not
compromised and the owner accepts the risk of `upgrade`. If the
registry owner is compromised, migration means deploying a new
registry account and asking counterparties (vaults, integrators, front
ends) to switch to it — a much larger operation.

For vault runtime / kernel exploits, work with the affected vault
curator via [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md).

---

## 3. Bad debt

Bad debt arises when a borrower's position drops below MCR and the
liquidation flow cannot fully clear the collateral before the position
goes underwater. In Templar markets, unlike Blend, there is no
per-pool "backstop" that absorbs bad debt — losses accrue to suppliers
in the affected market pro-rata. This makes early detection and
communication especially important.

### 3.1 Detection

| Class | Signal |
|-------|--------|
| **High (P1)** | Multiple bad-debt candidates identified; liquidator bot backlog; oracle staleness preventing liquidation; utilization approaching saturation on a market whose collateral is under peg stress. |
| **Medium (P2)** | Single user with negative health that liquidators cannot clear; borrow-asset metrics show reserved liquidity dropping below the threshold liquidators need. |
| **Low (P3)** | Liquidation chain stalls, but health factors recover within minutes. |

### 3.2 Dev team responsibilities

1. **Verify the chain of events.** Was a borrower liquidated? Did
   the liquidator bot fail? Is the failure oracle-driven, liquidity-
   driven, or bot-driven?
2. **Coordinate with vault curators first**, in this order:
   (a) Sentinel `set_restrictions(sentinel, ...)` to block deposits
   / withdrawals at the vault edge (immediate; `SyncExternalAssets`
   is an Allocator action and is *not* in `allowed_while_paused`, so
   a pause first would leave the vault stuck with the stale share
   price until unpause);
   (b) Allocator `SyncExternalAssets` to propagate loss recognition
   into the vault's `total_assets` and share price;
   (c) *then* Sentinel `set_paused(sentinel, true)` (immediate) to
   halt allocator activity for the remainder of the incident.
   Per [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md).
   *Loss must be recognised in share price before public exit
   guidance* — otherwise early withdrawers redeem at the stale
   overstated share price and concentrate the loss on remaining
   suppliers.
3. **Trace and communicate loss recognition.** Once loss is on-chain
   and reflected in share price (`SyncExternalAssets` complete),
   compute the actual share-price impact using the market snapshot
   and publish it. This is a post-recognition public disclosure,
   not a pre-recognition advance-warning to a subset of holders.

### 3.3 Recovery

If bad debt is the result of a one-off market move, no contract
change is needed — the loss propagates to suppliers and share prices
adjust. If it is the result of a market parameter mistake (MCR,
liquidation incentive, interest-rate model), the mitigation is to
deploy a corrected market via registry and migrate users. Update
audit notes and recommended-parameter guidance.

---

## 4. Faulty oracle

Templar markets accept any oracle contract at construction (typically
`pyth-oracle.near` for Pyth, `lst.oracle.tmplr.near` for LST-derived
prices, and forthcoming proxy-oracle deployments). Faults include:

- **Stale prices** — Pyth is a pull oracle; if no one pushes fresh
  prices, borrow and liquidation operations will fail. This is
  self-limiting but can cascade into bad debt if it persists.
- **Manipulated prices** — a fresh but wrong price is *worse* than a
  stale one.
- **Confidence collapse** — Pyth confidence band collapses; the
  underlying data sources are degraded.
- **Wrong feed wired in** — the wrong `price_id`, `decimals`, or
  `price_maximum_age_s` was configured for a market.
- **LST oracle adapter derivation drift** — the derivation from
  underlying prices produces an anomalous LST price.
- **Proxy oracle misconfiguration** — for markets that price via the
  Templar proxy oracle, source selection or freshness filter can be
  mutated by the proxy's own governance and may need correcting.

### 4.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Price deviation > 5% across sibling feeds, or price moves > N% in one block with no off-chain explanation. |
| **High (P1)** | Single-feed staleness > 2× the market's `price_maximum_age_s`; oracle provider acknowledges incident. |
| **Medium (P2)** | Confidence band collapse; one publisher in a multi-publisher Pyth feed disconnects; LST adapter derivation drift within a single block. |

### 4.2 Containment

Markets have no pause. Containment is:

1. Halt operator bots (liquidator, accumulator).
2. Publicly communicate to users and vault curators that the
   affected feed is unreliable and to avoid new borrows / supplies
   until the all-clear.
3. Coordinate with the oracle provider — for Pyth, via NEAR
   Foundation contacts
   ([`03-near-foundation.md`](./03-near-foundation.md) §4).
4. Coordinate with vault curators so they can `RebalanceWithdraw`
   away from any market pricing via the affected feed.
5. For proxy-oracle misconfiguration, work with the proxy-oracle
   operator to correct the source selection or freshness filter via
   the proxy's own governance.

### 4.3 Recovery

1. Wait for the oracle to recover, or for the provider to publish
   guidance on which prices are safe.
2. For wiring faults, the split is:
   - **Immutable market fields** (oracle account address, price
     identifiers, decimals, `price_maximum_age_s` — see
     [`docs/src/oracles.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/oracles.md)) → fix requires
     deploying a corrected market via registry and migrating
     users.
   - **Mutable proxy-oracle fields** (source selection, per-source
     freshness filter, aggregation strategy in
     [`contract/proxy-oracle`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/proxy-oracle)) →
     fix via the proxy's own governance while markets remain live.
3. Restart operator bots only after the all-clear and after the
   corrected feed has been observed to be healthy for at least a
   full liquidation cycle.

---

## 5. Registry admin compromise

Registry admin compromise is the highest-blast-radius incident on
Templar because the registry can `add_version` (publish new code)
and `deploy` (spawn new deployments); a compromised admin could
publish backdoored code and deploy fake markets that look official.

### 5.1 Dev team role

You do not have registry custody. Your job is:

1. **Verify the anomaly.** Cross-check any suspicious `add_version`
   / `deploy` / `remove_version` / `upgrade` transaction against
   the registry admin's known signing posture.
2. **If the admin is reachable**, support them in following
   [`05-templar-registry-admin.md`](./05-templar-registry-admin.md) §5.
3. **If the admin is not reachable or is suspected captured**,
   escalate immediately to NEAR Foundation
   ([`03-near-foundation.md`](./03-near-foundation.md) §5). Publish
   a factual statement identifying: which registry account is
   affected, which versions or deployments are now considered
   untrusted, and how users / vault curators / integrators can
   verify a deployment's provenance out-of-band (e.g. via
   `get_version_code_hash` against known-good hashes).
4. **Plan the fallback registry deployment**: a fresh registry
   account under a clean admin, publish the known-good versions
   into it, migrate integrators.

---

## 6. Curator / allocator / sentinel compromise

These are *vault* roles, not market roles. The dev team's
responsibilities are indirect. See
[`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)
for the vault-side response. Dev team supports by:

1. Confirming the blast radius: which Templar markets does the
   compromised vault supply into?
2. Watching for second-order effects on those markets: sudden supply
   / withdrawal spikes, utilisation crossing high-usage bands, burst
   deposit / withdraw cycles that could indicate manipulation.
3. Preparing rapid `add_version` + `deploy` if the response requires
   deploying a fresh vault runtime.

---

## 7. NEAR Intents-layer incident

NEAR Intents is a Templar counterparty for cross-chain settlement
flow. Compromise or misbehavior in the intents layer can affect
Templar liquidity. See
[`02-near-intents-team.md`](./02-near-intents-team.md) for the
intents-side response. Dev team supports by:

1. Confirming Templar's exposure to the affected intents route or
   solver.
2. Coordinating with vault curators whose vaults are exposed via
   intents-mediated flow.
3. Assisting with post-mortem analysis of any intents-layer
   transactions that touched Templar markets.

---

## 8. Communication protocol

- **Public statements** during a P0 / P1 are issued only by the dev
  on-call lead or the comms lead in the war room. No-one else
  speaks publicly.
- **Channels**: official protocol social, status page, GitHub
  security advisory once a fix is shipped. Coordinate with NEAR
  Foundation before publishing if the incident has cross-protocol
  impact.
- **What to publish at each phase**:
  - *Detection*: "We are investigating an incident affecting market
    X. Bots have been halted. User funds are not currently in
    motion. Do not initiate new borrows or supplies until further
    notice." Or explicit user instructions if there are any.
  - *Containment*: what state each affected market is in, what bots
    are halted, what timeline the war room expects.
  - *Recovery*: migration steps, deadlines, new market addresses,
    verification instructions (`get_version_code_hash` against
    published hash).
  - *Post-mortem*: published only after NEAR Safe Chain stand-down,
    including audit-of-the-fix references and parameter changes.

---

## 9. Stand-down checklist

Before declaring an incident resolved, the dev on-call lead must
confirm:

- [ ] Root cause identified and reproduced in a test.
- [ ] Patch (if any) audited or peer-reviewed by at least one
      engineer not on the on-call rotation.
- [ ] All affected markets have either been migrated or their
      user-facing exposure is deprecated in tooling.
- [ ] All affected vault curators have stepped down to normal
      posture.
- [ ] Registry admin sign-off (Safe Chain rule) that any new
      versions and deployments are legitimate and audited.
- [ ] NEAR Foundation contact has been told the incident is closed.
- [ ] Public post-mortem drafted and queued.
- [ ] War room archived (append-only log preserved).
- [ ] Hypernative / monitoring rules updated to detect the same
      class of event earlier next time.

A sign-off from a second NEAR Safe Chain role is required before
user-visible action is reversed (bots restarted, migration deadline
lifted) — this is the two-signer canonical rule from
[`README.md`](./README.md#near-safe-chain-coordination-model). For
Templar protocol-hack stand-down the second signer is either the
registry admin (only while control remains intact) or NEAR
Foundation per the quorum table in
[`03-near-foundation.md`](./03-near-foundation.md) §9. If the
registry admin key was captured, use Foundation + an independent
third role from that table's captured-admin row.
