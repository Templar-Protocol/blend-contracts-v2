# Runbook: NEAR Foundation

**Audience.** The NEAR Foundation security and ecosystem contacts who
coordinate cross-protocol incident response on NEAR. Foundation is not
a custodian of any Templar keys, is not a Templar market admin (markets
are immutable and have no admin), but is the central node of the NEAR
Safe Chain and the natural broker between the Templar dev team,
registry admin, vault curators, NEAR Intents team, bridge operators,
stablecoin issuers, and validators.

**Scope.** Coordination, communication, and ecosystem-level
mitigations. You do not call any contract function in Templar; you
*coordinate* the parties that do, and you can apply NEAR-network-level
mitigations (e.g. validator / block-producer outreach, RPC provider
coordination, issuer / bridge controls including stablecoin freezing
requests and asset-issuer authority actions, intents-layer coordination
via the NEAR Intents team, and network upgrade coordination for
nearcore protocol upgrades).

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

Before any incident, the Foundation security function must keep the
following in a known-good state:

- A maintained directory of contacts for: the Templar dev team, the
  Templar registry admin, every major Templar vault curator, every
  major NEAR bridge operator, every stablecoin issuer whose asset is
  listed on Templar or bridged via NEAR, the NEAR Intents team, and
  every top-N validator / block-producer operator. Multiple contacts
  per organisation; out-of-band channels (phone, Signal, Matrix)
  verified quarterly.
- A pre-shared incident classification language (this runbook + the
  Hypernative taxonomy in [`README.md`](./README.md)) so that
  everyone in the war room agrees on what "P0" means.
- Standing comms templates approved by Foundation legal and comms
  for ecosystem-level messages (see section 7).
- A subscription to monitoring outputs (Hypernative, internal feeds,
  oracle provider status pages, Pyth status
  [https://status.pyth.network/](https://status.pyth.network/), NEAR
  status [https://status.near.org/](https://status.near.org/)) at
  least at the High severity level.
- A pre-defined NEAR Safe Chain quorum: which roles count toward
  stand-down sign-off for which incident classes (see section 8).

---

## 1. Triage

When a P0 / P1 alert reaches Foundation:

1. **Confirm the alert is not noise** by cross-referencing the
   affected protocol team. Foundation is not the first responder on
   a single-market or single-vault incident — the Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md))
   or the vault curator
   ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md))
   is. Foundation joins when:
   - The incident is cross-protocol (touches a bridge, a stablecoin,
     an NEAR Intents route, or a validator-set issue), or
   - The incident has visible ecosystem-level impact (peg deviation,
     systemic oracle failure, exploit on a contract used by multiple
     integrators), or
   - A protocol team requests Foundation's help.
