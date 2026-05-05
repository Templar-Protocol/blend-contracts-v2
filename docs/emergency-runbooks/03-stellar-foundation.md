# Runbook: Stellar Foundation

**Audience.** The Stellar Development Foundation security and ecosystem
contacts who coordinate cross-protocol incident response on Stellar. The
Foundation is not a custodian of any Blend or curator-vault keys, and is not
a Blend pool admin, but is the central node of the Stellar Safe Chain and
is the natural broker between the Blend protocol team, pool admins,
curators, bridge operators, stablecoin issuers, validators, and exchanges.

**Scope.** Coordination, communication, and ecosystem-level mitigations.
You do not call any contract function in Blend or in the curator vaults; you
*coordinate* the parties that do, and you can apply Stellar-network-level
mitigations (e.g. validator / quorum outreach, RPC and Horizon operator
coordination, issuer / bridge controls including stablecoin freezing
requests and asset-issuer revocation actions, and network upgrade
coordination).

Read [`README.md`](./README.md) first for the severity matrix, Hypernative
classification, war room template, and Safe Chain coordination model.

---

## 0. Standing posture

Before any incident, the Foundation security function must keep the
following in a known-good state:

- A maintained directory of contacts for: every Blend pool admin, every
  major curator vault operator (e.g. Templar — the
  [`Templar-Protocol/contracts`](https://github.com/Templar-Protocol/contracts)
  vault stack), every bridge operator that touches Stellar assets, every
  stablecoin issuer (USDC issuer, EURC issuer, BRL issuer, etc.), and
  every top-N validator operator. Multiple contacts per organisation;
  out-of-band channels (phone, Signal, Matrix) verified quarterly.
- A pre-shared incident classification language (this runbook + the
  Hypernative taxonomy in [`README.md`](./README.md)) so that everyone in
  the war room agrees on what "P0" means.
- Standing comms templates approved by Foundation legal and comms for
  ecosystem-level messages (see section 7).
- A subscription to monitoring outputs (Hypernative, internal feeds, oracle
  provider status pages) at least at the High severity level.
- A pre-defined Safe Chain quorum: which roles count toward stand-down
  sign-off for which incident classes (see section 8).

---

## 1. Triage

When a P0 / P1 alert reaches the Foundation:

1. **Confirm the alert is not noise** by cross-referencing the affected
   protocol team. Foundation is not the first responder on a single-pool
   or single-vault incident — the dev team
   ([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)) or
   the pool admin ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md))
   is. Foundation joins when:
   - The incident is cross-protocol (touches a bridge, a stablecoin, or a
     validator-set issue), or
   - The incident has visible ecosystem-level impact (peg deviation,
     systemic oracle failure, exploit on a contract used by multiple
     curators), or
   - A protocol team requests Foundation's help.
2. **Classify** using the table in
   [`README.md`](./README.md#hypernative-alert-taxonomy). Foundation
   classification is *the* canonical severity once Foundation has joined
   the war room — protocols can disagree internally but external comms
   should match Foundation's class.
3. **Decide which Safe Chain roles to convene.** Use the dependency map
   below.

### Dependency map

| Incident family | Convene |
|-----------------|---------|
| Protocol hack on Blend pool / backstop | Blend dev team (lead), affected pool admin(s), curator vaults supplying into affected reserves, Foundation (coordination), validators (informational) |
| Protocol hack on a curator vault | Curator (lead), vault sentinel, vault dev team, Foundation (coordination), affected Blend pool admin(s) (informational) |
| Bad debt | Blend pool admin (lead), Blend dev team, curator vaults supplying into the affected reserve, Foundation (informational) |
| Faulty oracle | Oracle provider (lead), Blend dev team (technical), pool admins (containment), curator vaults (deallocate), Foundation (oracle-provider broker) |
| Pool admin compromise | Pool admin (lead), Blend dev team, Foundation (lead on cross-stack), validators if Stellar-Safe-Chain-level mitigation is needed, exchanges if the captured admin is moving funds, stablecoin issuers if frozen-asset action might be requested |
| Curator / allocator / sentinel compromise | Curator's governance (lead), curator's sentinel, vault dev team, Foundation, affected Blend pool admin(s) |
| Bridge incident | Bridge operator (lead), Foundation (lead on ecosystem comms), Blend pool admins whose pools list bridged assets, curator vaults exposed to bridged assets, stablecoin issuers if a bridged stablecoin is involved |
| Stablecoin incident | Stablecoin issuer (lead), Foundation, Blend pool admins listing the stablecoin, curator vaults holding the stablecoin |
| Validator misbehaviour | Validator (lead), Foundation (lead on Stellar-network response), all of the above (informational) |

---

## 2. Protocol hack

When the alert family is "exploit / invariant break" on Blend or on a
curator vault, Foundation's job is to coordinate, not to act on-chain.

### 2.1 First fifteen minutes

1. **Confirm war room** is open and that the lead role for the incident
   class has joined.
2. **Confirm the smallest containment** has been taken. For Blend pool
   exploits: has `set_status(4)` been broadcast? For curator vault
   exploits: has the sentinel called `submit_set_paused(true)` (see
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md))?
   If not, ask why.
