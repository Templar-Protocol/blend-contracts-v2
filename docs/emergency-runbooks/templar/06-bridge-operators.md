# Runbook: Bridge Operators

**Audience.** Operators of bridges that move assets between NEAR and
other chains (Rainbow Bridge, OmniBridge, deBridge, Wormhole, etc.),
especially those whose bridged assets are listed on Templar markets
or held by Templar curator vaults on NEAR.

**Scope.** Cross-chain inflow / outflow of assets that touch Templar
liquidity. You do not call any Templar contract function. Your levers
are at the bridge: pause the bridge, freeze specific addresses on the
bridge's allowlist / denylist, throttle limits, and adjust which
assets are relayable. You participate in the NEAR Safe Chain to stop
value from exiting the ecosystem during an in-flight incident.

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

Before any incident, the bridge operator must keep the following in
a known-good state:

- A documented per-asset list of which Templar markets list the
  asset as a reserve, and which curator vaults hold material
  exposure to that asset.
- An incident contact directory shared with NEAR Foundation
  ([`03-near-foundation.md`](./03-near-foundation.md)), the Templar
  dev team
  ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md)),
  every curator vault
  ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)),
  and the NEAR Intents team
  ([`02-near-intents-team.md`](./02-near-intents-team.md)) if
  intents may route through your bridge.
- Hypernative or equivalent monitoring on: bridge pause / unpause,
  signer-set changes, large outflow events (per-asset thresholds),
  per-address throttle hits, mint / burn discrepancies on either
  chain.
- A documented signing posture (HSM, multi-sig, threshold) for the
  bridge's privileged contracts.
- A pre-shared severity classification matching the Hypernative
  taxonomy in [`README.md`](./README.md).

---

## 1. Triage

When an alert fires that implicates a bridged asset on Templar:

1. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   Bridge operator is *lead* for any incident classified as bridge
   incident; *technical advisor* for protocol hack / curator
   compromise that might be exfiltrating funds via the bridge.
2. **Snapshot state**: bridge pause flag, signer set, recent inflow
   / outflow per asset, current mint vs. backing balance.
3. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
4. **Pick the smallest viable mitigation.** Bridges are usually the
   *exit* surface in a Templar incident; delaying an outflow rarely
   costs users, while a wrong outflow can be irreversible.

---

## 2. Protocol hack on Templar

You are not the protocol responder, but you may be the *first* to
see the exit. The attacker's path out of the ecosystem usually goes
through your bridge or through an intent route that ultimately
bridges.

### 2.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Large outflow request from an address known to be the exploiter (cross-referenced with the war room). |
| **High (P1)** | Per-asset outflow that exceeds normal thresholds, originating from an unusual address. |
| **Medium (P2)** | NEAR Foundation pages you about an in-progress Templar incident; no specific bridge action requested yet. |

### 2.2 Containment

1. **Confirm the war-room context.** A bridge halt unrelated to
   the actual incident causes wider damage. Do not act on a single
   Hypernative signal without cross-referencing.
2. **If specific addresses are identified**, add them to the
   bridge denylist or throttle them to zero. This is preferable to
   a full pause because legitimate users can continue to exit.
3. **If addresses are not yet identified** but the outflow pattern
   is clearly anomalous, pause the affected asset's exit direction
   from NEAR first (transfers *from NEAR* to any destination chain),
   entry direction second (transfers *to NEAR* from other chains).
   Terminology: "outbound" here means NEAR-as-source, and "inbound"
   means NEAR-as-destination — confirm this convention matches your
   bridge's own control-plane naming before invoking pauses, since
   some bridge stacks label directions relative to the destination
   chain instead. Pausing NEAR-as-destination inflow can strand
   legitimate liquidity that was already committed on the source
   chain, so it is the more disruptive action.
4. **If the bridge cannot perform per-asset or per-address
   mitigation**, pause the bridge. Pre-warn NEAR Foundation so the
   cross-stack communication is consistent.
5. Coordinate with stablecoin issuers
   ([`07-stablecoin-operators.md`](./07-stablecoin-operators.md))
   if the bridged asset is a stablecoin — issuer-level freeze via
   asset authority is often more precise than a bridge-level pause.
6. Coordinate with the NEAR Intents team
   ([`02-near-intents-team.md`](./02-near-intents-team.md)) if
   intent routes may be relaying the exit flow through your
   bridge.

### 2.3 Recovery

1. Lift restrictions only after NEAR Foundation Safe Chain
   confirms the Templar incident is contained
   ([`03-near-foundation.md`](./03-near-foundation.md) §8).
2. Lift in the reverse order: per-address denylist last,
   per-asset throttles next, full bridge pause first.
3. Publish a bridge-side post-mortem co-authored with the Templar
   dev team if the bridge was meaningfully involved in the
   incident.

---

## 3. Bad debt

Bad debt on a Templar market is not directly a bridge incident. It
becomes one when the bad-debt asset is bridged, because the loss can
prompt arbitrageurs to bridge in / out faster than usual and the
bridge needs to have the liquidity and price posture to handle the
spike.

