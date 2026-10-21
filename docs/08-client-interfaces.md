# 08 — Client interfaces

[Index](../README.md)

## Application state

```ts
type AppPhase =
  | "boot" | "menu" | "loading_assets" | "connecting"
  | "authenticating" | "loading_world" | "playing"
  | "resyncing" | "disconnected" | "fatal";
```

`GameApp` owns phase transitions and top-level services. Modules report events; they do not navigate independently or manipulate another module’s internal state.

Boot checks WebGL2, WebAssembly, WebSocket, and RTCPeerConnection support. Menu obtains guest session/ticket on Play, not on page load. Warm essential assets before connecting. A failed join returns to a screen with one retry action and a concrete reason; never leave a spinner with no timeout.

## Module boundaries

| Module | Owns | May read | Must not own |
|---|---|---|---|
| `net/transport.ts` | WS, RTC, signalling, timers, byte queues | endpoint config | game state |
| `net/session.ts` | envelope IDs, join phase, pending actions | generated protocol | camera/meshes |
| `world/replica.ts` | authoritative remote samples and descriptors | protocol | local prediction |
| `simulation/prediction.ts` | input ring, WASM world, reconciliation | replica proxies, content | authoritative economy |
| `input/input.ts` | keys, pointer delta, action edges | settings, app phase | network sends |
| `render/renderer.ts` | scene, lights, meshes, resource references | presentation frame | server state mutation |
| `render/camera.ts` | camera orbit/springs/collision | followed pose, static queries | player movement |
| `content/loader.ts` | manifests, hashes, GLB cache, load queues | URL configuration | world entity creation |
| `ui/hud.ts` | DOM overlay and local UI state | read-only HUD model | Three.js internals |
| `audio/audio.ts` | audio context, pooled voices | cosmetic events/poses | game decisions |

Use typed callbacks or a small explicit event dispatcher. Avoid a global mutable object accessible to all modules.

## Network interfaces

```ts
type TransportState = "idle" | "connecting" | "ready" | "closed";
type Channel = "reliable" | "unreliable";
type SendResult = "queued" | "backpressured" | "closed";
interface BrowserTransport {
  readonly state: TransportState;
  connect(url: string): Promise<void>;
  send(channel: Channel, bytes: Uint8Array): SendResult;
  onBytes(handler: (channel: Channel, bytes: Uint8Array) => void): () => void;
  onClose(handler: (reason: string) => void): () => void;
  close(reason: string): void;
}
interface ProtocolCodec {
  encodeClient(envelope: ClientEnvelope): Uint8Array;
  decodeServer(bytes: Uint8Array): ServerEnvelope; // throws typed DecodeError
}
interface GameSession {
  join(ticket: string, displayName: string): Promise<WelcomeData>;
  ready(baselineId: number): void;
  sendInputs(epoch: number, frames: InputFrame[]): SendResult;
  request(action: Action, epoch: number): Promise<ActionResultData>;
  requestResync(reason: ResyncReason): void;
  leave(): void;
}
```

`connect()` resolves when RTC is open, but GameSession still waits for ServerHello. Install onBytes before calling connect so the hello cannot race listener installation. Binary callbacks are processed in arrival order on the main thread; signalling maintains its separate serialized async chain.

GameSession owns request IDs and results. Action timeout is 3 seconds: mark “awaiting server,” query/resync if necessary, and never automatically issue a new ID for a possibly applied purchase/mission action. A transport close rejects all pending promises and clears state.

On reconnect obtain a new ticket and repeat full join. Never reuse a previous session ID, action counter, visibility set, or control epoch.

## Replica and presentation interfaces

```ts
interface ReplicaStore {
  beginBaseline(id: number): void;
  show(data: EntityShowData): void;
  hide(data: EntityHideData): void;
  applySnapshot(data: SnapshotData): void;
  finishBaseline(id: number, expectedCount: number): void;
  sample(entityId: number, renderTick: number): RenderPose | null;
  descriptors(): Iterable<EntityDescriptor>;
  clear(): void;
}
interface PresentationFrame {
  nowSeconds: number;
  local: RenderActor;
  remotes: readonly RenderActor[];
  hud: HudModel;
  effects: readonly CosmeticEffect[];
}
interface GameRenderer {
  mount(canvas: HTMLCanvasElement): void;
  render(frame: PresentationFrame, camera: CameraPose): void;
  resize(width: number, height: number, pixelRatio: number): void;
  dispose(): void;
}
```

ReplicaStore checks visibility IDs and state ticks before retaining samples. It allocates no meshes. Renderer observes descriptor additions/removals and acquires/releases pooled assets. Destroying a render object must not delete a replica unless a proper lifecycle message says so.

Do not interpolate private status. HUD uses latest authoritative status and may display local “pending” interactions separately. Cash changes only after PlayerStatus; a locally displayed estimated reward is labeled pending.

## WASM bridge

T06 is implemented and verified; [20 — WASM bridge](20-wasm-bridge.md) owns its concrete API and build/trace instructions. The bridge currently supports one character motor; vehicle proxies are reserved until T10.

Keep the bridge small. Rust/WASM executes the same `sim-core` code used by native. Use MessagePack byte buffers across the JS/WASM boundary initially; generated types and fixtures apply here too, but these are **local bridge messages**, not additional network messages.