3. **Identify the blast radius.** Which other curators are exposed to the
   same pool / asset / oracle? Foundation's contact directory is the
   authoritative source of truth here.
4. **Notify adjacents.** Bridges holding the affected asset, stablecoin
   issuers if a stablecoin reserve is involved, exchanges that list the
   token. Pre-incident comms templates apply (section 7).

### 2.2 Cross-stack mitigations Foundation can request

Foundation does not have unilateral on-chain authority, but can request:

- **Stablecoin freeze of attacker addresses.** If the attacker is moving
  USDC / EURC / etc., the issuer can freeze those addresses under their
  asset authority (see [`06-stablecoin-operators.md`](./06-stablecoin-operators.md)).
  Foundation makes the request; the issuer decides on policy and law
  grounds.
- **Bridge halt.** If the attacker is bridging out, the bridge operator can
  pause the bridge or block specific addresses (see
  [`05-bridge-operators.md`](./05-bridge-operators.md)).
- **Validator outreach.** Foundation maintains validator relationships and
  can ask validators to be aware of unusual transaction patterns and to
  follow [`07-stellar-validators.md`](./07-stellar-validators.md).
- **Network upgrade coordination.** In an extreme case, an upgrade can be
  coordinated to address protocol-level issues. This is rare, slow, and
  only contemplated for the worst-case ecosystem incidents (e.g. a
  consensus-layer issue, not an application-layer hack).

### 2.3 Recovery

Foundation's role in recovery is to:

1. **Verify migration plan** with the lead role.
2. **Coordinate communication windows** so that pool migrations,
   stablecoin freeze releases, bridge re-opens, and exchange listings
   align in time. A staggered uncoordinated stand-down is itself a risk.
3. **Sign off** on stand-down per the Safe Chain quorum.

---

## 3. Bad debt

Foundation is *not* a primary actor on a bad-debt incident — that is
contained within a single pool. Foundation joins if:

- Bad debt is large enough to threaten a curator vault's solvency, in which
  case curator depositors may need clear public guidance, or
- The bad debt has produced a peg / liquidity event that is now visible at
  the ecosystem level (e.g. one pool's reserve becoming the marginal
  borrow venue for a bridge, and the bridge inheriting the price impact).

In those cases, Foundation coordinates a single ecosystem-level statement
and ensures that curators and stablecoin issuers are aware so they can
evaluate their own exposure independently.

---

## 4. Faulty oracle

Faulty oracles are a class where Foundation has unique value: oracle
providers (Pyth, Reflector, Redstone, SEP-40 publishers, etc.) typically
maintain Foundation as a primary point-of-contact across protocols.

### 4.1 Containment workflow

1. **Verify the oracle issue** with the provider. Foundation paged the
   provider's incident contact; provider confirms or denies.
2. **Broadcast across affected protocols.** Every Blend pool whose oracle
   uses the affected feed needs to know within minutes. So does every
   curator vault whose policy depends on the feed (see
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)).
3. **Coordinate timing of pool freezes** if multiple pools all wired to
   the same feed need to step status simultaneously. Avoid having pool A
   freeze while pool B is still active and the price drift is causing
   migration risk.

### 4.2 Recovery

