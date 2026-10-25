# 07 — Server interfaces

[Index](../README.md)

T07 implementation: [21 — Authoritative localhost sessions](21-authoritative-sessions.md) owns the concrete module APIs, memory-only scope and local browser-reported readiness gate. The concrete T17 persistence adapter and restart rules are documented in [31 — Local profiles](31-local-profiles.md); conceptual interfaces below describe system boundaries.

Interfaces below are implementation contracts. Helper types may be organized into smaller files, but ownership and behavior must remain explicit.

## World and services

```rust
pub struct World {
    pub tick: u32,
    pub world_id: String,
    pub entities: EntityStore,
    pub sessions: SessionStore,
    pub collision: CollisionWorld,
    pub missions: MissionStore,
    pub population: PopulationDirector,
    pub rng: SeededRng,
}
pub struct Services {
    pub content: std::sync::Arc<WorldContent>,
    pub persistence: PersistenceHandle,
    pub metrics: Metrics,
}
pub struct TickOutput {
    pub reliable: Vec<RecipientMessage>,
    pub state_updates: Vec<RecipientState>,
    pub persistence_commands: Vec<PersistenceCommand>,
    pub disconnects: Vec<DisconnectCommand>,
}
pub fn step_world(
    world: &mut World,
    ingress: TickIngress,
    persistence_results: Vec<PersistenceResult>,
    services: &Services,
) -> TickOutput;
```

`EntityStore` is a monotonic ID allocator and ordered collections of players, vehicles, pedestrians, and police. The store owns all gameplay state. Collision handles are private indexes maintained by `CollisionWorld`. Despawning removes handles and releases reservations atomically.

`SessionStore` maps our connection key `(peer_id, local_generation)` to session ID, profile ID, player ID, phase, control epoch, input ring, action result cache, visibility set, and last-heard time. Never expose the crate handle to gameplay systems.

`TickIngress` contains bounded validated messages grouped by connection, plus open/closed notices. Payload decoding occurs off the simulation thread; ownership and time validation occur inside the tick.

## Transport boundary

```rust
pub struct PeerKey { pub id: u32, pub generation: u64 }
pub enum TransportEvent {
    Opened(PeerKey),
    Message { peer: PeerKey, message: ClientEnvelope },
    Closed { peer: PeerKey, reason: CloseReason },
}
pub enum Delivery { Reliable, Unreliable }
pub enum SendOutcome { Queued, ReplacedStale, Full, Closed }
pub trait GameTransport {
    fn drain(&mut self, max_events: usize) -> Vec<TransportEvent>;
    fn send(&mut self, peer: PeerKey, delivery: Delivery,
            message: ServerEnvelope) -> SendOutcome;
    fn disconnect(&mut self, peer: PeerKey, reason: CloseReason);
}
```

Implement `NetCrateTransport` as bounded queues to the dedicated pump, plus `FakeTransport` for tests. Only the pump calls `net::Server` methods. It maintains explicit open state and drops pre-open payloads.

`Full` on reliable output triggers local cleanup and then a kill request. `Full` on state output replaces/drops old state. Encode before enqueue so sizes and budgets are known. `broadcast` is not used for gameplay because membership and AOI are per recipient.

`disconnect` performs idempotent application cleanup immediately; a later Closed is harmless. Crate startup or thread failure marks health as failed and initiates orderly process shutdown. Full detection of internal crate failure requires the hardening task.

## Session state machine

```text
transport Open
  -> AwaitHello (send ServerHello; 5-second deadline)
  -> Loading (ticket consumed; profile loaded; inert player allocated;
              send Welcome and baseline; 30-second deadline)
  -> Playing (ClientReady verified; send StartPlay)
  -> Loading (explicit resync)
  -> Closing (timeout/rejection/kick/Leave/failure)
  -> Removed
```

The browser can load assets before joining to minimize Loading. Only one active session per guest profile: a second valid join replaces the old session after cleanup. Capacity reservations include Loading sessions. Unknown peer messages cannot allocate players.

Do not block a tick to load a profile. Await the worker result in a pending admission state within AwaitHello, with a bounded timeout and one consumed ticket. No gameplay entity is allocated until profile loading succeeds.

On disconnect:

1. Mark phase Closing and stop admitting input/actions.
2. Release driver ownership; stop the car safely, increment its control-related state as needed.
3. Fail/cancel active mission, release reserved recovery vehicle, clear police target assignments.
4. Remove player collision/entity and send appropriate visibility hides.
5. Remove session/observer/input/action cache. Committed profile updates remain.
6. Issue transport close without waiting for `Event::Closed`.

No reconnect grace possession in alpha.

