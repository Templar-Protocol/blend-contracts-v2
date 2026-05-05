# Runbook: Blend Protocol Dev Team

**Audience.** Engineers and on-call responders who maintain
[`blend-contracts-v2`](../../) (pool, pool-factory, backstop, emitter) and the
deployment / monitoring tooling around it.

**Scope.** You are the *protocol* responder. You do not generally hold pool
admin keys (those are held by per-pool admins, see
[`02-blend-pool-admins.md`](./02-blend-pool-admins.md)), but you own the
contracts, the build, the audit history, and the upgrade story. You are the
authoritative source of truth on what each pool / backstop / emitter call
*actually does* on-chain.

Read [`README.md`](./README.md) first for the severity matrix, Hypernative
classification, war room template, and Safe Chain coordination model.

---

## 0. Standing posture

Before any incident, the dev team must keep the following in a known-good state:

- A monitored fork of mainnet pool / backstop / emitter / pool-factory state
  with replay capability.
- A pre-built, signed, reproducible WASM artifact for every contract version
  currently in production. Builds must match `audits/`-referenced commits.
- A list of every deployed pool (pool factory event log) with: admin address,
  backstop address, oracle address, reserve list, current status.
- Hypernative (or equivalent) feeds wired to the pool / backstop / emitter
  events. At minimum: `set_admin`, `accept_admin`, `set_status`,
  `queue_set_reserve`, `cancel_set_reserve`, `set_reserve`, `bad_debt`,
  `new_auction`, `del_auction`, `update_pool`, `set_emissions_config`.
- A war room channel that can be joined within 5 minutes by:
  - Dev on-call (rotating).
  - Each pool admin on-call we publicly support.
  - Stellar Foundation security contact.
  - At least one validator-side contact (for Safe Chain).