1. Confirm with the affected vault curator and Templar dev team
   ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md) §3 /
   [`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §3)
   that loss recognition is in flight.
2. Adjust per-asset throttle on the bridge if you anticipate a
   brief spike. If your bridge supports emergency fee adjustments
   under pre-authorized bounds (e.g. a rate-limit-preserving surge
   fee configured by the operator's standing policy), consider
   using them; otherwise leave fee parameters unchanged during the
   incident.
3. Communicate to your users so they understand any short-term
   latency.

---

## 4. Faulty oracle

Bridges that rely on NEAR-side oracles for valuation (e.g. fee
calculations, collateralization checks) inherit oracle-fault risk.

1. If the bridge's own pricing depends on the affected feed, pause
   pricing-dependent flows first.
2. Coordinate with the Templar dev team
   ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §4)
   and NEAR Foundation
   ([`03-near-foundation.md`](./03-near-foundation.md) §4) so
   cross-protocol containment is consistent.
3. Lift only after the oracle provider's all-clear and stand-down.

---

## 5. Registry admin compromise

A captured Templar registry admin can `deploy` fake markets and
`add_version` malicious code. If such a fake market appears in your
routing tables or as a bridge integration target, refuse to bridge
into it until the compromise is contained.

1. **Primary control: gate on the Templar market manifest.** Refuse
   to integrate with (bridge into, list in routing tables, expose
   in the front end) any newly deployed market whose `version_key`
   / code hash is not on the Templar-published manifest, pending
   Safe Chain sign-off. A captured registry admin can deploy fake
   markets under attacker-controlled contract accounts; the bridge's
   own allowlist of trusted market integrations, not any address
   denylist, is what prevents flow into those.
2. **Throttle large outflows** of assets that are listed on any
   suspect market for the duration of the war room.
3. **Address denylist** is a secondary tool and only applies where
   the captured admin's own account address is a bridge sender or
   recipient and your bridge supports per-address filtering.
   Denylisting the registry admin's account does not block a fake
   market deployed under a *different* attacker-controlled account,
   which is why the manifest allowlist is primary.
4. Coordinate with stablecoin issuers if the captured admin
   deploys markets that could be used to convert stablecoins into
   exit assets.

---

## 6. Curator / allocator / sentinel compromise

A captured vault role can drain vault assets that ultimately exit
via the bridge. The bridge's role is as an *exit gate*.

1. Coordinate with the affected curator
   ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md))
   and NEAR Foundation Safe Chain
   ([`03-near-foundation.md`](./03-near-foundation.md) §6).
2. Apply per-address denylist on the captured key's address(es)
   if provided.
3. Throttle large outflows of the affected vault's asset(s).
4. Be aware that curator-vault depositors may legitimately want to
   exit the vault and bridge out — the goal is to stop the *captured
   key* from exiting, not depositors reacting to the news.

---

## 7. Bridge-internal incidents

Although this runbook is centred on Templar, the bridge itself can
be the incident source. When that happens, the bridge operator is
*lead* and the rest of the Safe Chain participates as informational
stakeholders.

| Class | Signal | Containment |
|-------|--------|-------------|
| **Critical (P0)** | Bridge insolvency (mint exceeds backing); bridge contract exploit; signer-set capture. | Pause the bridge end-to-end. Notify NEAR Foundation immediately; Foundation will broker comms with Templar dev team, vault curators holding the bridged asset, stablecoin issuers, and the NEAR Intents team. Vault curators may Sentinel-pause vaults holding the bridged asset before it re-prices. |
| **High (P1)** | Single-asset reserve mismatch; off-chain relayer outage; large unattributed outflow; rate-limit anomaly. | Pause the affected asset only. Coordinate with NEAR Foundation and Templar dev team. |
| **Medium (P2)** | Operator key rotation request out-of-band; signer hardware degraded; relayer queue depth spike. | Investigate and document; adjust throttle if warranted. |

In a bridge-internal P0, expect Templar vault curators to
Sentinel-pause vaults holding the bridged asset, the Templar dev
team to halt bots, and NEAR Intents team to reroute or pause
intents that use the bridge. Coordinate the timing of stand-down.

---

## 8. Communication protocol

- Bridge-side communications follow the same three checkpoints
  (awareness, containment, stand-down) as NEAR Foundation, on the
  same schedule.
- Coordinate any user-facing messaging with NEAR Foundation;
  conflicting bridge-side and protocol-side messages amplify
  panic.
- For a bridge-internal P0, the bridge operator publishes first,
  with Foundation coordination on cross-protocol implications.
- Post-mortem within 14 days for any P0 / P1, jointly with NEAR
  Foundation if cross-protocol.

---

## 9. Stand-down checklist

- [ ] All bridge pauses lifted.
- [ ] All per-address denylist entries added during the incident
      reviewed; permanent entries documented separately.
- [ ] Per-asset throttles returned to baseline.
- [ ] Bridge mint vs. backing reconciled and verified by an
      independent operator if the incident affected backing.
- [ ] NEAR Foundation contact informed of stand-down.
- [ ] All affected Templar vault curators, dev team, and NEAR
      Intents team informed of stand-down.
- [ ] Public stand-down statement published.
- [ ] War room archived (append-only log preserved).