## Exact tick order

For tick S = previous tick + 1:

1. Drain bounded lifecycle and authenticated messages. Process disconnects before gameplay.
2. Apply completed persistence results; publish committed economic changes.
3. Admit hello/readiness/resync transitions. Allocate entity IDs in stable order.
4. Validate and cache inputs/actions. Choose current input or neutral for S.
5. Process control actions (entry/exit) and mission accept/cancel in peer-key/request-ID order. Winning entry increments control epoch and makes old-epoch input inapplicable this tick.
6. Update NPC decisions scheduled for this tick.
7. Step vehicles in ascending entity ID, updating query proxies after each.
8. Step on-foot characters in ascending entity ID, then attach driving players to seat anchors.
9. Resolve fire/reload, impacts, damage, downing, and arrest; validate fire against current poses.
10. Evaluate mission zones/checkpoints, pursuit timers, repair requests, respawn/despawn.
11. Submit newly required persistence transactions; do not announce uncommitted rewards.
12. Recompute interest changes; build reliable lifecycle/control/status output.
13. Every third tick build owner states and snapshots; assign server tick S to all final states.
14. Publish metrics and output; set world tick S.

Events produced in a tick are staged and resolved once, avoiding mutation while iterating entity collections. When actions invalidate one another, the first valid action in stable order wins and the next gets a concrete failure. Damage prevents mission completion in the same tick: terminal failure is resolved before reward.

## Gameplay module interfaces

```rust
pub fn try_enter_vehicle(ctx: &mut GameplayContext, player: EntityId,
                         vehicle: EntityId) -> Result<ControlChange, ActionError>;
pub fn try_exit_vehicle(ctx: &mut GameplayContext, player: EntityId)
    -> Result<ControlChange, ActionError>;
pub fn resolve_shot(ctx: &mut GameplayContext, player: EntityId, aim: Aim)
    -> Result<ShotResolution, ActionError>;
pub fn accept_mission(ctx: &mut GameplayContext, player: EntityId, template: &str)
    -> Result<MissionInstanceId, ActionError>;
pub fn update_missions(ctx: &mut GameplayContext, motions: &[ActorMotion])
    -> Vec<MissionOutcome>;
pub fn update_police(ctx: &mut GameplayContext, crimes: &[CrimeEvent]);
pub fn update_population(ctx: &mut GameplayContext);
```

`GameplayContext` is a controlled borrow of world stores, immutable content, tick, and staged domain events. It has no socket/database access. `ActionError` maps exhaustively to protocol `ActionCode`. A successful control operation emits one ControlChanged to the owner and updates affected observer state.

Use server-generated domain events such as `CrimeEvent`, `DamageEvent`, `MissionOutcome`, and `ControlChange`. They are not wire enums; the replication layer decides recipients and redacts private fields.

## Replication interface

```rust
pub struct ObserverState {
    pub visible: OrderedMap<EntityId, VisibilityEntry>,
    pub next_visibility_id: u32,
    pub baseline_id: u32,
    pub last_status_revision: u32,
    pub budget: ByteBudget,
}
pub fn update_interest(world: &World, observer: &mut ObserverState,
                       player: EntityId) -> Vec<VisibilityChange>;
pub fn build_replication(world: &World, observer: &mut ObserverState,
                          player: EntityId) -> ReplicationBatch;
```

For a new visible entity, queue EntityShow before any other reliable reference requiring it. RTC can still overtake WS, so client unknown-record rules remain necessary. Owner and mission target descriptors are always visible. A status target outside AOI can supply only a marker coordinate if the target is not otherwise pinned.

Baseline creation freezes the initial observer set/count and uses one bounded reliable FIFO. Subsequent lifecycle changes append after BaselineEnd. Each visibility cycle gets a fresh increasing ID; do not reset that counter during resync within the same session.

Status revision increments only when public-to-owner status fields change. Send the complete initial PlayerStatus immediately after StartPlay on the reliable FIFO. Never include another player’s balance, private mission, auth token, or internal AI path in their replica.

## HTTP control plane

```text
GET  /health/live       200 if process supervisor is alive
GET  /health/ready      200 only after world + persistence + transport probe ready
GET  /api/build         protocol_version, build_id, content_hash, sim_config_hash
POST /api/session      create/reuse guest; return profile display defaults
POST /api/join-ticket  one-use ticket, expires_in_ms=30000, ws_url
GET  /api/metrics      implemented loopback-only cached timing/queue/population/byte report
```