2. **Classify** using the table in
   [`README.md`](./README.md#hypernative-alert-taxonomy). Foundation
   classification is *the* canonical severity once Foundation has
   joined the war room — protocols can disagree internally but
   external comms should match Foundation's class.
3. **Decide which Safe Chain roles to convene.** Use the dependency
   map below.

### Dependency map

| Incident family | Convene |
|-----------------|---------|
| Protocol hack on Templar market / registry | Templar dev team (lead), registry admin, vault curators supplying into affected markets, Foundation (coordination / communications support), validators (informational) |
| Protocol hack on a vault | Vault curator (lead), vault sentinel, vault dev team, Foundation (coordination), affected Templar dev team (informational) |
| Bad debt | Vault curator (lead), Templar dev team, other curators supplying into the affected market, Foundation (informational) |
| Faulty oracle | Oracle provider (lead), Templar dev team (technical), vault curators (deallocate), Foundation (oracle-provider broker), NEAR Intents team (routing adjustment) |
| Registry admin compromise | Registry admin (lead if reachable) or Foundation (lead if admin captured), Templar dev team, validators if network-level informational only, exchanges if the captured admin is moving funds, stablecoin issuers if frozen-asset action might be requested |
| Curator / allocator / sentinel compromise | Curator's governance (lead), curator's sentinel, vault dev team, Foundation, affected Templar dev team (informational) |
| NEAR Intents-layer compromise | NEAR Intents team (lead), Foundation, Templar dev team and affected vault curators (informational) |
| Bridge incident | Bridge operator (lead), Foundation (lead on ecosystem comms), Templar dev team if a bridged asset is listed on a market, vault curators exposed to bridged assets, stablecoin issuers if a bridged stablecoin is involved |
| Stablecoin incident | Stablecoin issuer (lead), Foundation, Templar dev team if the stablecoin is listed on any market, vault curators holding the stablecoin |
| Validator misbehaviour | Foundation (lead), validator operator(s), all of the above (informational) |

---

## 2. Protocol hack

When the alert family is "exploit / invariant break" on Templar or on
a curator vault, Foundation's job is to coordinate, not to act on-chain.

### 2.1 First fifteen minutes

1. **Confirm war room** is open and that the lead role for the
   incident class has joined.
2. **Confirm the smallest containment** has been taken. For Templar
   market exploits: markets have no on-chain pause, so containment
   means (a) has the Templar dev team halted the liquidator and
   accumulator bots? and (b) has public communication gone out
   asking users to avoid new borrows / supplies? For curator vault
   exploits, two questions, since the Templar governance contract
   requires the Admin (not the Sentinel) to submit a pause, while
   the Sentinel / Guardian / Admin can all revoke pending
   proposals (see
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)
   §2.2): (a) has the governance Admin / Owner called
   `submit_set_paused(true)` (immediate, no timelock)? and (b) has
   the Sentinel / Guardian / Admin revoked any pending proposals
   that would increase risk during the incident, including any
   pending unpause? If any is missing, ask why.
3. **Identify the blast radius.** Which other vault curators, which
   NEAR Intents routes, which bridged asset flows, which
   stablecoin issuers are exposed?
4. **Notify adjacents.** NEAR Intents team, bridges holding the
   affected asset, stablecoin issuers if a stablecoin reserve is
   involved, exchanges that list the token. Pre-incident comms
   templates apply (section 7).

### 2.2 Cross-stack mitigations Foundation can request

Foundation does not have unilateral on-chain authority, but can
request:

- **Stablecoin freeze of attacker addresses.** If the attacker is
  moving USDC / USDT / etc. on NEAR, the issuer can freeze those
  addresses under their asset authority (see
  [`07-stablecoin-operators.md`](./07-stablecoin-operators.md)).
  Foundation makes the request; the issuer decides on policy and
  law grounds.
- **Bridge halt.** If the attacker is bridging out (Rainbow Bridge,
  OmniBridge, deBridge, etc.), the bridge operator can pause the
  bridge or block specific addresses (see
  [`06-bridge-operators.md`](./06-bridge-operators.md)).
- **NEAR Intents route pause / solver revocation.** If the attacker
  is exiting via intent-routed flow, the NEAR Intents team can
  denylist the attacker's address, pause the affected route, or
  revoke a complicit solver (see
  [`02-near-intents-team.md`](./02-near-intents-team.md)).
- **Validator outreach.** Foundation maintains validator
  relationships and can ask validators to be aware of unusual
  transaction patterns and to follow
  [`08-near-validators.md`](./08-near-validators.md).
- **Network upgrade coordination.** In an extreme case, a nearcore
  upgrade can be coordinated to address protocol-level issues.
  This is rare, slow, and only contemplated for the worst-case
  ecosystem incidents (e.g. a consensus-layer issue, not an
  application-layer hack).

### 2.3 Recovery

Foundation's role in recovery is to:

1. **Verify migration plan** with the lead role.
2. **Coordinate communication windows** so that market migrations,
   stablecoin freeze releases, bridge re-opens, intents-route
   reopens, and exchange listings align in time. A staggered
   uncoordinated stand-down is itself a risk.
3. **Sign off** on stand-down per the Safe Chain quorum.

