# Runbook: Vault Curators, Allocators, Sentinels and Associated Parties

**Audience.** Operators of curator vaults that supply assets into Blend
pools. The reference vault stack used throughout this document is the
Templar vault implementation in
[`Templar-Protocol/contracts`](https://github.com/Templar-Protocol/contracts):

- Soroban runtime: [`contract/vault/soroban/src/`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/soroban/src)
- Governance contract: [`contract/vault/soroban/governance/src/lib.rs`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/governance/src/lib.rs)
- Share token (SEP-41): [`contract/vault/soroban/share-token/`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/soroban/share-token)
- Blend adapter: [`contract/vault/soroban/blend-adapter/src/lib.rs`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/blend-adapter/src/lib.rs)
- Chain-agnostic role + policy primitives: [`contract/vault/curator-primitives/`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/curator-primitives)

If you operate a different vault implementation, the role / policy class
mapping will be analogous; the on-chain entrypoint names will differ.

This runbook is inspired by and adapts ideas from Morpho's vault emergency
documentation ([V1](https://docs.morpho.org/curate/tutorials-v1/emergency/),
[V2](https://docs.morpho.org/curate/tutorials-v2/emergency/),
[bad debt](https://docs.morpho.org/curate/tutorials-v2/bad-debt/),
[security considerations](https://docs.morpho.org/curate/concepts/security-considerations/))
to Blend's Soroban architecture.

Read [`README.md`](./README.md) first for the severity matrix, Hypernative
classification, war room template, and Safe Chain coordination model.

---

## 0. Standing posture

### 0.1 Role separation

The vault stack uses three (sometimes four) named roles, each with its own
key and its own threat model. From
[`curator-primitives/src/auth/mod.rs`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/curator-primitives/src/auth/mod.rs):

| Role | Auth policy class | Actions it can take | When to use |
|------|-------------------|---------------------|-------------|
| **Curator** | `Curator` | `ManualReconcile`, `EmergencyReset`, `PolicyAdmin` (also the proposer for almost all governance actions). | Slowest, most powerful. Proposes timelocked policy changes (caps, fees, supply queue, market addition / removal, role changes). |
| **Allocator** | `Allocator` | `ExecuteWithdraw`, `BeginAllocating` / `FinishAllocating`, `SyncExternalAssets`, `RebalanceWithdraw`, `BeginRefreshing` / `FinishRefreshing`, `SettlePayout`, `RefreshFees`. | Day-to-day rebalancing within bounds set by curator. Hot key. |
| **AllocatorEmergency** | `AllocatorEmergency` | `AbortAllocating`, `AbortWithdrawing`, `AbortRefreshing`. | Cancel a stuck or in-progress operation. Often the same key as Allocator, but conceptually separable. |
| **Sentinel** | `Sentinel` | `Pause`, `SetRestrictions`. | Reactive risk reduction. Should be a hotter key than the curator and *separate* from the allocator. |
| **Public** (depositors) | `Public` | `Deposit`, `RequestWithdraw`, `AtomicWithdraw`, `AtomicRedeem`. | n/a |

The Sentinel exists specifically so that risk reduction does not require
the slow Curator timelock. Configure it with that in mind. **The Sentinel
should never share a key with the Curator or the Allocator.**

The vault governance contract additionally has:

| Governance role | Actions |
|-----------------|---------|
| **Admin / Owner** of governance contract | Acts as the canonical proposer / canonical timelock controller. Every `submit_*` on Templar's [governance](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/governance/src/lib.rs) goes through `require_admin`. Admin can `revoke` any pending proposal. Holds emergency abdication. |
| **Skim recipient** | Receives skim drains. |

Only the Admin and the Sentinel appear in the governance contract's
`RevokerRole` enum — there is no separate Guardian role in the
current Templar Soroban governance implementation.

### 0.2 Operational hygiene

Before any incident:

- **Hardware-back every privileged role** (Curator, Sentinel,
  AllocatorEmergency, Governance Admin). Allocator can be a hot key but
  should be protected by per-call value limits at the off-chain
  signing layer.
- **Separate rotation cadence** per role. A single rotation event should
  not require all four roles to sign at once.
- **Per-vault dashboards** showing: idle balance, allocated principal per
  market, share price, current cap per market, current fees, pending
  governance proposals, last refresh time, withdrawal queue length.
- **Hypernative or equivalent** monitoring on at least: governance
  `submit_*` actions, `set_paused`, `set_restrictions`, allocator
  rebalance actions, share-price discontinuities, allocator
  beginning/finishing operations that do not match the vault's
  refresh schedule.
- **A cold-start drill** — at least once per quarter, run through the
  containment ladder in section 2.2 on a testnet vault end-to-end and
  measure time-to-pause. Target: under 10 minutes for the Sentinel to
  call `set_paused(sentinel_address, true)` (direct pause, executes
  immediately) and observe the vault reject a subsequent public
  action.
- **Pre-shared incident contacts** with: Blend protocol dev team
  ([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)),
  every Blend pool admin you supply into
  ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md)), Stellar
  Foundation ([`03-stellar-foundation.md`](./03-stellar-foundation.md)).

### 0.2b Vault-side vs ecosystem-level stand-down

Every §X.3 recovery section in this runbook lists a **vault-side**
stand-down quorum — the sign-off the *curator* requires before lifting
the vault's own pause / restrictions / caps. This is separate from the
**ecosystem-level** stand-down quorum defined in the canonical Safe
Chain quorum table
([`03-stellar-foundation.md`](./03-stellar-foundation.md) §8) which
governs the incident as a whole. Vault-side stand-down may legitimately
be stricter than the ecosystem quorum for that incident class — for
example, the curator may require Sentinel + Foundation before lifting
a pause the Sentinel set, even if the ecosystem stand-down only
requires the incident lead + Foundation.

### 0.3 Timelock posture

The vault governance contract uses configurable per-action timelocks (see
`submit_set_paused` (unpause only — `SetPaused(true)` is rejected here;
see the direct `set_paused` path below), `submit_set_curator`,
`submit_set_governance`, `submit_set_supply_queue`, `submit_set_fees`,
`submit_set_restrictions`, `submit_set_sentinel`, `submit_set_cap`,
`submit_remove_market`, `submit_set_group_cap`, `submit_set_group_rel_cap`,
`submit_set_group_member`, `submit_set_skim_recipient`, `submit_skim`,
`submit_set_timelock`).

Following Morpho's
[security considerations](https://docs.morpho.org/curate/concepts/security-considerations/):

- **Risk-reducing actions should be immediate or near-immediate.** In
  the Templar governance contract the canonical example is
  `set_paused(sentinel, true)` — a direct entrypoint gated by
  `require_sentinel` that executes immediately (there is no timelock
  path for pausing; `submit_set_paused(true)` is rejected with
  `InvalidInput`). Cap decreases follow a similar immediate pattern.
- **Risk-increasing actions must have non-trivial timelocks** (Morpho
  recommends up to 3 weeks; choose per your governance posture). Cap
  increases, fee increases, adding markets, raising max growth rate,
  changing the curator, and *un*pause (`submit_set_paused(false)` is
  the only pause-related action that goes through the timelock).
- **The Sentinel can revoke a specific set of pending governance
  proposals.** Per `can_revoke_kind` in
  [governance](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/governance/src/lib.rs),
  the Sentinel may revoke: `Pause` (i.e. a pending unpause),
  `Sentinel`, `SupplyQueue`, `Allocators`, `AllowedAdapters`, `Fees`,
  `WithdrawalCooldown`, `IdleResyncCooldown`, `Restrictions`,
  `TimelockConfig`, `Cap`, `MarketRemoval`, and `CapGroup`. Everything
  else (Admin transfer, Curator change, Governance change, Skim,
  Upgrade, Migrate, Other approvals) is Admin-only to revoke. This is
  the single most important operational lever between proposing and
  executing a sensitive economic / operational change.

---

## 1. Triage

When an alert fires that implicates your vault:

1. **Identify which role you are** (Curator / Allocator / Sentinel /
   Governance Admin) and which roles are reachable.
2. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   The Curator (or Governance Admin) is *lead* for any incident classified
   as curator / allocator / sentinel compromise. The Sentinel is *lead*
   for "we need to pause now" mechanics, even if the Curator is the
   policy decision-maker.
3. **Snapshot state** for every affected vault:
   - Current `paused` state, restrictions, share price, idle assets,
     allocated principal per market.
   - Pending governance proposals (the queue in
     `SorobanVaultGovernanceContract`).
   - Caps per market and per cap-group.
   - Fees and skim recipient.
   - Roles assignment (curator address, sentinel address, allocator
     addresses, governance admin, guardian).
4. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
5. **Pick the smallest viable mitigation** before escalating to
   curator-level changes.

---

## 2. Protocol hack

A protocol hack here is any state where:

- A Blend pool the vault supplies into is exploited (see
  [`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)
  section 2), or
- The vault contract / governance / share-token / Blend adapter itself is
  exploited, or
- A market the vault is allocated to is producing impossible accounting
  (e.g. share price discontinuity, allocator-side accounting drift).

### 2.1 Detection (Hypernative classes)

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Vault share price discontinuity beyond rounding; impossible `total_assets` vs. allocator state; unexplained transfer out of vault; Blend pool the vault supplies into is admin-frozen by the pool admin; reentrancy alert on the vault. |
| **High (P1)** | Allocator beginning/finishing operations on a schedule the vault does not normally use; `RefreshFees` or `SettlePayout` from an unexpected caller; sustained share price drift not explained by accrued fees. |
| **Medium (P2)** | Single market the vault is allocated to shows oracle anomaly; pending governance proposal that increases risk has been queued. |

### 2.2 Containment ladder

The ladder is ordered from least to most invasive. Climb only as far as
the evidence supports.

| Step | Role | Action | Effect | Reversible? |
|------|------|--------|--------|-------------|
| **A** | Allocator (or AllocatorEmergency) | `AbortAllocating` / `AbortWithdrawing` / `AbortRefreshing` on any in-progress operation | Cancels a stuck operation so that subsequent steps can run. | Yes |
| **B** | Allocator | `RebalanceWithdraw` from the suspect market(s) into idle / safer markets | Reduces exposure without changing policy. | Yes |
| **C** | Sentinel | `SetRestrictions` (allowlist mode) to freeze new deposits / withdrawals at the vault edge | Stops new exposure entering the vault while existing depositors can still exit (or vice versa, depending on restriction mode). | Yes — sentinel can lift |
| **D** | Sentinel | `set_paused(sentinel_address, true)` — direct entrypoint on Templar's [governance](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/governance/src/lib.rs) gated by `require_sentinel`. Executes immediately (no timelock; the `submit_set_paused(true)` timelock path is rejected with `InvalidInput`). Rejects `paused == false` — the Sentinel cannot use this entrypoint to unpause. | Pauses kernel actions allowed by `allowed_while_paused`; only `Pause`, `SetRestrictions`, the three `Abort*`s, `ManualReconcile`, and `EmergencyReset` continue to work. | Reversible only by Admin: `submit_set_paused(false)` (timelocked) then `accept(proposal_id)` after the pause timelock matures. |
| **E** | Sentinel (or Admin) | `revoke(proposal_id)` or `revoke_kind(kind)` on any pending governance proposal in the Sentinel's revokable set (Pause / pending unpause, Sentinel, SupplyQueue, Allocators, AllowedAdapters, Fees, WithdrawalCooldown, IdleResyncCooldown, Restrictions, TimelockConfig, Cap, MarketRemoval, CapGroup — see `can_revoke_kind`). Admin-only kinds (Admin transfer, Curator, Governance, Skim, Upgrade, Migrate, Other) must be revoked by the Admin. | Prevents an in-flight proposal from maturing during the incident. | Trivially: the proposal can be re-submitted later by the appropriate role. |
| **F** | Curator | `submit_set_cap(market_id, 0)` for the affected market | Drives the supply cap to zero, forcing future rebalances away from that market. | Yes — propose a non-zero cap later |
| **G** | Curator | `submit_remove_market(market_id)` | Forces the market out of the vault entirely. Subject to the configured timelock so depositors have notice. | Slow to reverse — re-adding a market is a fresh `submit_*` action with timelock. |
| **H** | Curator | `EmergencyReset` (`PolicyAdmin`-class action) | Force-idle a stuck vault. Only when steps A–G are not sufficient. | Possible, but use only with dev-team review of the kernel state. |

In a P0 protocol-hack incident, the Sentinel should call step D
(`set_paused(sentinel, true)`, immediate) within the first 10 minutes,
and the Sentinel (or Admin) should sweep step E for any pending
proposals in the Sentinel's revokable set that would increase risk
during the incident (including any pending unpause). Deeper steps
require curator action and may have timelocks. If the Sentinel is
unreachable or compromised, the vault cannot be paused on the
immediate path, so containment falls back to allocator-side actions
(steps A–B) plus Admin-side revocation of Admin-only kinds (step E)
until the Sentinel is replaced via `submit_set_sentinel` (timelocked).

### 2.3 Coordination during a Blend pool exploit

If the underlying Blend pool is what is exploited (not the vault itself):

1. The pool admin will (per
   [`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 2)
   move the pool to status 4 (admin frozen). Status 4 still permits
   withdrawals (the Blend adapter's deallocation path submits
   `REQUEST_WITHDRAW` = action_type 1, which is not gated by status >
   3 — see `pool/src/pool/pool.rs:77-80`), so your `RebalanceWithdraw`
   against the affected reserve should still work in principle. Treat
   it as blocked only if reserve-level liquidity, oracle staleness /
   safety, the vault's own pause / restrictions, or adapter-specific
   errors make the withdrawal unsafe or unprofitable.
2. If you have **multiple Blend pools** in the vault, deallocate from
   sibling pools first to leave the affected pool's residual share as
   small as possible.
3. Use the sentinel's `SetRestrictions` to freeze the vault edge so new
   depositors do not unknowingly inherit the affected pool exposure.
4. Once the pool admin / dev team announce a migration pool, queue a
   `submit_remove_market` for the old pool reserve and a `submit_*`
   to add the new pool reserve. Both proposals are subject to your
   configured timelocks.

### 2.4 Recovery

1. Wait for the dev team / pool admin to confirm root cause and
   recovery path.
2. Step back through the containment ladder in reverse: lift `Pause`,
   then lift `SetRestrictions`, then resume normal allocation.
3. Stand-down requires **two** Safe Chain sign-offs: the Curator and one
   of (Blend dev team, Stellar Foundation). The Sentinel cannot sign
   itself off — it took the action; another role corroborates that the
   reason is resolved.
4. Public post-mortem within 14 days, jointly with the affected pool admin
   if applicable.

---

## 3. Bad debt

Following Morpho's [bad debt
guidance](https://docs.morpho.org/curate/tutorials-v2/bad-debt/), bad debt
is contained to the affected market in Blend. The vault's job is to make
sure the loss is **recognised** in share price (so that depositors who
exit afterwards do not exit at a stale price) and to **stop allocating**
to the bad-debt-producing market.

### 3.1 Detection

| Class | Signal |
|-------|--------|
| **High (P1)** | Backstop on the affected pool has `q4w_pct` approaching a transition threshold for the pool's current status (see the conditional table in [`../blend/02-blend-pool-admins.md`](./02-blend-pool-admins.md) §0 — 60% only forces a freeze from the backstop-driven branch; a pool in admin-active (0) or admin-on-ice (2) has different thresholds). Bad-debt auctions are stalling. The vault has material allocation to the affected reserve. |
| **Medium (P2)** | A user the vault has visibility into has negative health that is not being liquidated. `q4w_pct` between 30% and 60%. |

### 3.2 Containment

1. **Allocator: `RebalanceWithdraw`** from the affected reserve into idle
   / safer reserves. If withdrawal liquidity is constrained at the pool
   level, do partial withdraws repeatedly.
2. **Curator: `submit_set_cap(affected_market, 0)`** so subsequent
   allocations cannot send funds back into the reserve.
3. **Sentinel: `SetRestrictions`** if depositor activity needs to be
   constrained while the loss is being recognised. Especially important
   if the vault uses an idle-only fast-path withdrawal — fast-path users
   will exit at a higher share price than allocated users until the loss
   propagates.
4. **Force loss recognition.** Anyone (not just the curator) can call
   `bad_debt(user)` on the Blend pool to push the user's residual
   liabilities to the backstop. This is what triggers the pool's
   accounting to reflect the loss. The vault's `SyncExternalAssets`
   (allocator) then propagates the change to the vault's `total_assets`.
5. **Decide whether to socialize or compensate.** If the loss exceeds the
   curator's risk budget for that market and you have a treasury or
   insurance source, this is the moment to decide whether to top up.
   Either way, communicate the decision before users exit at the
   updated share price.

### 3.3 Recovery

1. Once `total_assets` is updated and the share price reflects the loss,
   curators can `submit_remove_market(affected_market)` to remove the
   reserve from the vault permanently, or leave the cap at 0 and
   re-evaluate later.
2. Update the curator's published policy / strategy document with the
   parameter change.
3. Vault-side stand-down requires Curator sign-off (the vault took the
   action). Ecosystem-level stand-down for the bad-debt incident
   follows [`03-stellar-foundation.md`](./03-stellar-foundation.md)
   §8: Pool admin lead + Blend dev team.

---

## 4. Faulty oracle

Vaults inherit the oracle of every Blend pool they supply into. A faulty
oracle on a single reserve can move the share price the wrong way and
let arbitrageurs extract value through deposit / atomic-withdraw cycles.

### 4.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Price deviation > N% on a feed used by a market the vault is allocated to, with active deposit / withdraw flow. |
| **High (P1)** | Single-feed staleness > 2× the configured `price_maximum_age_s`; oracle provider acknowledges incident. |
| **Medium (P2)** | Confidence band collapse on a single feed; one provider in a multi-provider feed disconnects. |

### 4.2 Containment

1. **Sentinel: `set_paused(sentinel, true)` (direct, immediate)** immediately if the oracle is *manipulated* (not
   just stale). A stale oracle will simply make Blend operations panic;
   a manipulated oracle will let depositors exit at the wrong share
   price. Pausing the vault edge is the right move.
2. **Allocator: `AbortRefreshing` / `AbortAllocating`** on any in-flight
   operations that depend on the affected feed.
3. **Allocator: `RebalanceWithdraw`** from any reserve that depends on
   the affected feed, while the feed is still publishable (a fully
   stale feed will block your rebalance).
4. **Curator: `submit_set_cap(market, 0)`** for any market that depends
   on the affected feed.
5. **Coordinate** with Blend pool admins
   ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 4) and
   Stellar Foundation
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 4)
   so that the pool-side response is consistent.

### 4.3 Recovery

Step back through the containment ladder in reverse only after the
oracle provider has published all-clear and Blend pool admins have
restored their pool's status. Stand-down requires Curator + Blend dev
team sign-off (Safe Chain rule).

---

## 5. Curator compromise

Your Curator key is the most powerful key in the vault, but its actions
are timelocked and subject to Sentinel revocation. That asymmetry is the
defence.

### 5.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | A `submit_*` proposal you did not authorise that increases risk: cap increases, fee increases, `submit_set_curator` to an unknown address, `submit_set_governance` to a non-multisig contract, `submit_set_sentinel` to an attacker-controlled address. |
| **High (P1)** | A `submit_*` proposal that is technically allowed but uncharacteristic in its parameters or timing. |
| **Medium (P2)** | Curator key rotation request in an unusual channel; signing infrastructure shows degraded health. |

### 5.2 Containment

1. **Sentinel (or Admin): revoke every pending proposal in scope from
   the compromised path.** The Sentinel can revoke the kinds listed in
   `can_revoke_kind` (Pause, Sentinel, SupplyQueue, Allocators,
   AllowedAdapters, Fees, WithdrawalCooldown, IdleResyncCooldown,
   Restrictions, TimelockConfig, Cap, MarketRemoval, CapGroup). If the
   compromised proposal is a Curator, Governance, Admin transfer,
   Skim, Upgrade, Migrate, or Other approval, only the Admin can
   revoke; escalate immediately to the Admin (and to NEAR / Stellar
   Foundation Safe Chain if the Admin is not reachable).
2. **Sentinel: `set_paused(sentinel_address, true)`** — direct
   entrypoint, executes immediately — to halt vault activity while
   the curator is being recovered.
3. **Governance Admin: `submit_set_curator(<known clean address>)`**.
   Subject to the curator-change timelock. During this window, the
   Sentinel (or Admin) must continue to revoke any further malicious
   proposals in scope; Curator-kind proposals themselves are
   Admin-only to revoke.
4. If the captured key is also the Governance Admin (it should *not* be
   — see standing posture), the situation is much worse. The
   Governance Admin can change the timelock configuration via
   `submit_set_timelock`, and a captured Admin who shortened the
   timelock before being detected may be able to push proposals
   through faster than you can react. Coordinate with Stellar
   Foundation Safe Chain
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 6)
   to alert depositors and prepare a migration to a fresh vault.

### 5.3 Recovery

1. Replace signing infrastructure end-to-end.
2. Re-publish standing-posture audit (which keys are in which roles,
   under what hardware, with what rotation cadence).
3. Stand-down requires the new Curator + the Sentinel + Foundation
   sign-off (Safe Chain rule).

---

## 6. Allocator compromise

The Allocator is the hottest key but its blast radius is bounded by the
Curator's policy: caps, supply queue, withdraw route. A captured
Allocator can:

- Allocate to caps' edge in coordinated markets (potentially in concert
  with an external exploit).
- Withdraw idle into adversary-controlled paths if the withdraw route
  is configurable to addresses (it is not — withdrawals go to the
  vault user; check your specific implementation).
- Spam `BeginAllocating` / `AbortAllocating` to disrupt operations.

### 6.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Allocator action that violates expected operational schedule and produces share price drift; `BeginAllocating` to a market with rapidly worsening conditions. |
| **High (P1)** | Allocator activity from an unusual source IP / signing setup; off-cycle `RefreshFees` or `SettlePayout`. |
| **Medium (P2)** | Allocator-key signing infrastructure shows degraded health. |

### 6.2 Containment

1. **AllocatorEmergency: `AbortAllocating` / `AbortWithdrawing` /
   `AbortRefreshing`** on any in-progress operation initiated by the
   captured Allocator.
2. **Sentinel: `set_paused(sentinel, true)` (direct, immediate)** to stop the captured Allocator from initiating
   new operations. While paused, only `Pause`, `SetRestrictions`,
   `Abort*`, `ManualReconcile`, `EmergencyReset` are callable — none of
   which is in the Allocator policy class.
3. **Curator: rotate the Allocator** (the standard mechanism — usually
   `submit_set_*` for the role assignment). Subject to your configured
   timelock; if the timelock is non-trivial, the sentinel must keep
   the vault paused for the duration.
4. If the captured Allocator was also the AllocatorEmergency (often the
   same key), the abort path is itself unsafe. Pause the vault and
   wait for the Allocator rotation to complete.

### 6.3 Recovery

1. Replace signing infrastructure for the Allocator key.
2. Stand-down requires Curator + Sentinel sign-off (Safe Chain rule).
3. Lift `Pause` only after the new Allocator has demonstrated a clean
   refresh cycle on the vault.

---

## 7. Sentinel compromise

A captured Sentinel can disrupt the vault by spamming pause / unpause and
restriction changes, and (importantly) can revoke legitimate Curator
proposals. It cannot extract value, but it can break the vault's
operational rhythm and undermine the Curator's ability to ship policy.

### 7.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Sentinel pauses / unpauses with no incident context; sentinel revokes a curator proposal that the curator publicly supports. |
| **High (P1)** | Sentinel `SetRestrictions` to an attacker-favourable mode (e.g. denylist that excludes only legitimate liquidators). |
| **Medium (P2)** | Sentinel-key signing infrastructure shows degraded health. |

### 7.2 Containment

1. **Curator: `submit_set_sentinel(<known clean address>)`** — subject
   to the sentinel-change timelock. While the timelock is maturing, the
   captured sentinel can keep revoking the very proposal that would
   replace them. Coordinate with Stellar Foundation Safe Chain
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 6)
   to publicly mark the sentinel as captured so that depositors are
   warned, and prepare for a possible migration to a fresh vault if
   the captured sentinel cannot be removed in reasonable time.
2. **Governance Admin** may be able to act faster than the Curator
   timelock if the governance contract permits; check your
   implementation. In Templar's
   [governance](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/soroban/governance/src/lib.rs),
   the Admin holds the timelock-config keys via `submit_set_timelock`,
   but cannot bypass the sentinel-change timelock without abdication
   /reconfiguration.
3. **Allocator** continues to operate within Curator policy. A pause
   set by a captured Sentinel is *not* something the Sentinel itself
   can undo — the direct `set_paused` entrypoint rejects
   `paused == false`. Unpause is only via `submit_set_paused(false)`
   (Admin, timelocked) followed by `accept(proposal_id)`. If the
   captured Sentinel has paused the vault, the vault stays paused
   until the Admin schedules and executes the unpause proposal, or
   until the Sentinel is replaced via `submit_set_sentinel` and the
   new Sentinel remains inactive.

### 7.3 Recovery

1. Replace signing infrastructure for the Sentinel key.
2. Stand-down requires Curator + Foundation sign-off (Safe Chain rule).

---

## 8. Pool admin compromise

Your vault is a *user* of a Blend pool. If the pool admin is compromised
([`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 5), the
captured admin can reconfigure reserves to drain liquidity, change
emissions, or hand the admin role to another address.

### 8.1 Containment

1. **Sentinel: `set_paused(sentinel, true)` (direct, immediate)** on every vault that supplies into the affected
   pool, immediately.
2. **Sentinel: `SetRestrictions`** to freeze the vault edge so new
   deposits cannot enter and inherit the exposure.
3. **Allocator: `RebalanceWithdraw`** as much as possible from the
   affected pool's reserves while the pool is still operational. If
   the captured admin has already moved the pool to status 4
   (admin-frozen), withdrawals are still allowed; if they have
   adversarially configured a reserve such that withdraw is unsafe
   (e.g. price feed misconfiguration), do not withdraw — coordinate
   with Foundation Safe Chain instead.
4. **Curator: `submit_set_cap(affected_market, 0)`** and
   `submit_remove_market(affected_market)` to remove the pool from
   the vault's strategy.
5. **Coordinate** with Foundation Safe Chain
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 5)
   for cross-stack defences (stablecoin freezes, bridge halts).

### 8.2 Recovery

Per the dev team's recovery plan
([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)
section 2.3 / 5). Migrate to the new pool when it is live and audited.

---

## 9. Communication protocol

Depositors are users of the vault, not of the Blend pool. They look to
*you* for clarity. Communication standards:

- **Status page or pinned thread** maintained per vault, updated within
  15 minutes of any sentinel action.
- **No attribution** until forensics is complete.
- **Coordinate with Blend pool admins and Foundation** before publishing
  cross-protocol claims.
- **Per-incident statements** at the same three checkpoints used by the
  Foundation (awareness, containment, stand-down).
- **Post-mortem within 14 days** for any P0/P1, including the
  containment-ladder steps you actually used and how long each took.

---

## 10. Stand-down checklist

- [ ] Vault `paused` is `false` only after Safe Chain sign-off.
- [ ] All `SetRestrictions` returned to baseline.
- [ ] No pending governance proposals that were submitted under
      adversary control are still in queue.
- [ ] Caps and fees match the curator's published policy.
- [ ] Roles assignment matches the curator's published policy.
- [ ] All affected Blend pool admins informed of stand-down.
- [ ] Stellar Foundation contact informed of stand-down.
- [ ] Public post-mortem drafted.
- [ ] War room archived (append-only log preserved).
- [ ] Cold-start drill scheduled within 30 days to re-validate the
      containment ladder against the post-incident topology.