Profile cookie: opaque random secret, HttpOnly, Secure in production, SameSite=Lax. Store only its hash server-side. Join ticket: cryptographically random 256-bit opaque token; bind to profile and shard, expire after 30 seconds, consume once in ClientHello, cap pending tickets. Ticket issuance requires same-origin requests and strict Origin checks. Local dev may use an explicitly configured development guest issuer; never hardcode a release token.

This authentication occurs after RTC setup with the baseline crate, so it does not protect against pre-auth RTC resource consumption. Pre-upgrade limits/admission are part of the public-release transport gate.

## Persistence contract

SQLite tables:

```text
profiles(
  profile_id TEXT PRIMARY KEY,
  cookie_secret_hash BLOB UNIQUE NOT NULL,
  display_name TEXT NOT NULL,
  credits INTEGER NOT NULL,
  completed_jobs INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
)
economic_events(
  event_id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES profiles,
  kind TEXT NOT NULL, amount INTEGER NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
)
schema_migrations(version INTEGER PRIMARY KEY)
```

All economic updates are transactions keyed by unique event IDs: `mission:<instance_id>` or `repair:<session_id>:<request_id>`. Insert event and update balance in one transaction; repeated event returns the already committed result. Validate nonnegative bounded balance inside the transaction. Restrict a profile to one in-flight economic transaction in alpha.

The worker interface accepts `LoadProfile`, `SetDisplayName`, and `ApplyEconomicEvent`; results return operation ID and committed profile values. The world may mark a mission `reward_pending` internally, but the client sees completion/reward only after commit. Do not award in memory and “save later.” On DB error retain a pending outcome, show a temporary unavailable status, retry with the same event ID, and reject further economic actions.

For repairs, reserve the request, debit once, then repair if the live actor is still eligible; if it is no longer eligible, submit a compensating uniquely keyed refund. On disconnect before application, refund rather than repair an unrelated car. This makes the asynchronous boundary explicit.

Memory mode implements the same interface and idempotency semantics. Save no active missions, car state, or position. On process crash the map restarts from content. Committed profile transactions survive only with the T17 durable adapter; T11 memory mode retains them for the running process and reconnects only.

## Startup/shutdown

Startup: validate config → load/bake-verified content → apply DB migrations → start workers/control plane → start net listener → transport probe → ready. Fail startup on hash, collision, schema, or bind mismatch.

Shutdown: stop tickets/joins → mark not-ready → notify connected clients → neutralize controls → finish in-flight persistence for at most 5 seconds → close sessions/workers → exit. Uncommitted rewards are not reported as earned. Log outcome and pending IDs for diagnosis.

### T16 memory repair worker

`MemoryProfiles` queues repair economic events with a minimum one-tick completion boundary, up to 32 queued operations and four completions per tick. Tests can delay completion through `repair_delay_ticks`. `World` permits one repair reservation per profile and car and defers mission reward commits while that profile's repair is in flight. Dwell progress is observed for 60 ticks, not supplied by the client. Application rechecks the original session, player, control epoch, car ownership, zone, speed, health and pursuit state; any interruption after submission latches invalidation. Disconnect leaves the operation available for completion/refund, even if the guest rejoins with a different entity or car.

A debit reserves one of the 4,096 ledger entries for its possible `refund:<repair-event>` compensation before committing. Other rewards cannot consume that slot. Successful application releases the unused reservation; compensation atomically replaces it with a refund event. Transient unavailable results retain/retry the same operation ID; committed balance is published only after completion. This remains memory-only and is replaced by durable storage in T17.


### T17 durable adapter

`admission::Profiles` now selects memory or a bounded SQLite worker. Default local configuration selects SQLite. Migration 1 adds the tables above plus `repair_pending(event_id PRIMARY KEY, profile_id UNIQUE)` as a durable debit/application acknowledgement boundary. The SHA-256 cookie hash is stored as 64 ASCII hex bytes in the BLOB column. No raw cookie is persisted. HTTP profile preparation uses 202/200; ClientHello consumes its ticket and awaits committed name/profile data before allocating a player. The world never waits on a database receive.

Worker requests/results and outstanding work are capped at 64. Busy/locked SQL gets three attempts with a 50 ms busy timeout; ongoing economic outcomes retry with their original IDs after 30 ticks. Successful repair application clears its pending debit through the worker; interrupted application refunds atomically. Startup refunds unresolved debits once before listeners, so a crashed world cannot strand a repair charge. An OS ownership lock prevents a second server from running recovery against a live save. See [31 — Local profiles](31-local-profiles.md) for shutdown, tests and configuration.

T20 measurement semantics and bounded collection are documented in [Capacity and performance](36-capacity-and-performance.md). Excess input batches above the strict sixty-per-rolling-second processing cap are discarded and counted; sustained byte floods still retire their session.