---

## 3. Bad debt

Foundation is *not* a primary actor on a bad-debt incident on
Templar — the loss is contained to the affected market and
propagates to that market's suppliers. Foundation joins if:

- Bad debt is large enough to threaten a curator vault's solvency,
  in which case curator depositors may need clear public guidance,
  or
- The bad debt has produced a peg / liquidity event visible at the
  ecosystem level (e.g. an LST loses its peg and cascades into
  other integrators).

In those cases, Foundation coordinates a single ecosystem-level
statement and ensures curators, stablecoin issuers, and the NEAR
Intents team are aware.

---

## 4. Faulty oracle

Oracle providers (Pyth, Redstone, LST oracle operators, proxy
oracle operators) typically maintain Foundation as a primary
point-of-contact across protocols. This is where Foundation has
unique value.

### 4.1 Containment workflow

1. **Verify the oracle issue** with the provider. Foundation pages
   the provider's incident contact; provider confirms or denies.
2. **Broadcast across affected protocols.** Every Templar market
   whose oracle uses the affected feed needs to know within
   minutes. So does every vault curator whose policy depends on
   the feed. So do NEAR Intents routes that quote through affected
   markets.
3. **Coordinate timing** of bot halts, vault sentinel pauses, and
   intents route throttles so the ecosystem-level state during the
   incident is coherent.

### 4.2 Recovery

1. Provider publishes all-clear; Foundation verifies and
   re-broadcasts.
2. Vault curators restart per
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md) §4.
3. NEAR Intents team restarts routes per
   [`02-near-intents-team.md`](./02-near-intents-team.md) §4.
4. Foundation publishes a single ecosystem post-mortem for the
   oracle incident, naming each affected protocol and its
   containment timeline.

---

## 5. Registry admin compromise

Foundation's most distinctive role in the Templar context. Because
Templar markets themselves are immutable and have no admin, the
registry admin is the single privileged NEAR-side surface, and a
compromise here has broad blast radius: attacker can `add_version`
malicious code and `deploy` fake markets that look official.

### 5.1 Containment workflow

1. **Confirm the admin is captured** with the Templar dev team and
   the registry admin's emergency contact (which must be different
   from the captured key). Partial captures (one signer of a
   multi-sig where applicable) may still be safe — verify.
2. **Coordinate the cross-stack defence.** A captured registry
   admin can:
   - `add_version` backdoored code that later `deploy` uses.
   - `deploy` fake markets.
   - `remove_version` legitimate versions.
   - `upgrade` the registry itself.
   Defences Foundation can broker:
   - Stablecoin issuers can freeze the captured admin's
     address(es) and any address that receives adversarial-deploy
     fee payments.
   - Bridges can refuse to bridge funds from the captured admin's
     address(es).
   - Exchanges can flag deposits from the captured admin.
   - The NEAR Intents team can exclude any newly deployed or
     updated markets from solver routing tables pending Safe Chain
     sign-off.
   - Validators can be made aware (informational) and follow
     [`08-near-validators.md`](./08-near-validators.md). The NEAR
     consensus layer is *not* a censorship surface; validators do
     not censor specific transactions on request.
3. **Coordinate user comms.** Vault curators, integrators, and
   front ends should treat any registry action after the capture
   time as untrusted until Safe Chain sign-off. Foundation
   coordinates the public statement and the recommended
   verification procedure (e.g. checking `get_version_code_hash`
   against Templar-published hashes).

### 5.2 Recovery

1. The compromised registry is functionally lost as long as the
   attacker holds the owner key. Recovery is a fresh-registry
   deployment coordinated by the Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §5).
2. Foundation sign-off is required on stand-down (Safe Chain rule).
   The sign-off conditions: the compromised registry is publicly
   marked unsafe, the fresh registry is deployed and audited, the
   known-good versions are published into it, no further
   attacker-driven configuration changes are observed.

---

## 6. Curator / allocator / sentinel compromise

