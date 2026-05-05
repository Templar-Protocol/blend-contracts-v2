# Runbook: Stellar Validators

**Audience.** Operators of Stellar Core validators, especially those in
the network's tier-1 quorum slices and those that host RPC / Horizon
endpoints used by Blend protocol services, curator vaults, bridges, and
stablecoin operators.

**Scope.** Validator-level participation in the Stellar Safe Chain.
Validators do not, and should not, censor specific transactions on
request — Stellar's consensus and sequencing properties are not levers
that protocol incident response can pull. Validators *are* the network's
liveness and observability backbone, and their role in incident response
is operational integrity, transparent communication, and (in the most
extreme cases) network-upgrade coordination.

Read [`README.md`](./README.md) first for the severity matrix, Hypernative
classification, war room template, and Safe Chain coordination model.

---

## 0. Standing posture

Before any incident, the validator operator must keep the following in a
known-good state:

- A documented signing posture for validator keys (HSM, multisig where
  applicable, rotation cadence).
- Independent monitoring of validator health: ledger close time,
  externalize messages, quorum participation, RPC/Horizon latency
  and error rate.
- An incident contact directory shared with the Stellar Foundation
  ([`03-stellar-foundation.md`](./03-stellar-foundation.md)) and other
  validator operators. The Foundation maintains the canonical list.
- A pre-shared upgrade-coordination process. Validators must be able
  to coordinate a protocol-level upgrade if one is requested by the
  Foundation in response to a consensus- or transport-layer issue. An
  application-layer (Soroban contract) issue is *not* an upgrade
  trigger.
- A documented public-comms standard. Validator-level statements during
  an incident are typically informational; you do not speak on behalf
  of any application protocol.

---

## 1. Triage

When an alert reaches you that may be related to an ecosystem incident:

1. **Determine whether the incident is at the network layer** (your
   surface) or **the application layer** (Blend / curator vaults /
   bridges / stablecoins). Application-layer incidents do not require
   any validator action beyond keeping operations healthy and
   participating in the war room as informational stakeholders.
2. **Open or join the war room** ([`README.md`](./README.md#war-room-template)).
   Validator operators are *lead* only for validator misbehaviour
   incidents; otherwise informational.
3. **Snapshot validator health**: latest closed ledger, externalize
   participation, peer count, RPC / Horizon health.
4. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).

---

## 2. Protocol hack on Blend or curator vaults

Validators have **no on-chain action** to take in response to an
application-layer hack. Stellar consensus does not provide a
transaction-censorship surface, and validators must not invent one. The
correct posture is:

1. **Maintain liveness.** A flood of attacker / responder traffic on the
   network can stress validator capacity. Make sure your validator and
   any RPC / Horizon endpoints you operate stay responsive — incident
   response by the Blend dev team
   ([`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)),
   pool admins
   ([`02-blend-pool-admins.md`](./02-blend-pool-admins.md)), curators
   ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)),
   bridges ([`05-bridge-operators.md`](./05-bridge-operators.md)), and
   stablecoin operators
   ([`06-stablecoin-operators.md`](./06-stablecoin-operators.md))
   depends on the network being usable.
2. **Be transparent about your posture.** If you operate a public RPC /
   Horizon endpoint and you apply per-IP rate limits during a network
   load spike, communicate that.
3. **Forward observability.** If your monitoring spots Soroban call
   patterns that match the war-room hypothesis (e.g. unusual entry
   points, large invoke-host-function calls against known affected
   contracts), share them with the war room. You are well-placed to
   see network-level trace data that application teams may not.
4. **Refuse out-of-band requests** to censor specific transactions or
   addresses. Any such request must come through Foundation Safe Chain
   and be addressed at the application layer (issuer freeze,
   bridge denylist, pool admin freeze) — not at the consensus layer.

---

## 3. Bad debt

No validator action.

---

## 4. Faulty oracle

No validator action. Note that some validators are also oracle
publishers (e.g. operating Pyth publishers, Reflector publishers, or
SEP-40 publishing oracles). When you wear that hat, you are an oracle
provider for the purposes of the Faulty oracle sections in
[`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md) and
[`02-blend-pool-admins.md`](./02-blend-pool-admins.md), and the Stellar
Foundation will treat you as the lead role on the oracle-provider side
([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 4).

---

## 5. Pool admin / curator / allocator / sentinel compromise

Same as protocol hack above. No consensus-layer action; no censorship of
the captured key on the validator side. Application-layer mitigations
(stablecoin freeze, bridge denylist, pool admin freeze, vault sentinel
pause) are the right tools. Validators are informational stakeholders.

---

## 6. Bridge / stablecoin compromise

Same posture: maintain liveness, do not censor, share network-level
observability with the war room.

---

## 7. Validator-internal incidents

When the incident is at the validator / network layer, the validator
operator(s) involved are *lead* and Foundation coordinates.

| Class | Signal | Containment |
|-------|--------|-------------|
| **Critical (P0)** | Validator key capture; quorum slice instability; consensus-layer bug producing inconsistent ledgers; large RPC / Horizon endpoint outage cascading across providers. | Coordinate with Foundation immediately. Quorum-slice changes and key rotations are slow; pre-arranged emergency upgrade procedures may apply. Application protocols that depend on Stellar liveness should be told to expect extended interruption and to make user-facing decisions accordingly. |
| **High (P1)** | Single-validator misbehaviour (invalid message, fork attempt detected by peers); single-large-RPC outage; transport-layer DoS targeting validators. | Coordinate with Foundation. Misbehaving validator may be removed from quorum slices. Application protocols may need to step pool / vault status to safer postures while the network re-stabilises. |
| **Medium (P2)** | Single validator slow to externalize; one RPC operator under maintenance; routine signer hardware refresh. | Document; no broad escalation unless it affects ecosystem. |

In a validator-internal P0, application protocols *will* react: pool
admins may move pools to status 4 (admin frozen) defensively, curator
sentinels may pause vaults, bridges may halt. That is expected and
correct, and validators should help by giving those teams a clear
estimate of stand-down timing.

### 7.1 Network upgrade coordination

A network upgrade is contemplated only for consensus- or transport-layer
issues that cannot be addressed at the application layer. The
Foundation coordinates the upgrade vote and timing. Validators
participating in an upgrade must:

1. Verify the upgrade artifact independently (reproducible build,
   matching the published commit).
2. Stage the upgrade in their non-production validators first.
3. Coordinate the activation ledger with Foundation and other
   validators.
4. Communicate the upgrade window publicly so application protocols
   (Blend pools, curator vaults, bridges, stablecoin operators) can
   adjust their status / pause posture accordingly during the window.

A network upgrade in response to an *application-layer* incident is
explicitly out of scope. Compromised application contracts must be
recovered via application-layer migration (per
[`01-blend-protocol-dev-team.md`](./01-blend-protocol-dev-team.md)
section 2.3 / 5).

---

## 8. Communication protocol

- Validator-level statements are typically informational ("our
  validator is healthy", "we are aware of an ecosystem incident
  affecting <protocol>", "RPC endpoint X is degraded").
- Do not speak on behalf of any application protocol.
- Do not attribute or speculate.
- Coordinate any public statements about validator-level posture with
  the Foundation.

---

## 9. Stand-down checklist

- [ ] Validator and any RPC / Horizon endpoints operating at baseline.
- [ ] Foundation contact informed of stand-down (for validator-internal
      incidents).
- [ ] Network-level observability shared with the war room is
      finalised in the war-room log.
- [ ] No out-of-band censorship requests were honoured during the
      incident; the war-room log records any such requests received
      and the response.
- [ ] War room archived (append-only log preserved).
