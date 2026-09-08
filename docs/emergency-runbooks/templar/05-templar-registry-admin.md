# Runbook: Templar Registry Admin

**Audience.** The keyholder(s) of the Templar registry contract owner
account (see [`contract/registry`](https://github.com/Templar-Protocol/contracts/tree/dev/contract/registry) and
[`docs/src/governance.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/governance.md)). You are the *only*
party that can call `add_version` (publish new contract code),
`remove_version` (delist a code version), `deploy` (spawn a new
instance of a version — new market, new vault, new oracle adapter),
and `upgrade` (upgrade the registry contract itself). Templar markets
are immutable and have no per-market admin, so the registry admin is
the single privileged NEAR-side surface for the protocol.

**Scope.** You administer the registry singleton. Your authority is
narrow but consequential: a compromised registry admin can publish
malicious code, deploy fake markets that look official, or delist
legitimate versions.

Read [`README.md`](./README.md) first for the severity matrix,
Hypernative classification, war room template, and NEAR Safe Chain
coordination model.

---

## 0. Standing posture

Before any incident, the registry admin must keep the following in a
known-good state:

- A documented, hardware-backed signing flow for every registry
  admin action. On NEAR, this typically means an HSM-backed
  full-access key on the registry owner account, with any function-
  call access keys strictly scoped and short-lived. Multi-sig via a
  contract-controlled owner is strongly recommended.
- A second, *independent* signing flow ready to broadcast a
  privileged action if the primary flow is degraded — for example,
  a fallback for adding an emergency-patched version quickly during
  an incident.
- An off-chain, publicly verifiable manifest of every code version
  published: `version_key` → `code_hash`, build source, audit
  report link. This is what integrators verify against
  `get_version_code_hash` when confirming a deployment's
  provenance.
- Hypernative or equivalent monitoring on: `add_version`,
  `add_version_01_finalize`, `remove_version`, `deploy`,
  `deploy_01_finalize`, `upgrade`, `fail`, and any NEAR access-key
  changes on the registry owner account.
- Direct contact with: the Templar dev team
  ([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md)),
  every vault curator supplying into Templar markets
  ([`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)),
  NEAR Foundation
  ([`03-near-foundation.md`](./03-near-foundation.md)), and the NEAR
  Intents team
  ([`02-near-intents-team.md`](./02-near-intents-team.md)) if any
  intent routing depends on Templar market provenance.
- A pre-drafted public statement template per privileged-action
  class (routine version publish; emergency patched version publish;
  compromise notice).

---

## 1. Triage

When an alert fires that implicates the registry:

1. **Open the war room** ([`README.md`](./README.md#war-room-template)).
   Registry admin is *lead* for registry-admin-compromise incidents
   **while control of the admin key remains intact**; if the admin
   key has been captured, NEAR Foundation is *lead* per the
   canonical Safe Chain quorum in
   [`03-near-foundation.md`](./03-near-foundation.md) §9 (Registry
   admin compromise — admin captured). Registry admin is *technical
   advisor* for protocol hacks (where the dev team leads) and for
   curator / allocator / sentinel compromises that require deploying
   a fresh vault via registry.
2. **Snapshot state**: `list_versions`, `list_deployments`, and the
   `get_version_code_hash` for every version currently exposed via
   the registry.
3. **Classify** using the Hypernative table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
4. **Pick the smallest viable mitigation.** A registry admin's
   instinct should be conservative — the registry has no pause,
   and every privileged action is irreversible in kind (a bad
   `deploy` cannot be un-deployed; a bad `add_version` can be
   `remove_version`'d, but downstream `deploy`s from that version
   are already published).

---

## 2. Protocol hack

The registry admin does not directly fix a protocol hack; the Templar
dev team drives that response
([`01-templar-protocol-dev-team.md`](./01-templar-protocol-dev-team.md) §2).
Your role during a protocol hack is to publish the patched version
and deploy the migration market when the dev team is ready.

### 2.1 What to do

1. **Wait for dev team sign-off** on the patched artifact. Do not
   `add_version` under time pressure without dev on-call lead
   confirmation, even if the incident feels urgent — a rushed
   `add_version` that later needs `remove_version` reduces
   confidence in the registry.
2. **Verify the artifact you are about to publish.** Rebuild
   reproducibly from the audited commit. Confirm the code hash
   matches what the dev team expects.
3. **Publish and finalize**: `add_version` and
   `add_version_01_finalize`. Publish the version key and code hash
   in the war-room log and in the public manifest.
4. **Deploy the migration market**: `deploy` and
   `deploy_01_finalize`. Publish the new market account ID in the
   war-room log and to users.
5. **Do not `remove_version`** the exploited version during the
   incident — users still need to `withdraw` from the old market.
   Wait until migration is complete and dev team sign-off is in
   hand before delisting.

### 2.2 What you should *not* do

- **Do not `upgrade`** the registry itself under time pressure.
  Registry upgrades change how future `deploy` and `add_version`
  behave; a rushed upgrade during an incident risks compounding the
  problem. Route any needed registry-side changes through the
  standard dev-team review path.
- **Do not `remove_version`** any version that has live
  deployments, unless the dev team has confirmed that no user
  needs to interact with those deployments any more. `remove_version`
  affects future `deploy`s from that version; existing deployments
  keep running.
- **Do not `deploy`** additional instances of any version under
  investigation.

---

## 3. Bad debt

Registry admin has no direct role in a bad-debt incident on a
specific market — the market handles its own liquidation flow and
loss recognition. Registry admin's involvement is only if the
incident produces a parameter learning that motivates a
next-generation market: publish the corrected version and deploy
the successor.

---

## 4. Faulty oracle

Registry admin has no direct role during a faulty-oracle incident.
If the resolution requires a market with corrected oracle
configuration (wrong `price_id`, wrong `decimals`, wrong
`price_maximum_age_s` — these are immutable per-market fields per
[`docs/src/oracles.md`](https://github.com/Templar-Protocol/contracts/blob/dev/docs/src/oracles.md)), coordinate with the
dev team to `add_version` the corrected market code (if a code
change is involved) and `deploy` the replacement.

Note: for *mutable* proxy-oracle configuration issues (source
selection, freshness filters), the fix is via the proxy oracle's own
governance, not via registry. Registry is only involved if a proxy-
oracle-code-level change is needed.

---

## 5. Registry admin compromise (your own key)

If you suspect your registry admin key is compromised — **do
whatever you can to prevent further privileged actions** before
convening the war room. The registry has no pause, but you can:

- Rotate the NEAR access-key posture on the registry owner account
  (delete the suspected key, add a clean key from a known-good
  device) — if you still have any signing capacity that is *known*
  clean.
- If the owner is controlled by a multi-sig contract, follow the
  multi-sig's emergency procedures to reduce or reconfigure
  signers.

### 5.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | `add_version` / `deploy` / `upgrade` / `remove_version` transaction you did not authorize; unexpected NEAR access-key added to the registry owner account; signing infrastructure (HSM / multi-sig coordinator) shows tampering. |
| **High (P1)** | A signer is socially-engineered or phished; a multi-sig threshold has been narrowed; recovery seed is exposed. |
| **Medium (P2)** | Unusual sign-in attempts on signing infrastructure; one signer is unreachable for an unusual period. |

### 5.2 Containment

1. **Access-key rotation** on the registry owner account
   immediately, with whatever signing capacity you still have that
   is *known* clean. If you cannot rotate access keys, every step
   below becomes much harder.
2. **Convene the war room.** Registry admin is *lead* **only while
   admin control remains intact** (e.g. after successful access-key
   rotation in step 1 that neutralised the attacker). If the
   attacker still holds admin, NEAR Foundation is *lead* per
   [`03-near-foundation.md`](./03-near-foundation.md) §9 (Registry
   admin compromise — admin captured). The Templar dev team is
   technical advisor in either case.
3. **Publicly mark suspect versions and deployments.** Anything
   `add_version`'d or `deploy`'d after the earliest possible
   capture time is now untrusted until Safe Chain sign-off. Publish
   the exact timestamp range and the affected `version_key`s /
   deployment account IDs. Instruct integrators to verify code
   hashes against the public manifest via `get_version_code_hash`.
4. If the attacker completed `upgrade` on the registry itself, the
   registry contract's code has changed. Treat every subsequent
   `add_version` / `deploy` from that registry as untrusted. Plan
   for a fresh registry deployment.
5. If the attacker still has admin control after step 1, the
   registry is **functionally lost**. Coordinate with the Templar
   dev team, vault curators, the NEAR Intents team, and NEAR
   Foundation Safe Chain for fund-protection options: migration to
   a fresh registry with a clean admin, updated integrations,
   allowlist updates upstream.

### 5.3 Recovery

1. Forensic review: how did the key leak?
2. Replace signing infrastructure end-to-end. Do not reuse
   compromised hardware or compromised people-in-the-loop
   processes.
3. If you control a recovered admin, coordinate with the dev team
   to review all registry state (`list_versions`,
   `list_deployments`, `get_registry_entry`) for adversarial
   changes.
4. If a fresh registry was deployed, publish the migration path,
   deprecate the old registry in tooling and integrations.
5. Publish a post-mortem.

---

## 6. Curator / allocator / sentinel compromise

You are not the curator. Registry admin's role is only if the
response requires deploying a fresh vault via registry. Follow the
same publish / deploy discipline as in §2.1.

---

## 7. Communication protocol

- **Routine version publishes**: publish the `version_key`, code
  hash, source commit, and audit report link at the time of
  `add_version`. Update the public manifest.
- **Emergency patched version publishes**: prepend an incident
  reference to the routine publish notice; coordinate timing with
  the war-room comms lead.
- **Compromise notice**: coordinate with NEAR Foundation
  ([`03-near-foundation.md`](./03-near-foundation.md) §8 —
  Communication protocol). Do not publish attribution.

---

## 8. Stand-down checklist

- [ ] Access-key posture on the registry owner account restored to
      standing posture, verified out-of-band.
- [ ] `list_versions` and `list_deployments` reviewed against the
      public manifest. `deploy` is irreversible in kind (see §1),
      so the achievable condition is: every suspect version and
      deployment is publicly marked unsafe, excluded from Templar
      tooling and downstream integrations (front ends, vault supply
      queues, indexer displays), and covered by a migration and
      user-notification plan.
- [ ] Templar dev team informed of stand-down.
- [ ] NEAR Foundation informed of stand-down.
- [ ] Public post-mortem drafted (for compromise incidents).
- [ ] War room archived (append-only log preserved).
- [ ] Signing posture reviewed: same key set, same rotation
      cadence, same hardware. If any of these changed during the
      incident, document why.
