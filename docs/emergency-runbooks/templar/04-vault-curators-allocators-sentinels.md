# Runbook: Vault Curators, Allocators, Sentinels and Associated Parties

**Audience.** Operators of Templar curator vaults on NEAR. These vaults
run on the shared vault kernel with NEAR-specific runtime code in
[`contract/vault/near`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/near) and reuse the
chain-agnostic policy and role primitives in
[`contract/vault/curator-primitives`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/vault/curator-primitives).
The role model is the same as the Soroban runtime (Curator, Sentinel,
Allocator, AllocatorEmergency) with the same governance timelock
semantics.

Cross-chain note: a curator that operates *both* a NEAR vault (this
runbook) and a Soroban vault (`../blend/04-vault-curators-allocators-sentinels.md`
in the sibling collection) should read both runbooks; the on-chain
entrypoints and adapter details differ per chain even though the role
model is shared.

This runbook is inspired by and adapts ideas from Morpho's vault
emergency documentation ([V1](https://docs.morpho.org/curate/tutorials-v1/emergency/),
[V2](https://docs.morpho.org/curate/tutorials-v2/emergency/),
[bad debt](https://docs.morpho.org/curate/tutorials-v2/bad-debt/),
[security considerations](https://docs.morpho.org/curate/concepts/security-considerations/))
to Templar's NEAR architecture and the immutable-market model.

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

### 0.1 Role separation

The vault stack uses three (sometimes four) named roles, each with
its own key and its own threat model. From
[`curator-primitives/src/auth/mod.rs`](https://github.com/Templar-Protocol/contracts/blob/dev/contract/vault/curator-primitives/src/auth/mod.rs):

| Role | Auth policy class | Actions it can take | When to use |
|------|-------------------|---------------------|-------------|
| **Curator** | `Curator` | `ManualReconcile`, `EmergencyReset`, `PolicyAdmin` (also the proposer for almost all governance actions when acting as governance Admin). | Slowest, most powerful. Proposes timelocked policy changes (caps, fees, supply queue, market addition / removal, role changes). |
| **Allocator** | `Allocator` | `ExecuteWithdraw`, `BeginAllocating` / `FinishAllocating`, `SyncExternalAssets`, `RebalanceWithdraw`, `BeginRefreshing` / `FinishRefreshing`, `SettlePayout`, `RefreshFees`. | Day-to-day rebalancing within bounds set by curator. Hot key. |
| **AllocatorEmergency** | `AllocatorEmergency` | `AbortAllocating`, `AbortWithdrawing`, `AbortRefreshing`. | Cancel a stuck or in-progress operation. Often the same key as Allocator, but conceptually separable. |
| **Sentinel** | `Sentinel` | `Pause`, `SetRestrictions` (executed by the governance Admin's submission of these actions; the Sentinel's on-chain lever is revocation). | Reactive risk reduction. Should be a hotter key than the curator and *separate* from the allocator. |
| **Public** (depositors) | `Public` | `Deposit`, `RequestWithdraw`, `AtomicWithdraw`, `AtomicRedeem`. | n/a |

The Sentinel exists specifically so that risk reduction does not
require the slow Curator timelock. Configure it with that in mind.
**The Sentinel should never share a key with the Curator or the
Allocator.**

The vault governance contract additionally has:

| Governance role | Actions |
|-----------------|---------|
| **Admin / Owner** of governance contract | Acts as the canonical proposer / canonical timelock controller. Holds emergency abdication. All `submit_*` methods on Templar governance (both NEAR and Soroban runtimes share the primitives) go through `require_admin`. |
| **Guardian** | Configurable; delegated blocking power. Satisfies `require_revoker` for revocation. |
| **Sentinel** | See above. Satisfies `require_revoker` for revocation. |
| **Skim recipient** | Receives skim drains. |

### 0.2 Operational hygiene

Before any incident:

- **Hardware-back every privileged role** (Curator, Sentinel,
  AllocatorEmergency, Governance Admin). Allocator can be a hot key
  but should be protected by per-call value limits at the off-chain
  signing layer. On NEAR, this typically means an HSM-backed
  full-access key or a function-call access key with strict allowance.
- **Separate rotation cadence** per role.
- **Per-vault dashboards** showing: idle balance, allocated principal
  per market, share price, current cap per market, current fees,
  pending governance proposals, last refresh time, withdrawal queue
  length.
- **Hypernative or equivalent** monitoring on at least: governance
  `submit_*` actions, `set_paused`, `set_restrictions`, allocator
  rebalance actions, share-price discontinuities, allocator
  beginning / finishing operations that do not match the vault's
  refresh schedule, unexpected NEAR access-key changes on any of the
  role accounts.
- **A cold-start drill** — at least once per quarter, run through
  the containment ladder in section 2.2 on testnet end-to-end and
  measure time-to-pause. Target: under 10 minutes from Sentinel
  detection to Admin submitting `submit_set_paused(true)` and
  observing the immediate pause.
- **Pre-shared incident contacts** with: Templar dev team
  ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md)),
  the registry admin
  ([`05-templar-registry-admin.md`](./05-templar-registry-admin.md)),
  NEAR Foundation
  ([`03-near-foundation.md`](./03-near-foundation.md)), and the NEAR
  Intents team
  ([`02-near-intents-team.md`](./02-near-intents-team.md)) if the
  vault is intent-routable.

### 0.3 Timelock posture

The vault governance contract uses configurable per-action timelocks
(`submit_set_paused`, `submit_set_curator`, `submit_set_governance`,
`submit_set_supply_queue`, `submit_set_fees`, `submit_set_restrictions`,
`submit_set_guardian`, `submit_set_sentinel`, `submit_set_cap`,
`submit_remove_market`, `submit_set_group_cap`,
`submit_set_group_rel_cap`, `submit_set_group_member`,
`submit_set_skim_recipient`, `submit_skim`, `submit_set_timelock`).

Following Morpho's [security
considerations](https://docs.morpho.org/curate/concepts/security-considerations/):

- **Risk-reducing actions should be immediate or near-immediate.**
  `SetPaused(true)` is decided as `TimelockDecision::Immediate` in
  the governance contract, so a pause submitted by the Admin takes
  effect immediately (no queue). Cap decreases are similarly
  immediate.
- **Risk-increasing actions must have non-trivial timelocks** (Morpho
  recommends up to 3 weeks; choose per your governance posture).
  Cap increases, fee increases, adding markets, raising max growth
  rate, changing the curator, `SetPaused(false)` (unpause).
- **The Sentinel / Guardian can revoke a pending governance
  proposal.** That is the design intent of the role and is the
  single most important operational lever between proposing and
  executing a sensitive change. This is the Sentinel's *only*
  direct on-chain lever — the Sentinel cannot submit `Pause`
  itself; that requires the Admin.

---

## 1. Triage

When an alert fires that implicates your vault:

1. **Identify which role you are** (Curator / Allocator / Sentinel /
   Governance Admin) and which roles are reachable.
2. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   The Curator (or Governance Admin) is *lead* for any incident
   classified as curator / allocator / sentinel compromise. For
   "we need to pause now" mechanics, the Governance Admin is the
   only role that can submit a pause; the Sentinel is *lead* for
   revoking any pending malicious proposals.
3. **Snapshot state** for every affected vault:
   - Current `paused` state, restrictions, share price, idle
     assets, allocated principal per market.
   - Pending governance proposals (the queue on the governance
     contract).
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

- A Templar market the vault supplies into is exploited (see
  [`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §2), or
- The vault contract / governance / share-token itself is exploited,
  or
- A market the vault is allocated to is producing impossible
  accounting (e.g. share price discontinuity, allocator-side
  accounting drift).

### 2.1 Detection (Hypernative classes)

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Vault share price discontinuity beyond rounding; impossible `total_assets` vs. allocator state; unexplained outbound transfer from vault; Templar dev team announces a market exploit affecting a market you supply into; reentrancy alert on the vault. |
| **High (P1)** | Allocator beginning/finishing operations on a schedule the vault does not normally use; `RefreshFees` or `SettlePayout` from an unexpected caller; sustained share price drift not explained by accrued fees. |
| **Medium (P2)** | Single market the vault is allocated to shows oracle anomaly; pending governance proposal that increases risk has been queued. |

### 2.2 Containment ladder

The ladder is ordered from least to most invasive. Climb only as
far as the evidence supports.

| Step | Role | Action | Effect | Reversible? |
|------|------|--------|--------|-------------|
| **A** | Allocator (or AllocatorEmergency) | `AbortAllocating` / `AbortWithdrawing` / `AbortRefreshing` on any in-progress operation | Cancels a stuck operation so that subsequent steps can run. | Yes |
| **B** | Allocator | `RebalanceWithdraw` from the suspect market(s) into idle / safer markets | Reduces exposure without changing policy. | Yes |
| **C** | Governance Admin | `submit_set_restrictions` (allowlist / denylist mode) to freeze new deposits / withdrawals at the vault edge | Stops new exposure entering the vault while existing depositors can still exit (or vice versa, depending on restriction mode). | Yes — Admin can propose a return to `RestrictionMode::None`; usually timelocked. |
| **D** | Governance Admin / Owner | `submit_set_paused(true)` — all `submit_*` methods on Templar governance go through `require_admin`, so only the Admin / Owner can submit a pause. `SetPaused(true)` is decided as `TimelockDecision::Immediate`, so the pause takes effect immediately upon submission. | Pauses kernel actions allowed by `allowed_while_paused`; only `Pause`, `SetRestrictions`, the three `Abort*`s, `ManualReconcile`, and `EmergencyReset` continue to work. | Yes — Admin submits `submit_set_paused(false)` (timelocked) once root cause is resolved. |
| **E** | Sentinel / Guardian (or Admin) | `revoke(proposal_id)` or `revoke_kind(kind)` to drop pending governance proposals that would increase risk | The `require_revoker` check accepts the Admin, Guardian, or Sentinel, so this is the Sentinel's primary on-chain emergency lever. Prevents an in-flight curator / admin proposal (cap increase, fee increase, market addition, unpause) from maturing during the incident. | Trivially: the proposal can be re-submitted later by the Admin. |
| **F** | Curator (as Governance Admin) | `submit_set_cap(market_id, 0)` for the affected market | Drives the supply cap to zero, forcing future rebalances away from that market. | Yes — propose a non-zero cap later |
| **G** | Curator (as Governance Admin) | `submit_remove_market(market_id)` | Forces the market out of the vault entirely. Subject to the configured timelock so depositors have notice. | Slow to reverse — re-adding a market is a fresh `submit_*` action with timelock. |
| **H** | Curator | `EmergencyReset` (`PolicyAdmin`-class action) | Force-idle a stuck vault. Only when steps A–G are not sufficient. | Possible, but use only with dev-team review of the kernel state. |

In a P0 protocol-hack incident, the Governance Admin should submit
step D (`submit_set_paused(true)`, immediate) within the first 10
minutes, and the Sentinel / Guardian should sweep step E (revoke any
pending proposals that would increase risk during the incident,
including any pending unpause). Deeper steps require curator action
and may have timelocks. If the Admin is unreachable or compromised,
the Sentinel / Guardian cannot pause the vault directly, so
containment falls back to allocator-side actions (steps A–B) plus
revocation (step E) until the Admin is replaced via
`submit_set_governance` (timelocked).

### 2.3 Coordination during a Templar market exploit

If the underlying Templar market is what is exploited (not the vault
itself):

1. The Templar dev team will (per
   [`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §2)
   halt operator bots and publish user guidance, but **cannot pause
   the market** — Templar markets are immutable. Your vault edge is
   the pause surface for your depositors.
2. If you have **multiple Templar markets** in the vault, deallocate
   from sibling markets first to leave the affected market's
   residual share as small as possible; your `RebalanceWithdraw`
   against the affected market should still work (Templar markets
   have no admin pause), but may face liquidity constraints if
   utilisation spiked.
3. Use restrictions (step C) and pause (step D) to freeze the vault
   edge so new depositors do not unknowingly inherit exposure.
4. Once the Templar dev team announces a migration market, queue
   `submit_remove_market` for the old market and a `submit_*` to
   add the new market. Both proposals are subject to configured
   timelocks.

### 2.4 Recovery

1. Wait for the Templar dev team / registry admin to confirm root
   cause and recovery path.
2. Step back through the containment ladder in reverse: Admin
   `submit_set_paused(false)` (timelocked), then adjust
   restrictions, then resume normal allocation.
3. Stand-down requires **two** NEAR Safe Chain sign-offs: the
   Curator and one of (Templar dev team, NEAR Foundation). The
   Sentinel cannot sign itself off — it acted; another role
   corroborates that the reason is resolved.
4. Public post-mortem within 14 days, jointly with the Templar dev
   team if applicable.

---

## 3. Bad debt

Templar markets have no per-pool backstop; bad debt propagates
directly to suppliers in the affected market. The vault's job is to
make sure the loss is **recognised** in share price (so depositors
who exit afterwards do not exit at a stale price) and to **stop
allocating** to the market that produced the loss.

### 3.1 Detection

| Class | Signal |
|-------|--------|
| **High (P1)** | Multiple bad-debt candidates identified on a market you allocate to; oracle staleness preventing liquidation; the vault has material allocation to the affected market. |
| **Medium (P2)** | A user the vault has visibility into has negative health that is not being liquidated. |

### 3.2 Containment

1. **Allocator: `RebalanceWithdraw`** from the affected market into
   idle / safer markets. If withdrawal liquidity is constrained at
   the market level, do partial withdraws repeatedly.
2. **Curator (as Governance Admin): `submit_set_cap(affected_market, 0)`**
   so subsequent allocations cannot send funds back into the
   market.
3. **Governance Admin: `submit_set_restrictions`** if depositor
   activity needs to be constrained while the loss is being
   recognised. Especially important if the vault uses an
   idle-only fast-path withdrawal — fast-path users will exit at a
   higher share price than allocated users until the loss
   propagates.
4. **`SyncExternalAssets`** (allocator) to propagate the change to
   the vault's `total_assets`.
5. **Decide whether to socialize or compensate.** If the loss
   exceeds the curator's risk budget for the market and you have a
   treasury or insurance source, this is the moment to decide
   whether to top up. Either way, communicate the decision before
   users exit at the updated share price.

### 3.3 Recovery

1. Once `total_assets` is updated and the share price reflects the
   loss, curators can `submit_remove_market(affected_market)` to
   remove the market from the vault permanently, or leave the cap
   at 0 and re-evaluate.
2. Update the curator's published policy / strategy document.
3. Stand-down requires Curator + Templar dev team sign-off (NEAR
   Safe Chain rule).

---

## 4. Faulty oracle

Vaults inherit the oracle of every Templar market they supply into.
A faulty oracle on a market moves that market's borrow / liquidation
behavior and can push share price the wrong way.

### 4.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Price deviation > N% on a feed used by a market the vault is allocated to, with active deposit / withdraw flow. |
| **High (P1)** | Single-feed staleness > 2× the configured `price_maximum_age_s`; oracle provider acknowledges incident. |
| **Medium (P2)** | Confidence band collapse on a Pyth feed; LST oracle adapter derivation drift; proxy oracle source-selection change. |

### 4.2 Containment

1. **Governance Admin: `submit_set_paused(true)`** immediately if
   the oracle is *manipulated* (not just stale). A stale Pyth
   price will simply make market operations panic; a manipulated
   price will let depositors exit at the wrong share price.
   Pausing the vault edge is the right move.
2. **Allocator: `AbortRefreshing` / `AbortAllocating`** on any
   in-flight operations that depend on the affected feed.
3. **Allocator: `RebalanceWithdraw`** from any market that depends
   on the affected feed, while the feed is still publishable (a
   fully stale feed will block your rebalance).
4. **Curator: `submit_set_cap(market, 0)`** for any market that
   depends on the affected feed.
5. **Coordinate** with Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §4)
   and NEAR Foundation
   ([`03-near-foundation.md`](./03-near-foundation.md) §4).

### 4.3 Recovery

Step back through the containment ladder in reverse only after the
oracle provider has published all-clear. Stand-down requires
Curator + Templar dev team sign-off (NEAR Safe Chain rule).

---

## 5. Curator compromise

Your Curator key is the most powerful key in the vault, but its
actions are timelocked and subject to Sentinel / Guardian revocation.
That asymmetry is the defence.

### 5.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | A `submit_*` proposal you did not authorise that increases risk: cap increases, fee increases, `submit_set_curator` to an unknown address, `submit_set_governance` to a non-multisig contract, `submit_set_sentinel` to an attacker-controlled address. Or an unexpected NEAR access-key addition on the curator's account. |
| **High (P1)** | A `submit_*` proposal that is technically allowed but uncharacteristic in its parameters or timing. |
| **Medium (P2)** | Curator key rotation request in an unusual channel; signing infrastructure shows degraded health. |

### 5.2 Containment

1. **Sentinel / Guardian: revoke every pending proposal from the
   curator.** This is the single most important action.
2. **Governance Admin (if not the same as compromised curator):
   `submit_set_paused(true)`** to halt vault activity while the
   curator is being recovered.
3. **Governance Admin: `submit_set_curator(<known clean address>)`**.
   Subject to the curator-change timelock. During this window, the
   sentinel must continue to revoke any further malicious proposals.
4. If the captured key is also the Governance Admin (it should
   *not* be — see standing posture), coordinate with NEAR
   Foundation Safe Chain
   ([`03-near-foundation.md`](./03-near-foundation.md) §6) to
   alert depositors and prepare a migration to a fresh vault.

### 5.3 Recovery

1. Replace signing infrastructure end-to-end.
2. Re-publish standing-posture audit.
3. Stand-down requires the new Curator + the Sentinel + Foundation
   sign-off (NEAR Safe Chain rule).

---

## 6. Allocator compromise

Bounded by the Curator's policy: caps, supply queue, withdraw route.
A captured Allocator can allocate to caps' edge in coordinated
markets, mass-withdraw during stress, or spam operations to disrupt.

### 6.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Allocator action that violates expected operational schedule and produces share-price drift; `BeginAllocating` to a market with rapidly worsening conditions. |
| **High (P1)** | Allocator activity from an unusual source IP / signing setup; off-cycle `RefreshFees` or `SettlePayout`. |
| **Medium (P2)** | Allocator-key signing infrastructure shows degraded health. |

### 6.2 Containment

1. **AllocatorEmergency: `AbortAllocating` / `AbortWithdrawing` /
   `AbortRefreshing`** on any in-progress operation initiated by
   the captured Allocator.
2. **Governance Admin: `submit_set_paused(true)` (immediate)** to
   stop the captured Allocator from initiating new operations.
   While paused, only `Pause`, `SetRestrictions`, `Abort*`,
   `ManualReconcile`, `EmergencyReset` are callable — none of
   which is in the Allocator policy class.
3. **Curator: rotate the Allocator** via the standard governance
   proposal path. Subject to timelock; sentinel must keep the vault
   paused during the timelock.
4. If the captured Allocator was also the AllocatorEmergency (often
   the same key), the abort path is itself unsafe. Pause the vault
   and wait for the Allocator rotation to complete.

### 6.3 Recovery

1. Replace signing infrastructure for the Allocator key.
2. Stand-down requires Curator + Sentinel sign-off (NEAR Safe Chain
   rule).
3. Lift `Pause` only after the new Allocator has demonstrated a
   clean refresh cycle on the vault.

---

## 7. Sentinel compromise

A captured Sentinel can disrupt operational rhythm and revoke
legitimate Curator proposals. Cannot extract value directly but
can undermine the Curator's ability to ship policy.

### 7.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Sentinel revokes a proposal that the curator publicly supports; sentinel spam-revokes. |
| **High (P1)** | Sentinel-key signing infrastructure shows degraded health. |
| **Medium (P2)** | Sentinel key rotation request in an unusual channel. |

### 7.2 Containment

1. **Governance Admin: `submit_set_sentinel(<known clean address>)`**
   — subject to the sentinel-change timelock. During the timelock,
   the captured sentinel can keep revoking the very proposal that
   would replace them; coordinate with NEAR Foundation Safe Chain
   ([`03-near-foundation.md`](./03-near-foundation.md) §6) to
   publicly mark the sentinel as captured and prepare for a
   possible migration to a fresh vault if the captured sentinel
   cannot be removed in reasonable time.

### 7.3 Recovery

1. Replace signing infrastructure for the Sentinel key.
2. Stand-down requires Curator + Foundation sign-off (NEAR Safe
   Chain rule).

---

## 8. Registry admin / dev-team-side incident

Your vault is a *user* of Templar markets deployed via the registry.
If the registry admin is compromised
([`05-templar-registry-admin.md`](./05-templar-registry-admin.md) §5),
new market deployments and code versions cannot be trusted. If the
Templar dev team announces a market exploit, the market cannot be
paused on-chain (immutable), so vault-side action is the only
containment.

### 8.1 Containment

1. **Governance Admin: `submit_set_paused(true)` (immediate)** on
   every vault that supplies into any market whose provenance is
   in question, or into any specific market announced as exploited.
2. **Governance Admin: `submit_set_restrictions`** to freeze the
   vault edge so new deposits cannot enter and inherit the
   exposure.
3. **Allocator: `RebalanceWithdraw`** as much as possible from any
   affected market — remember Templar markets cannot be paused, so
   the withdraw path is always open unless liquidity is
   constrained.
4. **Curator: `submit_set_cap(affected_market, 0)`** and
   `submit_remove_market(affected_market)` to remove the market
   from the vault's strategy.
5. **Coordinate** with NEAR Foundation Safe Chain
   ([`03-near-foundation.md`](./03-near-foundation.md) §5) for
   cross-stack defences.

### 8.2 Recovery

Per the Templar dev team's recovery plan
([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §2.3 / §5).
Migrate to the new market when it is live and audited and its
version code hash is confirmed against Templar-published values.

---

## 9. Communication protocol

Depositors are users of the vault, not of the Templar market. They
look to *you* for clarity.

- **Status page or pinned thread** per vault, updated within 15
  minutes of any pause / restriction / revocation action.
- **No attribution** until forensics is complete.
- **Coordinate with Templar dev team and NEAR Foundation** before
  publishing cross-protocol claims.
- **Per-incident statements** at the same three checkpoints used by
  Foundation (awareness, containment, stand-down).
- **Post-mortem within 14 days** for any P0/P1, including the
  containment-ladder steps you actually used and how long each
  took.

---

## 10. Stand-down checklist

- [ ] Vault `paused` is `false` only after Safe Chain sign-off.
- [ ] All `SetRestrictions` returned to baseline.
- [ ] No pending governance proposals submitted under adversary
      control are still in queue.
- [ ] Caps and fees match the curator's published policy.
- [ ] Roles assignment matches the curator's published policy.
- [ ] Templar dev team informed of stand-down.
- [ ] NEAR Foundation contact informed of stand-down.
- [ ] Public post-mortem drafted.
- [ ] War room archived (append-only log preserved).
- [ ] Cold-start drill scheduled within 30 days to re-validate the
      containment ladder against the post-incident topology.