```ts
interface WasmPredictionWorld {
  // Input blobs are encoded typed DTOs; errors return typed JS exceptions.
  load_collision(collisionBlob: Uint8Array, simConfigBlob: Uint8Array): void;
  set_vehicle_proxies(proxiesBlob: Uint8Array): void;
  reset(ownerStateBlob: Uint8Array): void;
  step(inputBlob: Uint8Array): Uint8Array; // PredictedFrame, exactly one DT
  current_state(): Uint8Array;
  raycast_static(queryBlob: Uint8Array): Uint8Array; // camera obstruction
  free(): void;
}
type PredictedFrame = {
  version: 1; tick: number; control_epoch: number;
  controlled_entity_id: number; motor: CharacterMotor;
};
type StepRequest = {
  version: 1; control_epoch: number; controlled_entity_id: number;
  input: InputFrame; // exactly current tick + 1
};
type ProxySet = { version: 1; proxies: VehicleProxy[] };
type VehicleProxy = {
  entity_id: number; template_id: string; motor: VehicleMotor;
};
```

The loader fetches validated static collision bytes once; WASM builds query structures once. `reset` resets motor state without rebuilding static collision. T06 accepts only an empty versioned ProxySet and explicitly rejects non-empty proxies. T10 must exclude the owned car from proxy collision and update only proxies that changed; its public interface can accept the whole capped set initially. T08 adds `spherecast_static` with a generated StaticSphereQuery (the ray fields plus `radius`) returning StaticRayResult; the radius query below is implemented against the same immutable BVH.

Expose the bridge’s DTOs through the same generator, under a separate `bridge` namespace. Do not mix them into ClientMessage/ServerMessage. If buffer encode/decode takes > 1 ms per tick on the target laptop, measure and replace this boundary with typed numeric arrays as a separate task; do not preemptively build unsafe pointer plumbing.

## Main loop

For each animation frame:

1. Process bounded queued server messages and install lifecycle/control/status changes.
2. Update estimated server time and determine fixed prediction steps owed.
3. Sample input for each owed 30 Hz step; create a tick-tagged frame, update local prediction, and append to send ring.
4. At 30 Hz network cadence send the latest three frames. Process reliable action edges once; clear edge flags after use.
5. Apply any pending OwnerState reconciliation before deriving the final render frame.
6. Sample remote poses at interpolation time. Build the local render pose with any decaying correction offset.
7. Update camera and visible chunk requests, then renderer/audio/HUD.

Limit prediction catch-up to 5 steps/frame. A long background pause forces neutral/resync, not hundreds of replay steps. Never tie gameplay simulation speed to FPS.

## Third-person camera

Default walking camera: orbit/collision pivot feet + `[0,1.45,0]`, look target feet + `[0,1.8,0]` (head near the screen-centre cross; body in the lower half), distance 4.5 m, horizontal offset 0 m (centred on the player, per T08 feedback), vertical pitch default −0.15 rad under an explicitly tested camera convention. Keep pitch implementation consistent with screen look; camera pitch is separate from gameplay aim if converted.

Driving: target car bottom-center + `[0,1.1,0]`, distance 7 m at rest growing to 9 m at top speed, FOV 65° to 72°. On-foot FOV 65°, aiming FOV 55° and distance 2.2 m. Reduced-motion mode disables speed FOV changes and impact shake.

Spherecast a 0.2 m camera radius from target toward desired camera position against static world and non-owned car geometry. Clamp camera in front of nearest obstruction with a 0.1 m margin. Shorten immediately to avoid walls; ease outward over 0.2 s. Fade owned character when camera distance < 0.8 m. Do not move the player to solve camera clipping.

Mouse movement rotates orbit immediately. Input movement basis uses that camera yaw. In driving mode recenter toward car yaw after 1.5 s without mouse input; retain manual pitch within bounds. Camera must work against a wall, under an awning, after entering/exiting, and when respawning.

## Input and action selection

Input normalization happens in one module. Press E selects:

1. Exit if driving.
2. Nearest eligible car within 2 m, breaking ties by entity ID.
3. Otherwise, nearest job terminal on foot within its zone.

F explicitly interacts with the nearest job terminal or repair zone on foot or while driving; repairs still require driving. This lets a driver start a time trial or repair without conflicting with E-to-exit. When zones overlap, prefer the active repair zone if driving a damaged car, then the nearest terminal, breaking ties by zone ID. Show the bound key beside each prompt.

Show exactly the prompt that would be requested. This is a client hint; server repeats all checks. Clicking UI must not fire the pistol. Space must not scroll the page while pointer lock and gameplay are active. Prevent default only for bound game keys, preserving ordinary text input in menus.

## Failure handling

Content hash mismatch → reload-required screen. Missing essential collision/WASM → retry assets. ICE timeout → connection failed with retry and “network may block the required connection.” No automatic claim that falling back to WebSocket will work.

On WebGL context loss pause presentation/input, show recovery state, and attempt resource recreation; if recovery fails, offer reload. Always remove event listeners, free WASM, dispose GPU resources, abort fetches, and close audio/transport on app teardown.

## Client acceptance

- Pointer-lock loss and hidden tab produce neutral input.
- Two join/leave cycles leave one socket, one peer connection, one rAF loop, and no duplicate handlers.
- A malformed server message cannot mutate partially validated state.
- Camera never moves through a tested solid wall.
- HUD correctly distinguishes loading, pending action, server rejection, stale connection, and confirmed success.
- Quality changes apply without reconnecting; simulation DT remains 1/30.
