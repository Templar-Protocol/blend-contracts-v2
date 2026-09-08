# Runbook: NEAR Intents Team

**Audience.** Operators of the NEAR Intents stack — the canonical
intents contract on NEAR, solvers competing to fill user intents,
settlement authorities, and off-chain infrastructure that routes and
attests cross-chain flow. Templar Protocol is exposed to NEAR Intents
as a liquidity source (Templar markets and vaults can be routes for
intent settlement) and as a settlement counterparty (intent-based
deposits / withdrawals into Templar vaults on NEAR).

**Scope.** You do not call any Templar market or vault function
directly. Your levers are on the intents surface: pause the intents
router or specific solver, revoke a settlement authority, throttle
or denylist a signer / solver, adjust routing to avoid an affected
Templar market or vault. You participate in the NEAR Safe Chain so
that intent-mediated flow does not weaponise a Templar incident, and
so that intents-layer incidents are coordinated with Templar
responders.

This runbook is distinct from the Blend collection because the
Templar NEAR context is exposed to the intents layer in a way the
Stellar / Blend context is not.

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

Before any incident, the NEAR Intents team must keep the following
in a known-good state:

- A documented per-route list of which Templar markets and vaults an
  intent may settle through, and which curator vaults hold material
  exposure to intent-routed flow.
- An incident contact directory shared with the Templar dev team
  ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md)),
  the registry admin
  ([`05-templar-registry-admin.md`](./05-templar-registry-admin.md)),
  every Templar vault curator supplying into intent-routable markets
  ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)),
  NEAR Foundation
  ([`03-near-foundation.md`](./03-near-foundation.md)), bridge
  operators ([`06-bridge-operators.md`](./06-bridge-operators.md)),
  and stablecoin issuers
  ([`07-stablecoin-operators.md`](./07-stablecoin-operators.md)).
- Hypernative or equivalent monitoring on: intents-contract
  privileged calls, solver bid / fill patterns, settlement authority
  signatures, per-solver flow throttles, per-route inflow / outflow
  anomalies, cross-chain attestation queue depth.
- A documented signing posture for the intents contract's privileged
  authorities and for solver operator keys.
- A pre-shared severity classification matching the Hypernative
  taxonomy in [`README.md`](./README.md).

---

## 1. Triage

When an alert fires that implicates a Templar market / vault, or
implicates the intents layer directly:

1. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   NEAR Intents team is *lead* for any incident classified as
   intents-layer compromise; *technical advisor* for protocol hack /
   curator compromise / bridge / stablecoin incidents that might be
   exfiltrating funds through intent routes.
2. **Snapshot state**: current intents-contract configuration,
   authorised solver set, recent per-solver flow, per-route
   inflow / outflow.
3. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
4. **Pick the smallest viable mitigation.** Denylisting a single
   solver is far less invasive than pausing the intents contract; a
   per-route throttle is less invasive than a full pause.

---

## 2. Protocol hack on Templar

Most Templar exploits target extraction of value in some asset. If
the exploiter uses intent-mediated flow to move funds — inbound
(depositing into a compromised vault to obtain shares at a bad
price) or outbound (converting extracted collateral into a
bridgeable stablecoin via a routed intent) — the intents surface is
the choke point.

### 2.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | NEAR Safe Chain pages you with a confirmed exploiter address routing intents that touch Templar. |
| **High (P1)** | Anomalous flow of a Templar-listed asset via a specific intent route or solver, with corresponding war-room awareness. |
| **Medium (P2)** | NEAR Foundation pages you about an in-progress Templar incident; no specific intents action requested yet. |

### 2.2 Containment

1. **Confirm war-room context.** Do not act on a single Hypernative
   signal. Denylisting a legitimate solver during an incident causes
   secondary damage.
2. **Per-address denylist / throttle**. If exploiter addresses are
   identified, add them to the intents-contract denylist (or set
   per-address throttles to zero) so intent-mediated flow stops for
   those specific addresses. This preserves legitimate user access.
3. **Per-route mitigation**. If a specific intent route into or out
   of an affected Templar market is the exit path, pause that route
   without pausing others.
4. **Per-solver mitigation**. If a solver is complicit or
   compromised, revoke that solver's authorization on the
   intents contract.
5. **Full-contract pause** — only if per-address / per-route /
   per-solver mitigation is insufficient. Pre-warn NEAR Foundation
   and the Templar dev team so cross-stack comms is consistent.
6. Coordinate with stablecoin issuers
   ([`07-stablecoin-operators.md`](./07-stablecoin-operators.md)) if
   the exploit exit converts into an authority-controlled
   stablecoin. Coordinate with bridge operators
   ([`06-bridge-operators.md`](./06-bridge-operators.md)) if the
   intent route ultimately bridges out.

### 2.3 Recovery

