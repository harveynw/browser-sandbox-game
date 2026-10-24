# Local profiles — T17

The default `cargo run -p server --locked` uses `config/dev.toml` and saves profiles in `var/profiles.sqlite`. Paths in the persistence configuration resolve from the repository root; an absolute path is also accepted. To choose disposable memory storage explicitly, run `cargo run -p server --locked -- --config config/memory.toml`. Tests create unique temporary directories and never open the developer save.

Use the same browser profile and hostname when reconnecting: the HttpOnly `pm_guest` cookie identifies the save. SQLite mode gives that cookie a one-year lifetime; deleting it creates a different guest. The database stores its SHA-256 hash, never the raw cookie secret. Tickets remain one-use, memory-only, bounded and expire after 30 seconds. This is still an explicit localhost guest issuer rather than public account authentication.

## What survives a restart

Committed credits, completed-job count, display name and economic event IDs persist. The server creates a new world ID and starts the city from authored content. Position, possession, vehicles, heat, active missions and unfinished dwell do not persist. Old session IDs and join tickets cannot resume the new world.

To check it manually, complete a courier delivery, note the balance, stop the server with Ctrl+C, start it again, refresh the client and choose Play. The same guest should keep the balance and return on foot at a spawn with no active job. A reward is earned only when the committed balance appears; an unfinished or uncommitted job is not a saved objective.

## Worker and transactions

`admission::Profiles` owns bounded admission/cache state and selects memory or SQLite. `persistence.rs` owns the sole database connection on its worker thread. Startup takes an OS file lock for that database, enables WAL, foreign keys and FULL synchronous writes, applies migration 1 transactionally, rejects a future/missing schema, and recovers unresolved repair debits before opening gameplay listeners. A second cooperating server cannot open the same save and mistakenly compensate the first server's live repairs.

There are at most 64 outstanding worker operations and bounded request/result channels. A world tick only submits with `try_send` and polls completed results. SQLite busy/locked failures get three bounded attempts with a 50 ms busy timeout and short backoff; ongoing mission/repair outcomes retry the same IDs after a 30-tick cooldown. Queue pressure leaves actions pending or returns unavailable. Readiness requires a live worker and recovered database availability as well as the existing WS/RTC receipt probe.

HTTP session creation/loading returns 202 with a guest cookie while pending, then 200 with committed profile data. The client polls within a five-second, cancellable admission deadline. ClientHello consumes its ticket once and waits in AwaitHello for the worker's display-name/profile result. No player or Welcome is allocated before that succeeds; expiry, disconnect and capacity checks still apply.

Economic transactions insert the event and update the bounded balance/job count atomically. Mission IDs use `mission:<instance>`, repair debits use `repair:<session>:<request>`, and refunds use `refund:<debit-event>`. Replaying a matching event returns the current committed profile without applying it again; conflicting owner/kind/amount is rejected. Insufficient funds roll back both the event and balance. Only one economic transaction per profile is submitted at once. Mission rewards also wait for repair application acknowledgement, and the database rejects a new reward while that profile has an unresolved repair debit, preserving refund headroom. PlayerStatus publishes committed values. While a mission reward waits, its private target is null and the client displays reward-pending guidance; the existing version-2 wire shape is unchanged. Both build IDs advance to `t17-local`.

The implementation uses pinned [rusqlite](https://docs.rs/rusqlite/0.40.2/rusqlite/) with bundled SQLite, TOML configuration and Ctrl+C/SIGTERM handling. No database code is shipped to the browser or shared simulation crates.

## Repair compensation and shutdown

A debit atomically inserts a `repair_pending` record. The world still checks the original player/session/car/control epoch and observed eligibility after commit. If valid, it repairs that car and queues acknowledgement to clear the record. If invalid, it submits the uniquely keyed refund and clears the pending record in that transaction. New economic requests wait while that profile's application acknowledgement is pending.

At startup, every remaining pending debit is refunded in the migration/recovery transaction. A crash after debit commit but before live application, or after application but before acknowledgement, therefore refunds once in the new world. Acknowledged repairs stay paid. This deliberately avoids retaining a charge for an uncertain application to a car that no longer exists after restart. Refunds require a matching unresolved debit and cannot mint credits independently.

Ctrl+C/SIGTERM marks the server unavailable, stops gameplay, disconnects ownership and drains repair compensation plus submitted worker work within a five-second budget. Any unresolved committed debit remains recoverable on next boot. Abrupt process termination relies on SQLite transactions and startup recovery. Ordinary builds/startups/tests never reset saves. Back up the SQLite database using SQLite-aware tooling, or stop the server before copying the database; do not delete a running save. No reset command is introduced.

## Verification scope

Nine temporary-database tests cover migrations and hashed secrets, reward replay/conflicts/reopen, funds/refund rollback, acknowledged vs unresolved repairs across repeated startup, schema failures, database ownership, nonblocking lock retries, bounded queues, HTTP pending admission and configuration. Three real-codec world scenarios cover withheld rewards and restart/rejoin, interrupted repair compensation without healing, and committed repair application/acknowledgement/restart. Existing memory-mode gameplay scenarios remain regression fixtures. These tests arrange game poses and database locks explicitly; they are not live playthroughs or RTC load benchmarks.


Live production verification completed a courier through ordinary keyboard/menu controls and observed **150 credits / one completed job** in SQLite. Ctrl+C reported a fully drained shutdown. Restart changed the world ID; refreshing and rejoining with the same browser cookie restored **150 credits** at the original spawn, with no active mission. Evidence: [earned credits](evidence/t17-earned-credits.png), [credits after restart](evidence/t17-restart-credits.png). No live game state or database values were injected.
