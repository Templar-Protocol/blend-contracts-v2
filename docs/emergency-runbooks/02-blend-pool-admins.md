# Runbook: Blend Pool Admins

**Audience.** The keyholder(s) (EOA, multisig, or custom contract) currently
designated as `admin` for one or more Blend pools. You are the *only* party
that can call `set_status`, `update_pool`, `queue_set_reserve`,
`cancel_set_reserve`, `set_emissions_config`, and `propose_admin` on those
pools. Almost every emergency lever in this runbook is gated by your key.

**Scope.** You administer one specific pool (or a set of pools). Your
authority is local: you cannot change another admin's pool, and you cannot
upgrade contract code (Blend pools are immutable; see
[`pool/src/contract.rs`](../../pool/src/contract.rs)). You can *pause* the
pool and *queue* parameter changes.

Read [`README.md`](./README.md) first for the severity matrix, Hypernative
classification, war room template, and Safe Chain coordination model.

---

## 0. Standing posture

Before any incident, the pool admin must keep the following in a known-good
state:

- A documented, hardware-backed signing flow for every admin action. No
  single laptop should be able to broadcast a `set_status` or
  `propose_admin`. Multisigs are strongly recommended.
- A second, *independent* signing flow ready to broadcast `set_status(4)`
  (admin frozen). Status 4 is the strongest action and you must be able to
  call it within minutes, even if your primary signing setup is degraded.
- A list of the reserves on your pool, with each one's current
  `c_factor`, `l_factor`, `max_util`, oracle feed, and curator vault
  exposure (which curator vaults are large suppliers).
- Hypernative or equivalent monitoring at the pool / backstop / emitter
  surface. The pool emits a fixed set of events (see
  [`pool/src/events.rs`](../../pool/src/events.rs)); `propose_admin`,
  `accept_admin`, `del_auction`, and `set_emissions_config` do **not**
  emit dedicated events and must be monitored as function invocations.
  At minimum: events `set_admin` (emitted by `accept_admin`),
  `set_status`, `queue_set_reserve`, `cancel_set_reserve`, `set_reserve`,
  `update_pool`, `bad_debt`, `defaulted_debt`, `new_auction`,
  `fill_auction`, `delete_auction`; function invocations of
  `propose_admin`, `accept_admin`, `del_auction`, `set_emissions_config`;
  and backstop `q4w_pct` thresholds (30 / 50 / 60 / 75%).
