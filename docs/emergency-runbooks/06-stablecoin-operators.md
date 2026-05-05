# Runbook: Stablecoin Operators

**Audience.** Issuers and operators of stablecoins (and other
reserve-backed or authority-controlled tokens) on Stellar whose tokens
are listed as Blend pool reserves or held by curator vaults that supply
into Blend.

**Scope.** Issuer-level controls over the token: issuance, redemption,
authority-flag-driven address freeze and clawback (where applicable
under the issuer's policy and law), pause of mint / redeem rails. You
do not call any Blend pool function. You participate in the Stellar
Safe Chain so that exit / arbitrage paths through your token are not
weaponised during a Blend incident, and so that issuance-side
incidents (depeg, reserve discrepancy) are coordinated with Blend
responders.

Read [`README.md`](./README.md) first for the severity matrix, Hypernative
classification, war room template, and Safe Chain coordination model.

---

## 0. Standing posture

Before any incident, the stablecoin operator must keep the following in
a known-good state:

- A documented list of Blend pools that list your stablecoin as a
  reserve and the curator vaults that hold material exposure.
- Documented internal policies and legal preconditions for any freeze
  / clawback action on user addresses. These actions are heavily
  governed and must not be a runtime decision.
- An incident contact directory shared with Stellar Foundation
  ([`03-stellar-foundation.md`](./03-stellar-foundation.md)), Blend
  pool admins listing your stablecoin
  ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md)), curator
  vaults
  ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)),
  and bridge operators
  ([`05-bridge-operators.md`](./05-bridge-operators.md)) that
  bridge your stablecoin.
- Hypernative or equivalent monitoring on issuance, redemption, large
  transfers, peg deviation against canonical references, authority
  changes on the asset.
- A documented signing posture for the asset's issuing and authority
  accounts.
- A pre-shared severity classification matching the Hypernative
  taxonomy in [`README.md`](./README.md).

---

## 1. Triage

When an alert fires that implicates your stablecoin on Blend or in the
ecosystem:

1. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   Stablecoin operator is *lead* for any incident classified as
   stablecoin incident; *technical advisor and address-freeze gatekeeper*
   for protocol hack / curator compromise that uses your stablecoin as
   the exit asset.
2. **Snapshot state**: outstanding supply on Stellar, recent
   issuance / redemption volume, authority-flag posture (clawback,
   freeze flags), reserves backing summary.
3. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
4. **Confirm legal preconditions** before any freeze / clawback. Ad-hoc
   freezes during an incident are particularly fraught — make sure the
   internal policy precondition is satisfied and documented in the war
   room log.

---

## 2. Protocol hack on Blend

Most Blend exploits target value extraction in some asset. Stablecoins
are favoured exit assets because they are fungible, liquid, and
bridgeable. Your role is the gate.

### 2.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Foundation Safe Chain pages you with a confirmed exploiter address moving your stablecoin. |
| **High (P1)** | Anomalous flow of your stablecoin through a Blend pool reserve, with corresponding war-room awareness. |
| **Medium (P2)** | Foundation pages you about an in-progress Blend incident; no specific stablecoin action requested yet. |

### 2.2 Containment

1. **Confirm war-room context.** Do not act on a single Hypernative
   signal. The damage of an erroneous freeze on your token can be as
   severe as the damage from missing a real one.
2. **Confirm internal policy precondition.** Each freeze decision is
   a stand-alone determination. Document it in the war-room log
   (timestamp, signer, basis).
3. **Per-address freeze** of the captured / exploiter addresses if your
   asset configuration permits and policy allows. This is the
   precision tool — it stops the *attacker's* exit while leaving
   legitimate users unaffected.
4. **Pause of issuance / redemption rails** (off-chain) if the incident
   is large enough that net new mint or redeem during the war room
   would compound the damage.
5. **Coordinate with bridge operators**
   ([`05-bridge-operators.md`](./05-bridge-operators.md)) — a frozen
   address on Stellar can still be the destination of a bridge
   transfer; the bridge needs to know to refuse.

### 2.3 Recovery

1. Lift any temporary issuance / redemption pauses after Foundation
   Safe Chain stand-down
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 8).
2. Per-address freezes that were applied for incident-response remain
   in place per your standing policy until those addresses are
   released by your normal compliance process — do not auto-unfreeze
   on stand-down.
3. Publish your post-mortem in coordination with Foundation if the
   freeze was material to the incident.

---

## 3. Bad debt on Blend

