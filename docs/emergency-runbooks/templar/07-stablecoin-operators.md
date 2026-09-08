# Runbook: Stablecoin Operators

**Audience.** Issuers and operators of stablecoins (and other
authority-controlled tokens) on NEAR whose tokens are listed as
Templar market reserves or held by Templar curator vaults on NEAR.

**Scope.** Issuer-level controls over the token: issuance,
redemption, address-freeze / clawback / authority-flag operations
where applicable under the issuer's policy and law, pause of mint /
redeem rails. You do not call any Templar contract function. You
participate in the NEAR Safe Chain so that exit / arbitrage paths
through your token are not weaponised during a Templar incident, and
so that issuance-side incidents (depeg, reserve discrepancy) are
coordinated with Templar responders.

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

Before any incident, the stablecoin operator must keep the following
in a known-good state:

- A documented list of Templar markets that list your stablecoin as
  a reserve and the curator vaults that hold material exposure.
- Documented internal policies and legal preconditions for any
  freeze / clawback action on user addresses. These actions are
  heavily governed and must not be a runtime decision.
- An incident contact directory shared with NEAR Foundation
  ([`03-near-foundation.md`](./03-near-foundation.md)), Templar dev
  team ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md)),
  vault curators listing your stablecoin
  ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)),
  bridge operators
  ([`06-bridge-operators.md`](./06-bridge-operators.md)) that
  bridge your stablecoin into or out of NEAR, and the NEAR Intents
  team ([`02-near-intents-team.md`](./02-near-intents-team.md)) if
  your stablecoin participates in intent routes.
- Hypernative or equivalent monitoring on issuance, redemption,
  large transfers, peg deviation against canonical references, and
  authority changes on the token contract or the token's NEAR
  account.
- A documented signing posture for the token's privileged accounts
  (issuer authority, freeze authority, upgrade authority).
- A pre-shared severity classification matching the Hypernative
  taxonomy in [`README.md`](./README.md).

---

## 1. Triage

When an alert fires that implicates your stablecoin on Templar or in
the ecosystem:

1. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   Stablecoin operator is *lead* for any incident classified as
   stablecoin incident; *technical advisor and address-freeze
   gatekeeper* for protocol hack / curator compromise / registry
   compromise that uses your stablecoin as the exit asset.
2. **Snapshot state**: outstanding supply on NEAR, recent issuance
   / redemption volume, authority-key posture, reserves backing
   summary.
3. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
4. **Confirm legal preconditions** before any freeze / clawback.
   Ad-hoc freezes during an incident are particularly fraught —
   make sure the internal policy precondition is satisfied and
   documented in the war-room log.

---

## 2. Protocol hack on Templar

Most Templar exploits target extraction of value in some asset.
Stablecoins are favoured exit assets. Your role is the gate.

### 2.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | NEAR Foundation Safe Chain pages you with a confirmed exploiter address moving your stablecoin. |
| **High (P1)** | Anomalous flow of your stablecoin through a Templar market, with corresponding war-room awareness. |
| **Medium (P2)** | NEAR Foundation pages you about an in-progress Templar incident; no specific stablecoin action requested yet. |

### 2.2 Containment

1. **Confirm war-room context.** Do not act on a single Hypernative
   signal. Erroneous freeze damage can rival missed-freeze damage.
2. **Confirm internal policy precondition.** Each freeze decision
   is a stand-alone determination. Document in the war-room log
   (timestamp, signer, basis).
3. **Per-address freeze / clawback** of the captured / exploiter
   addresses if your asset configuration permits and policy allows.
   Precision tool — stops the *attacker's* exit while leaving
   legitimate users unaffected. On NEAR, most authority-controlled
   tokens implement this as a contract-side operation on the token
   contract itself; the exact mechanism (freeze, clawback,
   blacklist) depends on the token implementation.
4. **Pause of issuance / redemption rails** (off-chain) if the
   incident is large enough that net new mint or redeem during the
   war room would compound the damage.
5. **Coordinate with bridge operators**
   ([`06-bridge-operators.md`](./06-bridge-operators.md)). If your
   stablecoin's on-NEAR contract implements a freeze, a frozen
   address cannot transact in the token on NEAR, so on-chain
   inbound bridge transfers to that address will typically fail at
   the token layer. Bridge coordination is still required for
   surfaces the on-NEAR freeze does not reach: bridge-side
   off-chain queues / pre-confirmation flows, wrapped
   representations on the destination chain, and any inflight
   messages that were already attested before the freeze.
6. **Coordinate with the NEAR Intents team**
   ([`02-near-intents-team.md`](./02-near-intents-team.md)) if
   intent-mediated flow through your stablecoin is a concern.

### 2.3 Recovery

