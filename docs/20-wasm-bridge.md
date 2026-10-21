# 20 — WASM prediction bridge

[Index](../README.md) · [Native motor](19-native-character-simulation.md) · [Client design](08-client-interfaces.md)

T06 packages the actual shared Rust motor for the browser. It does not introduce a playable scene, input sampling, networking or reconciliation. Those remain T07–T09. The server will call sim-core directly; TypeScript calls the same motor through sim-wasm.

## Build and run

Use the pinned Rust and Node toolchains from README, then install the ABI-matched generator once:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
npm run bridge:generate
npm run build:wasm
npm run dev
```

Open [localhost:5173/simulation](http://localhost:5173/simulation) and click **Run trace**. No Rust server is required. `npm run dev` runs contract generation and WASM packaging first. `npm run build` also packages WASM before type checking and Vite bundling; `npm run check` detects contract drift before building the WASM needed by integration tests. Use root scripts, not the client-only Vite build, for a clean checkout.

`tools/build-wasm.mjs` requires wasm-bindgen CLI **0.2.129**, matching the Rust crate exactly. It builds release wasm32-unknown-unknown and generates web-target JS/declarations/WASM into ignored `packages/sim-wasm/`. wasm-pack is not required. Vite resolves the generated module's WASM URL into a hashed production asset. Generated bindings use Symbol.dispose, so the consuming TS configurations include ESNext.Disposable; the client uses explicit `free()`.

## Rust boundary

`crates/sim-wasm/src/bridge.rs` owns Rust DTOs and the testable native byte adapter. `lib.rs` exposes only a thin wasm-bindgen class, `WasmPredictionWorld`:

| Method | Input / result | Effect |
|---|---|---|
| `load_collision(collision, config)` | Two MessagePack content DTOs | Validate and build one static BVH. Successful replacement clears the actor; failed replacement preserves the old world and actor. |
| `reset(frame)` | `PredictedFrame` | Restore complete character motor, tick, epoch and entity ID. Validate through a discarded shared-core candidate step; no tick advancement is committed and collision is retained. |
| `step(request)` | `StepRequest` → `PredictedFrame` bytes | Exactly one 1/30-second shared-core step, committed only after successful serialization. |
| `current_state()` | `PredictedFrame` bytes | Read an independent state copy. Requires successful reset. |
| `set_vehicle_proxies(set)` | `ProxySet` | Empty sets accepted; non-empty sets explicitly fail as unsupported until T10. |
| `raycast_static(query)` | `StaticRayQuery` → `StaticRayResult` bytes | Query vision-flagged static geometry with a normalized ray; miss is explicit `hit: null`. |
| `free()` | None | Release the WASM allocation. |

All local control DTOs have `version: 1`. `PredictedFrame` contains `{version, tick, control_epoch, controlled_entity_id, motor: CharacterMotor}`. `StepRequest` contains `{version, control_epoch, controlled_entity_id, input: InputFrame}`. Step requires the restored identity/epoch and **exactly current tick + 1**. Duplicate, skipped, stale identity/epoch and exhausted ticks fail without advancing. Reset may intentionally move the tick backwards or change identity/epoch for future reconciliation; the caller must authorize that reset from valid owner state. Tick zero is legal only as restored initial state.

This is a deliberate narrow refinement of document 08's proposed `state: EntityState`: T06 supports a character motor only. There is no invented vehicle or character gameplay state. Convert validated authoritative character owner state into the local bridge DTO when implementing T07–T09. The wire protocol and its version are unchanged.

MessagePack uses named maps and numeric arrays. Control buffers are capped at **16 KiB**, each collision/config buffer at **2 MiB**; decoding has a depth cap of 32 and rejects empty, malformed, trailing bytes, unknown/duplicate struct fields and typed range violations. The shared motor validates finite values, settings, geometry, input and overlaps. These are local byte messages, not accepted network envelopes. Call `loadCollision` with content already verified by the T04 loader; the bridge checks simulation safety but does not independently authenticate content hashes.

## TypeScript boundary and contracts

`apps/client/src/simulation/bridge.ts` wraps initialization and allocation ownership. `await PredictionWorld.create()` initializes asynchronously, sharing concurrent initialization. A failed load rejects with `BridgeError`; a later explicit retry may initialize again. `loadCollision`, `reset`, `step`, `currentState`, `setVehicleProxies`, and `raycastStatic` encode/decode the Rust-owned DTOs. Only the wrapper's `step(input)` fills the currently restored entity/epoch; no movement calculations exist in TS. `free()` is idempotent and further calls fail with BridgeError.

`xtask generate-bridge` generates separate bridge TS types and five JSON Schemas under `apps/client/src/simulation/generated/`. `bridge:check` compares without overwriting and rejects extra files. They import existing protocol motor/input/identity types but are not added to ClientMessage or ServerMessage. AJV2020 validates control DTOs on both sides of the JS wrapper. Content DTOs retain their existing content namespace. Native Rust still validates the underlying byte boundary if called without the wrapper.

No BVH construction occurs in reset, step, state or ray queries. Only `load_collision` builds it. Every diagnostic run releases its allocation in `finally`, including failure/cancellation. The page prevents concurrent runs and aborts on teardown; a later run creates a fresh world. WASM stays cached as a module across runs.

## Agreement and validation

`runCharacterTrace` imports the frozen T05 fixture, checks config/collision hashes and uses the **public wrapper and MessagePack one-step path** for 1,800 ticks. It loads collision once, resets once, compares 13 full motor checkpoints and checks a static wall ray. It requires position and velocity differences ≤0.01, yaw difference ≤0.001 rad, and exact grounded/cooldown fields. It yields every 300 ticks for cancellation. The fixture is not regenerated to match WASM.

Observed on 20 October 2026:

| Path | Result |
|---|---|
| Native trace CLI | 1,800 ticks / 13 checkpoints, zero measured position error |
| Release WASM through Node integration test | All frozen checkpoints within 1 cm |
| Live Vite browser page | PASS; maximum position error approximately 0.000002 m, velocity error rounds to 0.000000 m/s |
| Live production preview | Same PASS/error; mean wrapper serialization + simulation approximately 0.035 ms/tick |

The mean is a local diagnostic, not a frame-rate or capacity benchmark. It excludes initial WASM compilation and BVH construction. The measured boundary does not justify numeric-array/pointer optimization. The current production WASM is approximately **1,265 kB raw / 358 kB gzip**, loaded only by the simulation route. The map inspector's existing >500 kB JS chunk warning remains visible.

Six TS bridge tests include actual release WASM, invalid DTOs/buffers/ticks/identities, transactional failed reload/reset, restored epoch, ray hit/miss/invalid query, explicitly unsupported non-empty proxies, idempotent free and async initialization failure/retry. A native adapter test replays the byte path against exact native checkpoints. The existing 16 native motor tests remain unchanged.

## T07 handoff and limits

T07 must implement real admission/session/world ownership, load the baked package on the server, call sim-core at fixed tick rate, and validate input timing before simulation. Do not use the local bridge's tick checks as a substitute for authoritative server admission. The browser diagnostic never connects to the game server.

T08 supplies keyboard/mouse sampling, a rendered third-person character and camera. Its added `spherecast_static` accepts a generated StaticSphereQuery: version, origin, direction, max_distance, radius. It returns StaticRayResult with the sphere center at first contact. Radius must be finite and within 0.001–2 m; the camera uses 0.2 m plus a 0.1 m margin. The existing ray interface is preserved. Initial overlap returns distance zero. Native tests cover walls, grazing corners and overhead clearance; the release-WASM test compares sphere and ray distances against a fixture wall. T09 supplies rings/replay/reconciliation, and T10 supplies vehicle motors/proxies. There is no dynamic collision support or owned-car exclusion claim at this checkpoint.