1. Provider publishes all-clear; Foundation verifies and re-broadcasts.
2. Pool admins step status back per
   [`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 4.2.
3. Foundation publishes a single ecosystem post-mortem for the oracle
   incident, naming each affected protocol and its containment timeline.

---

## 5. Pool admin compromise

Foundation's most distinctive role. A captured pool admin is largely
irreversible from on-chain alone; Foundation's coordination is what
limits damage.

### 5.1 Containment workflow

1. **Confirm the admin is captured** with the protocol team and the pool
   admin's emergency contact (which must be different from the captured
   key; if the same person is the only key holder *and* the only
   contact, the standing-posture process has failed and this should be a
   post-incident corrective). If the admin is partially captured (one
   signer of a multisig), the multisig itself may still be safe — verify
   with the admin's full signing roster.
2. **Coordinate the cross-stack defence.** A captured admin can:
   - Reconfigure reserves to drain liquidity.
   - Move emissions.
   - Hand the admin role to another address.
   The defences Foundation can broker:
   - Stablecoin issuers can freeze the captured admin's address(es) and
     freeze any address proposed via `propose_admin` if those addresses
     are attacker-controlled and the asset is one the issuer controls.
   - Bridges can refuse to bridge funds from the captured admin's
     address(es).
   - Exchanges can flag deposits from the captured admin.
   - Validators can be made aware (informational) and follow
     [`07-stellar-validators.md`](./07-stellar-validators.md). The
     Stellar consensus / sequencing layer is *not* a censorship surface;
     validators do not censor specific transactions on request.
3. **Coordinate user comms.** Curator vaults supplying into the affected
   pool should be paused / deallocated on their own initiative, not via
   any Foundation directive. Foundation's job is to make sure they have
   the information.

### 5.2 Recovery

1. The compromised pool is functionally lost as long as the attacker holds
   admin. Recovery is a fresh-pool migration coordinated by the dev team
   ([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)
   section 2.3 / 5).
2. Foundation sign-off is required on stand-down (Safe Chain rule). The
   sign-off conditions: the compromised pool is publicly marked
   unsafe, the migration pool is live and audited, no further
   in-progress attacker-driven configuration changes are observed.

---

## 6. Curator / allocator / sentinel compromise

Curator vaults are a category where Foundation's role is *informational and
coordinative*. The vault's own governance, sentinel, and curator are the
on-chain actors. Foundation:

- Confirms the compromise is real (cross-checks with the vault operator's
  emergency contact).
- Notifies adjacent stakeholders — Blend pool admins whose pools the vault
  supplies into, other curators competing for the same liquidity (so they
  can model knock-on rate effects), bridges and stablecoin issuers whose
  assets are in the vault.
- Brokers stablecoin / bridge defences only if the captured key is
  actively moving stablecoin or bridged funds.
- Signs off on stand-down (Safe Chain quorum).

---

## 7. Communication protocol

Foundation communications during an incident follow these rules:

- **One voice.** No-one inside Foundation comms publishes about the
  incident except the war-room comms lead (or their designate).
- **No attribution.** Until forensics is complete, no attribution
  statements. "We are aware of an incident" is the maximum claim.
- **Coordinate timing with other roles.** A Foundation tweet that
  contradicts a curator's pause notice undermines both. Comms leads
  across the war-room roles co-author the public statement.
- **Three statement classes:**
  1. **Awareness**: "Foundation is aware of an incident on <protocol /
     pool / asset>. The teams involved are coordinating. We will share
     more in <interval>." Posted within 30 minutes of P0 detection.
  2. **Containment**: "Containment action <X> has been taken by
     <role>. <User-facing impact>. No further user action required" /
     or specific user instructions if there are any.
  3. **Stand-down**: "The incident is resolved. <Brief root cause>. A
     full post-mortem will be published within <interval>."
- **Post-mortem within 14 days** for any P0/P1, jointly authored with the
  lead role for the incident.

---

## 8. Safe Chain quorum

Stand-down for any P0/P1 requires sign-off from the lead role *and* at
least one further Safe Chain role. Foundation maintains the quorum
definition:

| Incident family | Lead | Required additional sign-off |
|-----------------|------|-------------------------------|
| Protocol hack on Blend | Blend dev team | Affected pool admin OR Foundation |
| Protocol hack on a vault | Curator | Foundation OR Blend dev team |
| Bad debt | Pool admin | Blend dev team |
| Faulty oracle | Oracle provider (off-Safe-Chain) | Blend dev team AND Foundation |
| Pool admin compromise (admin still controlled) | Pool admin | Blend dev team AND Foundation |
| Pool admin compromise (admin captured) | Foundation | Blend dev team AND an independent second role (e.g. an unaffected pool admin, a major curator, or an external auditor) |
| Curator / allocator / sentinel compromise | Curator governance | Foundation |
| Bridge incident | Bridge operator | Foundation AND any affected stablecoin issuer |
| Stablecoin incident | Stablecoin issuer | Foundation |
| Validator misbehaviour | Foundation | Validator operator(s) |

Sign-off is recorded in the war-room log with the signer name, time, and
a one-line statement of what they are signing off on.

---

## 9. Stand-down checklist

- [ ] Lead role has declared incident contained.
- [ ] Required additional Safe Chain sign-off recorded in war-room log.
- [ ] All affected parties (per dependency map in section 1) notified of
      stand-down.
- [ ] Public stand-down statement published.
- [ ] Post-mortem owner assigned with a 14-day deadline.
- [ ] War room archived (append-only log preserved).
- [ ] Standing posture (contact directory, monitoring subscriptions,
      comms templates) reviewed for changes the incident exposed.