1. Lift any temporary issuance / redemption pauses after NEAR
   Foundation Safe Chain stand-down
   ([`03-near-foundation.md`](./03-near-foundation.md) §8).
2. Per-address freezes applied for incident-response remain in
   place per your standing policy until those addresses are
   released by your normal compliance process — do not auto-
   unfreeze on stand-down.
3. Publish your post-mortem in coordination with NEAR Foundation
   if the freeze was material to the incident.

---

## 3. Bad debt on Templar

Bad debt on a Templar market denominated in your stablecoin can
produce brief peg pressure if the loss is large relative to
circulating supply on NEAR.

1. Confirm with the affected vault curator and Templar dev team
   that loss recognition is in flight.
2. Brief the issuer treasury / market-making desks so peg defence
   (if applicable to your stablecoin's design) is informed.
3. Coordinate timing of public statements with the Templar dev
   team so the narrative is consistent.

No stablecoin-side on-chain action is normally required. Update
peg-monitoring thresholds if the incident exposed missing coverage.

---

## 4. Faulty oracle

Stablecoin operators are sometimes oracle providers (NAV / PoR
feeds). This runbook treats the case where you are *consuming* a
faulty oracle that affects pricing of your stablecoin in Templar.

1. If your stablecoin's price as observed by Templar is being
   misread by the oracle, coordinate with Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §4).
2. Pause issuance / redemption only if the oracle fault is causing
   incoming redemption requests to be mispriced.
3. Coordinate with the oracle provider and NEAR Foundation
   ([`03-near-foundation.md`](./03-near-foundation.md) §4).

---

## 5. Registry admin compromise

A captured Templar registry admin may deploy fake markets that
list your stablecoin as a reserve and drain them. Your role: refuse
integration with any unverified deployment and treat address
denylist coordination as in §2.

---

## 6. Curator / allocator / sentinel compromise

A captured curator vault can drain assets that include your
stablecoin. Per-address freezes target the captured key, not
depositors. Be particularly careful — vault depositors reacting to
the news may be moving your stablecoin legitimately.

---

## 7. Stablecoin-internal incidents

When the stablecoin itself is the incident (peg deviation, reserve
discrepancy, issuer-side compromise, authority key compromise), you
are *lead*. Templar dev team, vault curators, bridges, NEAR
Intents team, and NEAR Foundation participate as informational
stakeholders.

| Class | Signal | Containment |
|-------|--------|-------------|
| **Critical (P0)** | Confirmed reserve discrepancy; peg deviation exceeding your issuer's published P0 threshold, measured against your published reference price over your published evaluation window (default recommendation if unpublished: > 100 bps for > 15 minutes against the reference price defined in your peg-monitoring policy); authority key capture. | Pause issuance and redemption. Notify NEAR Foundation immediately; Foundation pages every Templar dev-team contact, vault curator holding your stablecoin, bridge operator moving it, and the NEAR Intents team. Expect vault curators to Sentinel-pause vaults holding the stablecoin. Templar dev team will halt bots defensively. |
| **High (P1)** | Single-source reserve report missing; large unexplained mint / redeem flow; peg deviation approaching but under the P0 threshold and trending (default recommendation if unpublished: 25–100 bps sustained over the evaluation window). | Investigate; brief NEAR Foundation; pre-warn curators and dev team to be ready to act. |
| **Medium (P2)** | Single-monitor failure; reserve report late; routine signer hardware refresh. | Document; no external comms unless escalation required. |

In a stablecoin-internal P0, expect a long stand-down: vault
curators will not lift Sentinel pauses until your peg is restored
and your reserve report is re-published. Coordinate timing with
NEAR Foundation.

---

## 8. Communication protocol

- Stablecoin issuers are subject to legal, regulatory, and customer
  obligations on disclosure timing and content. Coordinate with
  your internal legal function on every public statement.
- NEAR Foundation brokers the *timing* of cross-protocol
  statements but does not draft your statements.
- Three checkpoints (awareness, containment, stand-down) at the
  same cadence as NEAR Foundation when participating in a Templar
  war room.
- For a stablecoin-internal P0, your statements are first;
  Foundation re-publishes for ecosystem consistency.

---

## 9. Stand-down checklist

- [ ] All issuance / redemption pauses lifted (or extended per
      standing policy with explicit reason).
- [ ] Authority-action freezes applied during the incident reviewed
      against standing policy; permanent entries handled through
      normal compliance.
- [ ] Reserve report current and published (for stablecoin-internal
      incidents).
- [ ] NEAR Foundation contact informed of stand-down.
- [ ] All affected Templar vault curators, dev team, bridges, and
      NEAR Intents team informed of stand-down.
- [ ] Public stand-down statement published, coordinated with NEAR
      Foundation.
- [ ] War room archived (append-only log preserved).