1. Lift restrictions only after NEAR Foundation Safe Chain confirms
   the Templar incident is contained
   ([`03-near-foundation.md`](./03-near-foundation.md) §8).
2. Lift in the reverse order: per-address denylist last (leave
   compliance-driven entries in place per your standing policy),
   per-solver revocations reviewed individually, per-route pauses
   next, full-contract pause first.
3. Publish an intents-side post-mortem co-authored with Templar dev
   team if the intents layer was meaningfully involved.

---

## 3. Bad debt on Templar

Bad debt on a Templar market where intent-mediated flow is
significant becomes an intents-layer concern because arbitrageurs
may attempt to route intents opportunistically as the loss is being
recognised. Your role is to:

1. Confirm with the Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md))
   and affected vault curators
   ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md))
   that loss recognition is in flight.
2. Consider a temporary per-route throttle on intent routes that
   settle through the affected market, so intent execution during
   the loss-recognition window does not compound the impact.
3. Communicate to solvers so they can adjust their routing
   independently.

---

## 4. Faulty oracle

If a Templar market that participates in intent routing is pricing
via a faulty oracle, intents that quote through that market will be
mispriced.

1. Coordinate with the Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §4)
   and NEAR Foundation
   ([`03-near-foundation.md`](./03-near-foundation.md) §4).
2. Per-route throttle or pause on intents that settle through the
   affected market until the oracle all-clear.
3. Solvers may already be avoiding the mispriced venue as an
   arbitrage-avoidance measure; communicate the situation so their
   avoidance is deliberate rather than reactive.

---

## 5. Registry admin compromise

If the Templar registry admin is compromised, intents that settle
through a market may be routing through a market whose provenance
is now in question (the compromised admin could deploy fake
markets that look official). Your role:

1. Coordinate with the Templar dev team and NEAR Foundation to
   determine which market deployments are trusted vs suspect.
2. Update solver routing tables to exclude any newly deployed or
   updated markets pending Safe Chain sign-off.
3. Communicate the interim routing policy to solvers.

---

## 6. Curator / allocator / sentinel compromise

A captured Templar vault role can produce anomalous vault behavior
that intents may route through unwittingly. Coordinate with the
affected curator
([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md))
and NEAR Foundation. If the compromise materially changes the
vault's economic behavior (e.g. share price discontinuity, cap
saturation manipulation), pause or throttle intent routes that
settle into that vault.

---

## 7. Intents-layer internal incidents

When the intents layer itself is the incident source, the NEAR
Intents team is *lead* and the rest of the Safe Chain participates
as informational stakeholders.

| Class | Signal | Containment |
|-------|--------|-------------|
| **Critical (P0)** | Intents contract exploit; settlement authority key capture; solver-set-wide collusion; cross-chain attestation forgery detected. | Pause the intents contract end-to-end. Notify NEAR Foundation immediately; Foundation will broker comms with Templar dev team, curator operators, bridge operators, and stablecoin issuers whose flow touches your intent routes. Expect vault curators to Sentinel-pause their vaults and Templar dev team to halt bots defensively. |
| **High (P1)** | Single-solver malfunction; single-route reserve mismatch; attestation queue depth spike; per-solver flow pattern deviates sharply from baseline. | Revoke or throttle the affected solver / route. Coordinate with NEAR Foundation and downstream counterparties. |
| **Medium (P2)** | Operator key rotation request out-of-band; signer hardware degraded; single-off-chain relayer outage. | Investigate and document; adjust throttle if warranted. |

In an intents-internal P0, expect Templar vault curators to pause
vaults that rely on intent flow, and expect bridge operators to
throttle bridged assets that were routed via NEAR Intents.
Coordinate the timing of stand-down across the war room.

---

## 8. Communication protocol

- Intents-side communications follow the same three checkpoints
  (awareness, containment, stand-down) as NEAR Foundation, on the
  same schedule.
- Coordinate any user-facing messaging with NEAR Foundation and
  the Templar dev team; conflicting messages amplify panic.
- For an intents-internal P0, the NEAR Intents team publishes
  first, with Foundation coordination on cross-protocol
  implications.
- Post-mortem within 14 days for any P0 / P1, jointly with NEAR
  Foundation if cross-protocol.

---

## 9. Stand-down checklist

- [ ] All intents contract pauses lifted.
- [ ] All per-address denylist entries added during the incident
      reviewed; permanent entries documented separately.
- [ ] Per-route and per-solver throttles returned to baseline
      (compliance-driven entries per standing policy).
- [ ] Settlement authority key custody verified after any incident
      that involved authority-level suspicion.
- [ ] NEAR Foundation contact informed of stand-down.
- [ ] All affected Templar vault curators and dev team informed of
      stand-down.
- [ ] Public stand-down statement published, coordinated with NEAR
      Foundation and Templar dev team.
- [ ] War room archived (append-only log preserved).
