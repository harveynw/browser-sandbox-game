# Data-only server components

Scope: consolidate all 15 production ECS components into `crates/server/src/components.rs`, with nested Weapon/Slot/Lifecycle payloads. Keep component field layouts and the live schedule stable. Components contain data declarations, documentation and derives only; no handwritten implementations, constructors, Deref forwarding or gameplay functions.

## Implementation stages

1. Extract weapon operations and initialization into `systems/weapons/state.rs`; shot geometry into `systems/weapons/geometry.rs`. Use explicit equipped inventory slots. Move weapon regression tests alongside operations.
2. Extract public character and inventory projection into `replication/projection.rs`; pedestrian stepping into `systems/movement/pedestrians.rs`; shared vehicle/pedestrian damage into `systems/damage.rs`; stopping, anchors, collision proxies and safe exits into `systems/vehicle.rs`.
3. Extract Wanted initialization, observation and progression plus Unit control helpers into `systems/police/state.rs`; extract Traffic input generation into `systems/population/input.rs`.
4. Consolidate Player, Vehicle, Pedestrian, Wanted, Unit, Officer, Traffic, GameId, VehicleHome, DisabledVehicle, Incapacitated, PoolMembership, PlayerMission, RecoveryReservation and SecuredVehicle. Remove entities/ and metadata.rs; retain tuple-component fixture adapters only in ecs/values.rs. Update production and test consumers.
5. Validate workspace compilation, the full server tests, lint/formatting and release build. Restart locally and run the connection probe; browser gameplay verification remains with the user.

All five stages complete. Workspace/all-target compilation, formatting and whitespace checks passed. Full server suite: 165 passed, 5 existing ignored. Clippy completed with 22 pre-existing warnings. Release server/probe built successfully. Local restart preserved saves; connection probe verified baseline, StartPlay, WS and RTC receipts. Browser gameplay testing remains with the user.

## Preserved contracts

Shot eligibility, ammo commitment, deterministic ray generation and damage remain sequential. Trigger leases, per-weapon ammunition/cooldowns, reload/equip cancellation, damage attribution and pursuit/arrest timers retain their rules. Spawn/respawn use an explicit loadout factory. Nested Weapon and Lifecycle remain Player payloads. Recovery and durable economic flows, ECS message ordering and deferred-command checkpoints retain their existing schedule.