- Direct contact with: the Blend dev on-call (see
  [`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)),
  every curator vault that holds a material share of your pool, and the
  Stellar Foundation security contact
  ([`03-stellar-foundation.md`](./03-stellar-foundation.md)).
- A pre-drafted public statement template per status transition.

### Pool status reference

The status code controls every user action on the pool. From
[`pool/src/pool/status.rs`](../../pool/src/pool/status.rs):

| Status | Source | Meaning | Borrow | Supply | Withdraw | Liquidate | Cancel liq |
|--------|--------|---------|--------|--------|----------|-----------|------------|
| 0 | admin `set_status` | admin active | yes | yes | yes | yes | yes |
| 1 | backstop `update_status` | backstop active | yes | yes | yes | yes | yes |
| 2 | admin `set_status` | admin on-ice | **no** | yes | yes | yes | **no** |
| 3 | backstop `update_status` | backstop on-ice | **no** | yes | yes | yes | **no** |
| 4 | admin `set_status` | **admin frozen** (supersedes all) | **no** | **no** | yes | yes | **no** |
| 5 | backstop `update_status` | backstop frozen | **no** | **no** | yes | yes | **no** |
| 6 | (setup only) | not user-callable | n/a | n/a | n/a | n/a | n/a |

Status 4 (admin frozen) is the only state that *only the admin* can move out
of (`update_status` panics with `StatusNotAllowed` if status is 4 or 6).
Status 0 / 2 / 4 are admin-set; status 1 / 3 / 5 are backstop-driven.

`update_status` is permissionless. The transition it produces depends on
the *current* status (see `execute_update_pool_status` in
[`pool/src/pool/status.rs`](../../pool/src/pool/status.rs)):

| Current status | Behaviour of `update_status` |
|----------------|------------------------------|
| 4 (admin frozen) | Panics with `StatusNotAllowed`. Only admin `set_status` can move out. |
| 6 (setup) | Panics with `StatusNotAllowed`. |
| 2 (admin on-ice) | Stays at 2 unless `q4w_pct` ≥ 75%, in which case → 5. |
| 0 (admin active) | Stays at 0 unless `q4w_pct` ≥ 50% or backstop threshold not met, in which case → 3. |
| 1 / 3 / 5 (backstop-driven) or default | If `q4w_pct` ≥ 60% → 5; else if `q4w_pct` ≥ 30% or threshold not met → 3; else → 1. |

The 30% / 50% / 60% / 75% thresholds therefore matter at different
*current* statuses: 60% only forces a freeze when the pool is in the
backstop-driven branch, 75% forces a freeze from admin on-ice, and 50%
demotes admin active to backstop on-ice.

---

## 1. Triage

When an alert fires that implicates your pool:

1. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   Pool admin is *lead* for any incident classified as pool admin
   compromise; *technical advisor* for protocol hack / oracle / bad debt
   (where the dev team leads); and *coordinator* for curator / allocator /
   sentinel compromises that touch your pool.
2. **Snapshot state**: pool status, reserve list, per-reserve `Reserve`
   view, backstop `pool_data().q4w_pct`, all open auctions.
3. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
4. **Pick the smallest viable mitigation.** A pool admin's instinct should
   be `set_status(2)` (admin on-ice) before `set_status(4)` (admin
   frozen) — but if there is *any* evidence of value extraction in
   progress, jump straight to 4.

---

## 2. Protocol hack

The pool admin is not the dev team. Your job during a protocol hack is to
**buy time** while the dev team and Stellar Foundation triage.

### 2.1 Containment ladder

| Step | Action | Effect | Reversible? |
|------|--------|--------|-------------|
| A | `set_status(2)` admin on-ice | Blocks new borrows and liquidation cancels. Supplies and withdrawals continue. | Yes (admin can `set_status(0)` if backstop is healthy). |
| B | `set_status(4)` admin frozen | Blocks borrows, supplies, liquidation cancels. Withdrawals and liquidations continue. Permissionless `update_status` is disabled while in 4. | Yes, but only the admin can move out. |
| C | Extend ledger TTL on every relevant entry — Soroban TTL is per ledger entry, not per contract. The pool / backstop / oracle code uses instance storage, persistent storage (reserves, user positions, queued reserves, queued admin proposals), and temporary storage (auctions and other short-lived state); each tier has its own TTL extension path. Inventory and bump every entry the incident depends on. | Prevents incident-relevant ledger entries from expiring during a long incident. | Yes (TTL is monotonic). |

Use A if the suspected exploit needs new borrowing to extract value (most
oracle / parameter abuses). Use B if the exploit can extract value without
borrowing (e.g. a withdrawal-side exploit), with one nuance: status 4 still
allows withdrawals — if the exploit is a withdraw-path exploit you cannot
fully stop it from on-chain admin alone. Coordinate immediately with the
dev team and Stellar Foundation Safe Chain
([`03-stellar-foundation.md`](./03-stellar-foundation.md)) for cross-stack
mitigations: validator / quorum coordination, RPC and Horizon endpoint
throttling and gating, bridge and stablecoin operator controls (denylists,
issuer freezes), and complementary pool / vault status actions on adjacent
pools or vaults that share exposure (status 4, sentinel `Pause`,
allocator deallocation).

### 2.2 What you should *not* do

- **Do not call `set_emissions_config` to "rescue" emissions** unless the
  dev team has confirmed it is safe. It can interact with cached state in
  unexpected ways.
- **Do not call `queue_set_reserve` with rushed parameters** to "patch" a
  bad reserve. `queue_set_reserve` only takes effect on the next
  `set_reserve` (which is permissionless), and the queue can be
  front-run. Use `cancel_set_reserve` if you need to back out.
- **Do not call `propose_admin` to a "safe" address** under time pressure.
  Admin transfer requires `accept_admin` from the new address; if the new
  address cannot complete that flow, you may strand the pool.

### 2.3 Recovery

Recovery from a protocol hack is dev-team-led
([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md) section
2.3). Your role is to:

1. Hold the pool at status 4 until the dev on-call lead and at least one
   second Safe Chain role sign off. Status 4 already permits withdrawals
   and liquidations, so users can migrate while supplies and new borrows
   are blocked.
2. If during migration you need to allow new supplies (e.g. backfill a
   reserve to support orderly liquidations), step `set_status(2)` admin
   on-ice (requires `q4w_pct` < 75%). Borrows and liquidation
   cancellations remain blocked. Do *not* step to 0 — admin active —
   until the dev team has signed off on the patched contract.
3. Once user funds are migrated, leave the old pool at status 4 forever.

---

## 3. Bad debt

Bad debt is contained to the affected reserve in the affected pool — Blend
isolates losses per pool. The pool admin's job is to make sure the loss is
*recognised* (transferred to the backstop) rather than left to grow, and to
prevent the same parameter mistake from continuing.

### 3.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Backstop `q4w_pct` ≥ 75% — the next `update_status` will move the pool to 5 frozen. |
| **High (P1)** | `q4w_pct` between 60 and 75%; multiple bad-debt auctions stalled (`del_auction` only callable after 500 blocks of staleness). |
| **Medium (P2)** | Single user with negative health that liquidators cannot clear; backstop has < 5% of threshold. |

### 3.2 Containment

1. **Step status to admin on-ice (`set_status(2)`)** to stop new borrows
   while existing positions are being worked through. Supplies and
   withdrawals continue.
2. **Trigger `bad_debt(user)`** for any user with liabilities and no
   collateral (anyone can call this; it is not admin-only). This passes
   the residual debt to the backstop.
3. **Check backstop solvency.** If the backstop is below ~5% of threshold,
   the next `bad_debt` call against the backstop itself will *default*
   the loss — i.e. socialize it across suppliers in that reserve. Confirm
   with the dev team that this is the intended outcome.
4. **Coordinate with vault curators** so they can deallocate from the
   affected reserve before more depositors share the loss.

### 3.3 Recovery

1. Once liabilities have been processed and `q4w_pct` recovers below
   thresholds, you can step status 2 → 0 (or let the backstop drive
   1 / 3).
2. **Tighten the reserve config** if the bad debt was caused by aggressive
   parameters. `queue_set_reserve` with reduced `c_factor` /
   `l_factor` / `max_util`, then publicly announce the change before the
   permissionless `set_reserve` execution applies it. Use
   `cancel_set_reserve` if you need to revise.
3. Communicate the loss size, who bore it, and the parameter delta in a
   public post-mortem.

---

## 4. Faulty oracle

The pool admin does not control the oracle contract. You can only act on
the pool side.

### 4.1 Containment

1. If the oracle is **stale**, most operations will already fail — borrows
   and liquidations require fresh prices. Confirm by checking the
   `Reserve` view for the affected asset.
2. If the oracle is **manipulated** or **wrong**, fresh prices are *worse*
   than no prices. `set_status(4)` admin frozen immediately. Do not wait
   for confirmation.
3. Coordinate with the dev team
   ([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)
   section 4) and the oracle provider (Stellar Foundation maintains those
   relationships, see
   [`03-stellar-foundation.md`](./03-stellar-foundation.md) section 4).

### 4.2 Recovery

1. Wait for the oracle provider's all-clear.
2. Step status 4 → 2 → 0, never skipping.
3. **Wiring faults split into two cases**:
   - **Immutable pool / reserve fields** — the reserve's
     `decimals`, the `oracle` contract address baked into the pool
     config, and other fields fixed at `set_reserve` time cannot be
     edited freely after the reserve is initialised. The fix is a
     *new pool*; recover by migrating users.
   - **Mutable proxy-oracle config** — if the wiring fault is in a
     replaceable layer (e.g. wrong `price_id`, wrong `max_age` /
     freshness filter, wrong source selection in a Templar
     [proxy-oracle](https://github.com/Templar-Protocol/contracts/tree/dev/contract/proxy-oracle)
     aggregator), it can be fixed via the proxy's own governance
     while the pool is held at status 4. Coordinate with the proxy-
     oracle operator and verify the corrected feed end-to-end before
     stepping the pool out of status 4.

---

## 5. Pool admin compromise (your own key)

If you suspect your admin key is compromised — broadcast a `set_status(4)`
**before** doing anything else, even before convening the war room. This is
the single most valuable minute you will spend.

### 5.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | `propose_admin` invocation you did not authorize (no event is emitted; this is function-call monitoring); a `set_admin` event firing from an `accept_admin` invocation by an address you did not authorize; `set_status` event you did not authorize; signing infrastructure (HSM / multisig coordinator) shows tampering. |
| **High (P1)** | A signer is socially-engineered or phished; a multisig threshold has been narrowed; recovery seed is exposed. |
| **Medium (P2)** | Unusual sign-in attempts on signing infrastructure; one signer is unreachable for an unusual period. |

### 5.2 Containment

1. **Status 4 immediately**, with whatever signing capacity you still have
   that is *known* clean. If you cannot reach status 4, every step below
   becomes much harder.
2. **Convene the war room.** The pool admin is *lead* for this incident
   class. The dev team is technical advisor. Stellar Foundation is comms
   coordinator and Safe Chain owner.
3. **Rotate the admin** — but only if you control a clean address that
   can `accept_admin`. Two-step transfer means a single bad
   `propose_admin` is recoverable as long as you control the old admin
   and the proposed address never accepts. If the attacker has already
   completed `accept_admin`, the old key is no longer admin and you
   cannot reverse it on-chain.
4. If the attacker is now the admin, the pool is **functionally lost**.
   Coordinate with vault curators and Stellar Foundation Safe Chain for
   user-fund-protection options (migration to a new pool with a new
   admin, paused integrations, allowlist updates upstream).

### 5.3 Recovery

1. Forensic review: how did the key leak?
2. Replace signing infrastructure end-to-end. Do not reuse compromised
   hardware or compromised people-in-the-loop processes.
3. If you control a recovered admin, step status 4 → 2 → 0 only after the
   dev team has reviewed all admin-level state on the pool
   (`get_config()`, reserve list, queued reserves via
   `cancel_set_reserve` checks, emissions config).
4. Publish a post-mortem.

---

## 6. Curator / allocator / sentinel compromise

You are not the curator. But a captured curator can damage your pool by:

- Mass-supplying into a reserve to drive utilisation up (raises rates,
  starves borrowers).
- Mass-withdrawing during stress (forces liquidity events and may push
  `q4w_pct` if backstop deposits are unwound to follow).
- Allocating to a reserve with a known weakness in coordination with an
  external exploit.

### 6.1 Detection

Watch for:

- Sudden, large bidirectional flows from a single curator vault address.
- `b_rate` discontinuities on a single reserve.
- Coordinated allocator activity timed with oracle anomalies.

### 6.2 Containment

1. **Coordinate with the curator** following
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md).
2. If the curator confirms compromise and the captured role is still
   able to act, consider `set_status(2)` admin on-ice on the affected
   pool to stop the captured allocator from driving more borrowing
   activity.
3. Do **not** target the curator's vault directly from the pool side; the
   pool has no concept of curators and any blanket pause hits all
   suppliers, not just the captured one. Coordinate with the curator's
   sentinel / governance instead.

### 6.3 Recovery

When the curator confirms recovery, return to standing posture per their
sign-off + dev team sign-off (Safe Chain rule).

---

## 7. Communication protocol

- **First minute**: status 4 (if applicable) before any messages.
- **First fifteen minutes**: status post — "We have moved pool X to admin
  frozen (status 4) following <one-line reason>. User funds in the pool
  remain in custody of the pool. No supplies or new borrows are possible.
  Withdrawals continue. We will publish next update within <N> minutes."
- **Hourly**: status updates while the war room is open.
- **Stand-down**: only after dev on-call lead and one further Safe Chain
  role have signed off (Foundation, second admin, or a major curator
  with skin in the pool).

Keep all communication factual and parameterised. Do not speculate about
attacker identity or attribution.

---

## 8. Stand-down checklist

- [ ] Pool status path back to operational (4 → 2 → 0) verified by the dev
      on-call lead.
- [ ] Backstop `q4w_pct` is below 30%.
- [ ] No queued reserve config changes are pending unintentionally.
- [ ] All affected curator vaults have stepped down.
- [ ] Stellar Foundation contact informed of stand-down.
- [ ] Public post-mortem drafted, including parameter changes and any
      migration path.
- [ ] War room archived (append-only log preserved).
- [ ] Signing posture reviewed: same key set, same rotation cadence, same
      hardware. If any of these changed during the incident, document
      why.