- A pre-drafted public statement template ("we are aware of a potential
  incident on Blend pool X, investigating, no action required from users
  yet") so comms is not blocked on writing prose.

---

## 1. Triage

When a P0 / P1 alert fires:

1. **Open the war room** using the template in
   [`README.md`](./README.md#war-room-template). The dev on-call is the
   default *lead* for any incident classified as protocol hack, bad debt,
   or faulty oracle. For pool / curator / allocator / sentinel compromise,
   the dev on-call acts as *technical advisor* and the affected role is
   lead.
2. **Classify** the alert family using the table in
   [`README.md`](./README.md#hypernative-alert-taxonomy).
3. **Snapshot state.** Before any mitigation, record for every affected pool:
   - Pool status (`get_config().status`).
   - Reserve list and per-reserve `Reserve` view.
   - Backstop `pool_data` (especially `q4w_pct`).
   - All open auctions (`get_auction(0|1|2, user)` for known users).
   - Last few hundred ledger entries of pool events.
4. **Decide containment** with the relevant role(s). The dev team's role in
   containment is to confirm that the proposed action is consistent with
   contract semantics and will not make things worse. Do not advise any
   action you have not personally re-derived from the source.

---

## 2. Protocol hack

A protocol hack is any state where the on-chain invariants of pool / backstop /
emitter are violated, or where an attacker is extracting value from a contract
in a way the design did not contemplate.

### 2.1 Detection (Hypernative classes)

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Confirmed invariant break (e.g. b-token total supply / underlying mismatch beyond rounding), unexpected outbound transfer from pool, share-price discontinuity in a curator vault, reentrancy alert. |
| **High (P1)** | Unusual call pattern: rapid `submit` chains from one address, abnormal `flash_loan` usage, repeated `gulp` callsites. |
| **Medium (P2)** | Anomaly in oracle inputs combined with unusual borrow growth; large `new_auction` from non-backstop user. |

### 2.2 Containment workflow

1. **Identify the smallest pool / reserve set that contains the exploit.** Do
   not freeze pools that share only the contract code if they do not share
   the vulnerable state. Each Blend pool is isolated.
2. **Coordinate with the affected pool admin(s).** Per
   [`02-blend-pool-admins.md`](./02-blend-pool-admins.md), the admin can call
   `set_status(4)` (admin frozen) which supersedes all other statuses and
   blocks borrows / cancellations / supplies. The dev team's job is to:
   - Confirm `set_status(4)` is the right step (vs `set_status(2)` admin
     on-ice if borrowing alone needs to stop).
   - Pre-sign or pre-broadcast helper transactions if the admin requests
     them (e.g. simultaneous `set_status` on multiple admins' pools).
3. **Coordinate with vault curators** (see
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)).
   When a pool is frozen, vaults supplying into that pool's reserves cannot
   withdraw on behalf of their depositors. Curators may need to deallocate
   from sibling reserves first.
4. **Notify Stellar Foundation** via the security contact (see
   [`03-stellar-foundation.md`](./03-stellar-foundation.md)). Foundation is
   responsible for cross-protocol communication with bridges, validators
   and stablecoin issuers if the incident has wider blast radius.
5. **Preserve evidence.** Once containment is in place, snapshot the pool
   ledger entries, all relevant TTL extensions, and the full call trace of
   the exploit transactions. Do *not* call `del_auction` on stale auctions
   that may be evidence; let them age out only after forensics is done.

### 2.3 Recovery

Pool / backstop / emitter contracts in `blend-contracts-v2` have **no
upgradeability primitive**. Recovery means deploying a patched contract and
migrating state.

1. Cut a patched commit. Update `audits/` with a delta note describing the
   issue and the fix.
2. Build reproducibly; publish artifact and source.
3. Coordinate a new pool deployment via `pool-factory`.
4. Coordinate user migration. Users withdraw from the frozen pool (the admin
   may need to thaw to status 5 / 3 long enough for orderly withdrawals; do
   not unfreeze to 0 / 1 without dev sign-off) and re-supply into the new
   pool.
5. Sunset the old pool. After all user funds are migrated, leave the old
   pool admin-frozen (status 4) and document the migration in a public
   post-mortem.

Stand-down requires sign-off from the dev on-call lead **and** the affected
pool admin (Safe Chain rule).

---

## 3. Bad debt

Bad debt arises when a borrower is liquidated to zero collateral while still
owing liabilities. The `bad_debt` entrypoint
([`pool/src/contract.rs`](../../pool/src/contract.rs)) transfers the residual
liabilities to the backstop. If the backstop has less than ~5% of its
threshold, `check_and_handle_backstop_bad_debt`
([`pool/src/pool/bad_debt.rs`](../../pool/src/pool/bad_debt.rs)) defaults the
debt — i.e. socializes the loss to suppliers in that reserve.

### 3.1 Detection

| Class | Signal |
|-------|--------|
| **High (P1)** | Backstop `q4w_pct` ≥ 60% (pool will auto-transition to status 5 frozen on next `update_status`). Bad-debt auctions stalling. |
| **Medium (P2)** | Single user with negative health that cannot be liquidated due to oracle staleness or low liquidity. `q4w_pct` ≥ 30%. |
| **Low (P3)** | Liquidation chain stalls, but health factors recover within minutes. |

### 3.2 Dev team responsibilities

The dev team does not call `bad_debt` directly under normal operation;
liquidator bots do. Your job is to make sure the bots can do their job and
to verify that the backstop / pool math is correct after each event.

1. **Verify the chain of events.** Was a user liquidated? Did the liquidator
   fail to fully clear collateral? Is `bad_debt(user)` callable for that
   user (it panics with `BadRequest` if there is no bad debt to handle)?
2. **Confirm backstop solvency.** Compute the current backstop product
   constant (see `calc_pool_backstop_threshold` in
   [`pool/src/pool/status.rs`](../../pool/src/pool/status.rs)). If it is
   below ~5% of threshold, the next `bad_debt` call will *default* (socialize)
   the loss instead of transferring it to the backstop.
3. **Coordinate with the pool admin** before defaulting socializes more than
   the admin expects. The admin may want to call `set_status(2)` (on-ice) to
   stop new borrowing while the bad debt is being worked through.
4. **Coordinate with vault curators** so they can de-risk affected reserves
   per [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md)
   before more depositors share the loss.

### 3.3 Recovery

If the bad debt is the result of a one-off market move and the backstop
absorbed it, no contract change is needed. If it is the result of a
parameter mistake (`c_factor` / `l_factor` / `max_util` set too aggressively),
the dev team should work with the pool admin to:

1. Queue a tighter reserve config via `queue_set_reserve`.
2. Communicate the change publicly before `set_reserve` executes it.
3. Update audit notes and recommended-parameter guidance.

---

## 4. Faulty oracle

Pools accept any `oracle` contract address at construction. The oracle
contract must conform to the Blend oracle interface (price + decimals + max
age). Faults include:

- **Stale prices** — feed has not been pushed within the configured maximum
  age, or the oracle contract itself is stale.
- **Manipulated prices** — feed value diverges sharply from sibling feeds or
  from external venues.
- **Confidence collapse** — the price is technically fresh but the
  underlying data sources are degraded.
- **Wrong feed wired in** — the wrong asset / decimals was configured for a
  reserve.

### 4.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | Price deviation > 5% across sibling oracle feeds, or price moves > N% in one ledger with no off-chain explanation. |
| **High (P1)** | Single-feed staleness > 2× the configured max age; oracle provider acknowledges incident. |
| **Medium (P2)** | Confidence band collapse; one provider in a multi-provider feed disconnects. |

### 4.2 Containment

1. The dev team **does not directly hold the oracle keys**. The oracle is a
   third-party contract (Pyth / Reflector / Redstone / SEP-40 adapter etc).
2. The fastest mitigation that the protocol can apply is via the pool
   admin: `set_status(2)` admin on-ice (blocks new borrows and liquidation
   cancels) or `set_status(4)` admin frozen (blocks everything).
3. Coordinate with the oracle provider (the Stellar Foundation contact in
   [`03-stellar-foundation.md`](./03-stellar-foundation.md) maintains
   provider relationships).
4. Coordinate with vault curators so they can deallocate from any reserve
   that prices via the affected feed.

### 4.3 Recovery

1. Wait for the oracle to recover, or for the oracle provider to publish
   guidance on which prices are safe.
2. Once safe prices are confirmed, work with the pool admin to step status
   back: 4 → 2 → 0/1. Do not skip steps.
3. For wiring faults (wrong decimals / wrong feed id), the only fix is a
   new pool with corrected configuration, since reserve configs cannot
   be edited freely after `set_reserve` executes for non-config fields.

---

## 5. Pool admin compromise

A pool admin compromise is when the EOA / multisig / contract that holds the
pool's `admin` slot is suspected to be controlled by an attacker.

### 5.1 Detection

| Class | Signal |
|-------|--------|
| **Critical (P0)** | `propose_admin(<unknown_address>)` event from the admin, followed by an `accept_admin` from that address; or `set_status(0)` immediately after a freeze event you did not authorize. |
| **High (P1)** | `queue_set_reserve` with extreme parameters (e.g. `max_util` near 100%, `c_factor` raised); `set_emissions_config` to drain emissions; `update_pool` with a 0 `min_collateral`. |
| **Medium (P2)** | Admin signing key rotates without prior comms. |

### 5.2 Dev team response

You do not have admin custody, but you control the *facts*:

1. **Verify the call.** Check the transaction envelope, signers, and
   originating account against the pool admin's known signing posture.
2. **If the admin is reachable**, support them in following
   [`02-blend-pool-admins.md`](./02-blend-pool-admins.md) section 5.
3. **If the admin is not reachable or is suspected captured**, the protocol
   has limited mitigations. The Stellar Foundation Safe Chain runbook
   ([`03-stellar-foundation.md`](./03-stellar-foundation.md) section 5)
   applies. The dev team's job is to:
   - Publish a clear, factual statement about which pool is affected and
     which actions are now considered untrusted.
   - Help vault curators evaluate whether they can withdraw from the
     reserves of the affected pool *before* the attacker reconfigures
     them.
   - Help liquidators decide whether liquidation incentives published via
     a captured admin are safe to follow.
4. **Plan the migration.** Compromised admins are not recoverable in
   `blend-contracts-v2`; the long-term answer is a new pool with a clean
   admin. Coordinate that deployment.

---

## 6. Curator / allocator / sentinel compromise

These are *vault* roles, not pool roles. The dev team's responsibilities are
indirect.

1. **Confirm the blast radius**: which Blend pools / reserves does the
   compromised vault supply into?
2. **Watch for second-order effects**: a captured allocator may flood a
   single reserve with deposits / withdrawals to manipulate utilisation
   and rates. Pool-level signals to monitor are sudden `b_rate` jumps,
   reserve utilisation crossing `max_util`, and burst withdrawals.
3. **Support the curator** following
   [`04-vault-curators-allocators-sentinels.md`](./04-vault-curators-allocators-sentinels.md).
4. If the compromised vault holds significant fraction of a pool's
   liquidity, talk to the pool admin about whether to step the pool to
   admin on-ice while the vault is being recovered.

---

## 7. Communication protocol

- **Public statements** during a P0 / P1 are issued only by the dev on-call
  lead or the comms lead in the war room. No-one else speaks publicly.
- **Channels**: official protocol social, status page, GitHub security
  advisory once a fix is shipped. Coordinate with Stellar Foundation
  before publishing if the incident has cross-protocol impact.
- **What to publish at each phase**:
  - *Detection*: "We are investigating an incident affecting pool X.
    Containment action Y has been taken. No further user action is
    required at this time." / or specific user instructions if there are
    any.
  - *Containment*: confirm the pool status, what is and is not still
    callable, expected next checkpoint time.
  - *Recovery*: migration steps, deadlines, contract addresses.
  - *Post-mortem*: published only after Safe Chain stand-down, including
    audit-of-the-fix references and parameter changes.

---

## 8. Stand-down checklist

Before declaring an incident resolved, the dev on-call lead must confirm:

- [ ] Root cause identified and reproduced in a test.
- [ ] Patch (if any) audited or peer-reviewed by at least one engineer not
      on the on-call rotation.
- [ ] All affected pools have either been migrated or their status path back
      to normal has been verified.
- [ ] All affected vault curators have stepped down to normal posture.
- [ ] Stellar Foundation contact has been told the incident is closed.
- [ ] Public post-mortem drafted and queued.
- [ ] War room archived (append-only log preserved).
- [ ] Hypernative / monitoring rules updated to detect the same class of
      event earlier next time.

A sign-off from a second Safe Chain role (Foundation, pool admin, or a
curator) is required before user-visible action is reversed.
