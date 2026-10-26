# Guest profiles and persistent economy

[Documentation index](../README.md#documentation) · [Operations](operations.md) · [Gameplay](gameplay.md)

## Save identity

`config/dev.toml` selects SQLite at **`var/profiles.sqlite`**. Relative database paths resolve from the repository root; absolute paths are accepted. `config/memory.toml` selects disposable in-memory storage with the same economic interface. Normal startup/build/test never resets the developer save.

A guest's HttpOnly **`pm_guest`** cookie identifies the profile. SQLite mode uses a one-year cookie lifetime; the database stores only the secret's SHA-256 hash. Use the same hostname/context to return to that guest. Separate/private contexts give independent guests; same-hostname tabs replace one active session.

Guest cookies belong to a particular database. Use a private/separate context for temporary databases: joining a different database through the same hostname/context can replace the original guest cookie while leaving the original database file intact. No public account/recovery service is implemented.

| Saved | Reset at world restart / reconnect |
|---|---|
| Committed credits, completed-job count, display name | Position, possession, vehicles, heat, active mission, dwell |
| Economic event IDs and pending repair recovery state | World/session IDs and one-use join tickets |

## Worker and admission

`admission::Profiles` owns bounded profile/admission/cache state and uses either the memory adapter or `persistence.rs`'s single worker/connection. Startup takes a database file lock, enables WAL/foreign keys/FULL synchronous writes, applies migration **1** transactionally, rejects incompatible schema state and recovers unresolved repair debits before gameplay listeners open. Cooperating servers cannot simultaneously compensate the same save's live repairs.

The worker permits **64 outstanding operations** and bounded request/result channels. Simulation submits with `try_send` and polls results; it never blocks on database receive. Busy/locked failures get three bounded attempts with a 50 ms busy timeout and short backoff. Pending mission/repair outcomes retry identical IDs after a thirty-tick cooldown. Pressure yields pending/unavailable results rather than unbounded queues.

HTTP session preparation can return 202 with the guest cookie, then 200 with committed data. The browser has a five-second cancellable poll deadline. ClientHello consumes its ticket once and awaits committed display-name/profile loading before allocating a player or sending Welcome. Readiness requires a live available persistence worker plus recent actual transport receipts.

## Transactions and idempotence

`profiles` holds guest identity and bounded balances/job counts. `economic_events` records the unique event, owner, kind, amount and creation time. Inserting the event and updating profile values is one transaction. Matching retries return current committed values without reapplying them; conflicting owner/kind/amount reject. Insufficient funds roll back the event and balance.

Mission rewards use `mission:<instance>`; repair debits use `repair:<session>:<request>`; compensation uses `refund:<debit-event>`. One economic operation per profile is submitted at a time. PlayerStatus publishes committed values only; a pending mission reward has no active target and cannot be spent as completed money.

## Repair debit, application and compensation

A committed debit also creates a **`repair_pending`** record. The world rechecks the original session/player/control epoch/car and observed eligibility after commit. Valid application heals only that car and queues acknowledgement to clear pending state. Lost eligibility submits the uniquely keyed refund instead; a rejoined guest's different car is never healed.

New economic work waits while application acknowledgement is pending. Mission reward submission and database constraints preserve refund headroom. The bounded memory adapter reserves compensation capacity before debit, releasing it on successful application or replacing it with the refund event. SQLite startup refunds all unresolved pending debits transactionally, so a crash after debit cannot silently lose the payment.

Ctrl+C/SIGTERM stops new gameplay, performs bounded persistence draining and reports whether it drained. Unacknowledged repair debits remain recoverable at next startup. A failed clean drain is not evidence that uncommitted mission rewards were saved.

## Verification

Native scenarios cover committed-only publication, matching/conflicting retry IDs, insufficient funds, delayed debit after disconnect, refund/acknowledgement restart behavior, ledger pressure and fresh-world profile restoration. The [recorded browser acceptance](evidence/persistence.json) earned one real courier reward and retained exactly one 150-credit mission event across restarts. Its profile rows are limited to named test identities; these tests do not merge balances into normal saved play.