Curator vaults are a category where Foundation's role is
*informational and coordinative*. The vault's own governance,
sentinel, and curator are the on-chain actors. Foundation:

- Confirms the compromise is real (cross-checks with the vault
  operator's emergency contact).
- Notifies adjacent stakeholders — the Templar dev team, other
  curators, the NEAR Intents team (routing implications), bridges
  and stablecoin issuers whose assets are in the vault.
- Brokers stablecoin / bridge / intents defences only if the
  captured key is actively moving material assets.
- Signs off on stand-down (Safe Chain quorum).

---

## 7. NEAR Intents-layer compromise

Similar coordination role. The NEAR Intents team is lead
([`02-near-intents-team.md`](./02-near-intents-team.md) §7).
Foundation:

- Confirms the compromise with the intents team's emergency
  contact.
- Notifies Templar dev team and affected vault curators that
  intent-mediated flow may be unreliable during the incident.
- Coordinates comms with bridges, stablecoin issuers, and
  exchanges whose flow is intent-routed.

---

## 8. Communication protocol

Foundation communications during an incident follow these rules:

- **One voice.** No-one inside Foundation comms publishes about the
  incident except the war-room comms lead (or their designate).
- **No attribution.** Until forensics is complete, no attribution
  statements. "We are aware of an incident" is the maximum claim.
- **Coordinate timing with other roles.** A Foundation tweet that
  contradicts a curator's pause notice undermines both. Comms
  leads across the war-room roles co-author the public statement.
- **Three statement classes:**
  1. **Awareness**: "Foundation is aware of an incident on
     <protocol / market / asset>. The teams involved are
     coordinating. We will share more in <interval>." Posted
     within 30 minutes of P0 detection.
  2. **Containment**: "Containment action <X> has been taken by
     <role>. <User-facing impact>. No further user action
     required" / or specific user instructions if there are any.
  3. **Stand-down**: "The incident is resolved. <Brief root
     cause>. A full post-mortem will be published within
     <interval>."
- **Post-mortem within 14 days** for any P0/P1, jointly authored
  with the lead role for the incident.

---

## 9. NEAR Safe Chain quorum

Stand-down for any P0/P1 requires sign-off from the lead role *and*
at least one further Safe Chain role. Foundation maintains the
quorum definition:

| Incident family | Lead | Required additional sign-off |
|-----------------|------|-------------------------------|
| Protocol hack on Templar | Templar dev team | Registry admin OR Foundation |
| Protocol hack on a vault | Vault curator | Foundation OR Templar dev team |
| Bad debt | Vault curator | Templar dev team |
| Faulty oracle | Oracle provider (off-Safe-Chain) | Templar dev team AND Foundation |
| Registry admin compromise (admin still controlled) | Registry admin | Templar dev team AND Foundation |
| Registry admin compromise (admin captured) | Foundation | Templar dev team AND an independent second role (unaffected vault curator, major integrator, or external auditor) |
| Curator / allocator / sentinel compromise | Curator governance | Foundation |
| NEAR Intents-layer compromise | NEAR Intents team | Foundation AND Templar dev team |
| Bridge incident | Bridge operator | Foundation AND any affected stablecoin issuer |
| Stablecoin incident | Stablecoin issuer | Foundation |
| Validator misbehaviour | Foundation | Validator operator(s) |

Sign-off is recorded in the war-room log with the signer name,
time, and a one-line statement of what they are signing off on.

---

## 10. Stand-down checklist

- [ ] Lead role has declared incident contained.
- [ ] Required additional Safe Chain sign-off recorded in war-room
      log.
- [ ] All affected parties (per dependency map in section 1)
      notified of stand-down.
- [ ] Public stand-down statement published.
- [ ] Post-mortem owner assigned with a 14-day deadline.
- [ ] War room archived (append-only log preserved).
- [ ] Standing posture (contact directory, monitoring
      subscriptions, comms templates) reviewed for changes the
      incident exposed.