Bad debt on a Blend pool whose asset is your stablecoin can produce a
brief peg pressure if the loss is large relative to circulating supply
on Stellar.

### 3.1 Containment

1. Confirm with the affected pool admin and curators
   ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 3 /
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md) section 3)
   that loss recognition is in flight.
2. Brief the issuer treasury / market-making desks so peg defence (if
   applicable to your stablecoin's design) is informed.
3. Coordinate timing of public statements with the Blend pool admin so
   the ecosystem-side narrative is consistent.

### 3.2 Recovery

No stablecoin-side action is normally required for stand-down — the
loss is contained to the pool. Update peg-monitoring thresholds if
the incident exposed missing coverage.

---

## 4. Faulty oracle

Stablecoin operators are sometimes oracle providers (NAV / PoR feeds).
This runbook treats the case where you are *consuming* a faulty oracle
that affects pricing of your stablecoin in Blend.

### 4.1 Containment

1. If your stablecoin's price as observed by Blend is being misread by
   the oracle, coordinate with the Blend pool admins
   ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 4)
   to step the affected pool's status.
2. Pause issuance / redemption only if the oracle fault is causing
   incoming redemption requests to be mispriced.
3. Coordinate with the oracle provider and Stellar Foundation
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 4).

### 4.2 Recovery

Per Foundation Safe Chain stand-down.

---

## 5. Pool admin compromise

A captured Blend pool admin may attempt to drain pool reserves into your
stablecoin and bridge it out. Your role is identical to the protocol-hack
case: precise per-address freezes are the right tool, with full pause as
fallback.

1. **Per-address freeze** of the captured admin's address(es), and any
   address proposed via `propose_admin(<unknown>)` on the affected
   pool, subject to your policy.
2. **Coordinate** with bridges and Foundation Safe Chain
   ([`05-bridge-operators.md`](./05-bridge-operators.md) section 5 /
   [`03-stellar-foundation.md`](./03-stellar-foundation.md) section 5).

---

## 6. Curator / allocator / sentinel compromise

A captured curator vault can drain assets that include your stablecoin.
Per-address freezes target the captured key, not depositors. Be
particularly careful here — vault depositors who are reacting to the
news may be moving your stablecoin legitimately.

---

## 7. Stablecoin-internal incidents

When the stablecoin itself is the incident (peg deviation, reserve
discrepancy, issuer-side compromise, authority key compromise), you are
*lead*. Blend pool admins, curators, bridges, and Foundation participate
as informational stakeholders.

| Class | Signal | Containment |
|-------|--------|-------------|
| **Critical (P0)** | Confirmed reserve discrepancy; peg deviation > N% for > N minutes; authority key capture. | Pause issuance and redemption. Notify Foundation immediately; Foundation pages every Blend pool admin listing your stablecoin and every curator vault holding it. Expect pool admins to step affected pools to status 2 (admin on-ice) or 4 (admin frozen). Expect curators to deallocate from reserves listing your stablecoin. |
| **High (P1)** | Single-source reserve report missing; large unexplained mint / redeem flow; peg deviation < N% but trending. | Investigate; brief Foundation; pre-warn pool admins and curators to be ready to act. |
| **Medium (P2)** | Single-monitor failure; reserve report late; routine signer hardware refresh. | Document; no external comms unless escalation required. |

In a stablecoin-internal P0, expect a long stand-down: pool admins will
not lift status 4 until your peg is restored and your reserve report is
re-published. Coordinate timing with Foundation.

---

## 8. Communication protocol

- Stablecoin issuers are subject to legal, regulatory, and customer
  obligations on disclosure timing and content. Coordinate with your
  internal legal function on every public statement.
- Foundation can broker the *timing* of cross-protocol statements but
  does not draft your statements.
- Three checkpoints (awareness, containment, stand-down) at the same
  cadence as the Foundation when participating in a Blend war room.
- For a stablecoin-internal P0, your statements are first; Foundation
  re-publishes for ecosystem consistency.

---

## 9. Stand-down checklist

- [ ] All issuance / redemption pauses lifted (or extended per standing
      policy with explicit reason).
- [ ] Authority-action freezes that were applied during the incident
      reviewed against standing policy; permanent entries handled
      through normal compliance.
- [ ] Reserve report current and published (for stablecoin-internal
      incidents).
- [ ] Foundation contact informed of stand-down.
- [ ] All affected Blend pool admins, curator vaults, and bridges
      informed of stand-down.
- [ ] Public stand-down statement published, coordinated with
      Foundation.
- [ ] War room archived (append-only log preserved).
