import { decodeServer, encodeClient } from "@port-meridian/protocol";
import type {
  ClientMessage,
  ServerEnvelope,
  CharacterMotor,
  InputFrame,
  OwnerState,
  PlayerStatus,
  MissionView,
  Action,
  WorldEvent,
} from "@port-meridian/protocol";
import { loadPackage, type LoadedPackage } from "@port-meridian/content";
import { BrowserTransport, type Channel } from "./transport";
import { ServerClock } from "./clock";
import { LocalPrediction } from "../simulation/prediction";
import { ReplicaStore } from "../world/replica";
import type { PredictionWorld } from "../simulation/bridge";

export interface SessionEvents {
  worldEvent?: (event: WorldEvent, own: boolean) => void;
  assets?: (assets: LoadedPackage, queries: PredictionWorld) => void;
  owner?: (owner: OwnerState) => void;
  playerStatus?: (status: PlayerStatus) => void;
  phase?: (phase: "loading" | "playing" | "closed") => void;
}
/** Own admission, protocol scope, readiness and authoritative samples. */
export class GameSession {
  private readonly transport = new BrowserTransport();
  private readonly abort = new AbortController();
  private hello: ServerEnvelope | undefined;
  private assets: LoadedPackage | undefined;
  private prediction: PredictionWorld | undefined;
  private ticket: string | undefined;
  private world: string | null = null;
  private session: string | null = null;
  private phase: "connecting" | "loading" | "playing" | "closed" = "connecting";
  private baseline = 0;
  private baselineOpen = false;
  private readonly shown = new Set<number>();
  readonly clock = new ServerClock();
  readonly replicas = new ReplicaStore();
  private local: LocalPrediction | undefined;
  private lastMessageAt = performance.now();
  private resyncAt = -Infinity;
  private resyncPending = false;
  private statusRevision = 0;
  private readonly seenEvents = new Set<number>();
  private mission: MissionView | null = null;
  private receivedBytes = 0;
  private sentBytes = 0;
  private rateAt = performance.now();
  private rateBytes = { received: 0, sent: 0 };
  private rates = { down: 0, up: 0 };
  private counter = 1;
  private readonly pending = new Map<
    number,
    { channel: Channel; started: number }
  >();
  private timer: ReturnType<typeof setInterval> | undefined;
  private movement: ReturnType<typeof setInterval> | undefined;
  private movementEnd: ReturnType<typeof setTimeout> | undefined;
  private deadline = performance.now() + 30_000;
  private readonly joinTimeout = setTimeout(
    () =>
      this.close("Loading/join timed out. Check the local server and retry."),
    30_000,
  );
  private reliable = 0;
  private unreliable = 0;
  private receipts = { reliable: -Infinity, unreliable: -Infinity };
  private reporting = false;
  private serverTick = 0;
  private tickReceivedAt = performance.now();
  private epoch = 0;
  private player = 0;
  private controlled = 0;
  private driving = false;
  private requestId = 0;
  private actionAt = -Infinity;
  actionMessage = "";
  private motor: CharacterMotor | undefined;
  private ownerLogAt = 0;
  private ownerTick = 0;
  constructor(
    private readonly log: (text: string) => void,
    private readonly status: (text: string) => void,
    private readonly events: SessionEvents = {},
  ) {
    this.transport.onBytes((channel, bytes) => this.receive(channel, bytes));
    this.transport.onClose((reason) => {
      this.phase = "closed";
      this.events.phase?.("closed");
      this.abort.abort();
      clearTimeout(this.joinTimeout);
      clearInterval(this.timer);
      clearInterval(this.movement);
      clearTimeout(this.movementEnd);
      this.prediction?.free();
      this.prediction = undefined;
      this.local = undefined;
      this.replicas.clear();
      this.pending.clear();
      this.session = null;
      this.status(`Closed: ${reason}`);
      this.log(`Closed: ${reason}`);
    });
  }
  private async api(path: string, body: unknown = {}): Promise<unknown> {
    const response = await fetch(path, {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.any([this.abort.signal, AbortSignal.timeout(5000)]),
    });
    if (!response.ok)
      throw new Error(`Local admission failed (${response.status})`);
    return response.json();
  }
  async connect(): Promise<void> {
    this.status("Loading verified collision + WASM…");
    try {
      const [assets, bridge] = await Promise.all([
        loadPackage(
          new URL("/content/city.json", location.href),
          this.abort.signal,
          fetch,
          { streamVisuals: true },
        ),
        import("../simulation/bridge"),
      ]);
      const prediction = await bridge.PredictionWorld.create();
      if (this.abort.signal.aborted) {
        prediction.free();
        return;
      }
      this.prediction = prediction;
      this.local = new LocalPrediction(
        prediction,
        this.clock,
        (reason) => this.requestResync(reason),
        assets.manifest.vehicle_templates,
        (tick, controlled) => {
          const proxies = this.replicas
            .sampleAll(tick, performance.now())
            .flatMap((actor) => {
              if (actor.id === controlled || actor.state.kind !== "vehicle")
                return [];
              const template = assets.manifest.vehicle_templates.find(
                (t) => t.id === actor.descriptor.template_id,
              );
              return template
                ? [
                    {
                      entity_id: actor.id,
                      template,
                      motor: {
                        ...actor.state.data.motor,
                        position: actor.motor.position,
                        yaw: actor.motor.yaw,
                      },
                    },
                  ]
                : [];
            });
          prediction.setVehicleProxies({ version: 1, proxies });
        },
        (id) => this.replicas.template(id),
      );
      this.assets = assets;
      prediction.loadCollision(assets.collision, assets.sim);
      this.events.assets?.(assets, prediction);
      await this.api("/api/session");
      const ticket = await this.api("/api/join-ticket");
      if (
        !ticket ||
        typeof ticket !== "object" ||
        !("join_ticket" in ticket) ||
        typeof ticket.join_ticket !== "string" ||
        !/^[0-9a-f]{64}$/.test(ticket.join_ticket) ||
        !("ws_url" in ticket) ||
        ticket.ws_url !== "ws://127.0.0.1:3000"
      )
        throw new Error("Invalid local join-ticket response");
      this.ticket = ticket.join_ticket;
      this.status("Connecting WS + RTC…");
      this.timer = setInterval(() => {
        const now = performance.now();
        if (this.phase !== "playing" && now > this.deadline) {
          this.close("Admission timed out. Retry joining.");
          return;
        }
        for (const [nonce, pending] of this.pending)
          if (now - pending.started > 3000) this.pending.delete(nonce);
        this.flushResync(now);
        if (this.session && now - this.lastMessageAt >= 10_000) {
          this.close("Connection lost. Retry joining.");
          return;
        }
        if (this.session) {
          this.ping("reliable");
          this.ping("unreliable");
        }
        if (
          this.phase === "playing" &&
          now - this.receipts.reliable < 2000 &&
          now - this.receipts.unreliable < 2000
        )
          void this.reportReadiness();
      }, 1000);
      await this.transport.connect(ticket.ws_url);
      this.log("RTC game channel open: unordered, maxRetransmits=0");
      this.sendHello();
    } catch (error) {
      if (!this.abort.signal.aborted)
        this.close(
          error instanceof Error ? error.message : "Connection failed",
        );
    }
  }
  private send(message: ClientMessage, channel: Channel = "reliable"): boolean {
    const bytes = encodeClient({
      v: 1,
      world_id: this.world,
      session_id: this.session,
      message,
    });
    const queued = this.transport.send(channel, bytes) === "queued";
    if (queued) this.sentBytes += bytes.length;
    return queued;
  }

  private sendHello(): void {
    if (
      !this.hello ||
      this.transport.state !== "ready" ||
      this.world ||
      !this.ticket ||
      !this.assets
    )
      return;
    if (this.hello.message.kind !== "server_hello")
      throw new Error("Expected hello");
    const info = this.hello.message.data;
    if (
      info.build_id !== "t14-local" ||
      info.content_hash !== this.assets.manifest.content_hash ||
      info.sim_config_hash !== this.assets.manifest.sim_config_hash
    )
      throw new Error("Server build/content mismatch; reload assets");
    this.world = this.hello.world_id;
    this.status("Authenticating one-use guest ticket…");
    this.send({
      kind: "client_hello",
      data: {
        protocol_version: 1,
        build_id: info.build_id,
        content_hash: info.content_hash,
        sim_config_hash: info.sim_config_hash,
        join_ticket: this.ticket,
        display_name: "Local Player",
      },
    });
    this.ticket = undefined;
  }
  private receive(channel: Channel, bytes: Uint8Array): void {
    const envelope = decodeServer(bytes, channel);
    const message = envelope.message;
    this.receivedBytes += bytes.length;
    if (message.kind === "server_hello") {
      if (this.hello || channel !== "reliable" || !envelope.world_id)
        throw new Error("Duplicate/invalid hello");
      this.hello = envelope;
      this.log("ServerHello: real content hashes / 30 Hz");
      this.sendHello();
      return;
    }
    if (!this.world || envelope.world_id !== this.world) return;
    if (this.session && envelope.session_id !== this.session) return;
    if (message.kind === "reject") {
      this.close(`Join rejected: ${message.data.code}`);
      return;
    }
    if (message.kind === "disconnect") {
      this.close(message.data.detail || message.data.code);
      return;
    }
    if (message.kind === "welcome") {
      if (this.session || !envelope.session_id || channel !== "reliable")
        throw new Error("Invalid welcome");
      this.session = envelope.session_id;
      this.lastMessageAt = performance.now();
      this.player = message.data.player_id;
      this.phase = "loading";
      this.events.phase?.("loading");
      this.observeTick(message.data.server_tick);
      this.log(
        `Welcome: player ${this.player}; profile ${message.data.profile.profile_id}; inert loading actor`,
      );
      return;
    }
    if (!this.session || envelope.session_id !== this.session) return;
    this.lastMessageAt = performance.now();
    switch (message.kind) {
      case "baseline_begin":
        this.phase = "loading";
        this.events.phase?.("loading");
        this.deadline = performance.now() + 30_000;
        this.baseline = message.data.baseline_id;
        this.resyncPending = false;
        this.baselineOpen = true;
        this.shown.clear();
        this.local?.stop();
        this.replicas.beginBaseline(
          this.clock.estimate(performance.now()) - 3,
          performance.now(),
        );
        this.stopMovement();
        this.log(`Baseline ${this.baseline} begin`);
        break;
      case "entity_show":
        if (
          message.data.baseline_id !== null &&
          (!this.baselineOpen || message.data.baseline_id !== this.baseline)
        )
          throw new Error("Wrong baseline show");
        if (this.baselineOpen && this.shown.has(message.data.descriptor.id))
          throw new Error("Duplicate visible entity");
        this.shown.add(message.data.descriptor.id);
        this.replicas.show(message.data, performance.now());
        break;
      case "entity_hide":
        this.shown.delete(message.data.entity_id);
        this.replicas.hide(message.data);
        break;
      case "baseline_end":
        if (
          !this.baselineOpen ||
          message.data.baseline_id !== this.baseline ||
          message.data.entity_count !== this.shown.size ||
          !this.shown.has(this.player)
        )
          throw new Error("Incomplete baseline");
        this.baselineOpen = false;
        this.replicas.finishBaseline();
        this.log(
          `Baseline ${this.baseline} complete: ${this.shown.size} entities; verified assets ready`,
        );
        this.send({
          kind: "client_ready",
          data: { baseline_id: this.baseline },
        });
        break;
      case "start_play": {
        if (
          this.phase !== "loading" ||
          message.data.baseline_id !== this.baseline ||
          message.data.owner.player_id !== this.player
        )
          throw new Error("Invalid StartPlay");
        const owner = message.data.owner;
        clearTimeout(this.joinTimeout);
        this.phase = "playing";
        this.events.phase?.("playing");
        this.epoch = owner.control_epoch;
        this.controlled = owner.controlled_entity_id;
        this.driving = owner.state.kind === "vehicle";
        this.ownerTick = owner.server_tick;
        this.motor = owner.player_state.motor;
        this.observeTick(owner.server_tick);
        this.local?.install(owner, performance.now());
        this.events.owner?.(owner);
        this.log(
          `StartPlay: player ${this.player}, epoch ${this.epoch}, server tick ${owner.server_tick}`,
        );
        this.ping("reliable");
        this.ping("unreliable");
        break;
      }
      case "player_status":
        if (message.data.revision < this.statusRevision) return;
        this.statusRevision = message.data.revision;
        this.mission = message.data.mission;
        this.events.playerStatus?.(message.data);
        this.log(
          `Private status baseline: credits ${message.data.credits}, revision ${message.data.revision}`,
        );
        break;
      case "owner_state":
        if (
          this.phase === "playing" &&
          message.data.control_epoch > this.epoch
        ) {
          this.local?.reconcile(message.data, performance.now());
          return;
        }
        if (
          this.phase !== "playing" ||
          message.data.control_epoch !== this.epoch ||
          message.data.player_id !== this.player ||
          message.data.controlled_entity_id !== this.controlled ||
          message.data.server_tick <= this.ownerTick
        )
          return;
        this.ownerTick = message.data.server_tick;
        this.local?.reconcile(message.data, performance.now());
        this.motor = message.data.player_state.motor;
        this.events.owner?.(message.data);
        this.observeTick(message.data.server_tick);
        if (performance.now() - this.ownerLogAt > 3000) {
          this.ownerLogAt = performance.now();
          this.log(
            `Owner tick ${this.serverTick}: feet ${this.motor.position.map((v) => v.toFixed(2)).join(", ")}`,
          );
        }
        this.showStatus();
        break;
      case "snapshot":
        if (this.phase === "playing")
          this.replicas.snapshot(message.data, performance.now());
        break;
      case "control_changed":
        if (
          this.phase !== "playing" ||
          message.data.player_id !== this.player ||
          message.data.control_epoch <= this.epoch
        )
          return;
        this.epoch = message.data.control_epoch;
        this.controlled = message.data.controlled_entity_id;
        this.driving = message.data.state.kind === "vehicle";
        this.ownerTick = message.data.server_tick;
        this.motor = message.data.player_state.motor;
        this.local?.install(message.data, performance.now());
        this.events.owner?.(message.data);
        break;
      case "pong": {
        const pending = this.pending.get(message.data.nonce);
        if (!pending) return;
        if (
          pending.channel !== channel ||
          message.data.client_time_ms !== pending.started
        )
          throw new Error("Probe response mismatch");
        this.pending.delete(message.data.nonce);
        this.clock.pong(
          message.data.server_tick,
          pending.started,
          performance.now(),
        );
        if (channel === "reliable") this.reliable++;
        else this.unreliable++;
        this.receipts[channel] = performance.now();
        this.observeTick(message.data.server_tick);
        this.showStatus();
        break;
      }
      case "world_event": {
        const event = message.data;
        if (!this.seenEvents.has(event.data.event_id)) {
          this.seenEvents.add(event.data.event_id);
          if (this.seenEvents.size > 256) {
            const oldest = this.seenEvents.values().next().value;
            if (oldest !== undefined) this.seenEvents.delete(oldest);
          }
          this.events.worldEvent?.(
            event,
            event.kind === "shot" && event.data.shooter_id === this.player,
          );
        }
        break;
      }
      case "action_result":
        this.actionMessage = message.data.ok
          ? ""
          : `Cannot interact: ${message.data.code.replaceAll("_", " ")}`;
        this.log(`Action: ${message.data.code}`);
        break;
      default:
        throw new Error("Unexpected server message");
    }
  }
  private observeTick(tick: number): void {
    this.clock.observe(tick, performance.now());
    if (tick < this.serverTick) return;
    this.serverTick = tick;
    this.tickReceivedAt = performance.now();
  }
  private showStatus(): void {
    if (this.phase === "playing")
      this.status(
        this.reliable && this.unreliable
          ? `PASS · player ${this.player} · epoch ${this.epoch} · tick ${this.serverTick} · WS ${this.reliable} · RTC ${this.unreliable}`
          : "Playing; checking both binary paths…",
      );
  }
  private async reportReadiness(): Promise<void> {
    if (this.reporting || !this.session) return;
    this.reporting = true;
    try {
      await this.api("/api/transport-probe", {
        session_id: this.session,
        ws: true,
        rtc: true,
      });
    } catch {
      if (!this.abort.signal.aborted) this.log("Readiness report unavailable");
    } finally {
      this.reporting = false;
    }
  }
  ping(channel: Channel): void {
    if (!this.session || this.pending.size >= 8) return;
    const nonce = this.counter++ + (channel === "unreliable" ? 0x8000_0000 : 0);
    const started = performance.now();
    if (
      this.send(
        { kind: "ping", data: { nonce, client_time_ms: started } },
        channel,
      )
    )
      this.pending.set(nonce, { channel, started });
  }
  requestResync(
    reason: "stale" | "overflow" | "invalid_state" = "stale",
  ): void {
    if (this.phase !== "playing") return;
    this.stopMovement();
    this.local?.stop();
    this.phase = "loading";
    this.resyncPending = true;
    this.deadline = performance.now() + 30_000;
    this.events.phase?.("loading");
    this.log(`Requesting a fresh baseline / control epoch (${reason})`);
    this.flushResync(performance.now(), reason);
  }
  private flushResync(
    now: number,
    reason: "stale" | "overflow" | "invalid_state" = "stale",
  ): void {
    if (!this.resyncPending || now - this.resyncAt < 5000) return;
    this.resyncAt = now;
    this.send({ kind: "resync_request", data: { reason } });
  }
  advance(now: number, sample: () => Omit<InputFrame, "tick">): void {
    this.flushResync(now);
    if (this.phase !== "playing") return;
    if (now - this.lastMessageAt >= 10_000) {
      this.close("Connection lost. Retry joining.");
      return;
    }
    if (now - this.lastMessageAt >= 2000) {
      this.local?.stop();
      return;
    }
    if (this.local?.stopped) {
      this.requestResync();
      return;
    }
    if (this.local?.advance(now, sample).length) {
      const frames = this.local.batch();
      if (frames.length)
        this.send(
          { kind: "input_batch", data: { control_epoch: this.epoch, frames } },
          "unreliable",
        );
    }
  }
  presentation(now: number): CharacterMotor | undefined {
    return this.local?.presentation(now);
  }
  get controlledVehicleTemplate() {
    return this.driving
      ? this.assets?.manifest.vehicle_templates.find(
          (t) => t.id === this.replicas.template(this.controlled),
        )
      : undefined;
  }
  get isDriving(): boolean {
    return this.driving;
  }
  private target(now: number) {
    const local = this.presentation(now);
    if (!local || !this.assets) return undefined;
    return this.remoteSamples(now)
      .filter((a) => a.state.kind === "vehicle")
      .map((a) => {
        const template = this.assets?.manifest.vehicle_templates.find(
          (t) => t.id === a.descriptor.template_id,
        );
        const offset = template?.exit_offsets[0] ?? [-1.5, 0, 0];
        const x =
          a.motor.position[0] +
          Math.cos(a.motor.yaw) * (offset[0] ?? 0) +
          Math.sin(a.motor.yaw) * (offset[2] ?? 0);
        const z =
          a.motor.position[2] -
          Math.sin(a.motor.yaw) * (offset[0] ?? 0) +
          Math.cos(a.motor.yaw) * (offset[2] ?? 0);
        return {
          actor: a,
          distance: Math.hypot(local.position[0] - x, local.position[2] - z),
        };
      })
      .filter((t) => t.distance <= 2)
      .sort((a, b) => a.distance - b.distance)[0]?.actor;
  }
  get currentMission(): MissionView | null {
    return this.mission;
  }
  estimatedTick(now: number): number {
    return this.clock.estimate(now);
  }
  private terminal(now: number) {
    const motor = this.presentation(now);
    if (!motor || !this.assets) return undefined;
    return this.assets.manifest.mission_templates.find((t) => {
      if (t.objective.kind !== "courier") return false;
      const zone = this.assets?.manifest.zones.find(
        (z) => z.id === t.terminal_zone_id,
      );
      return (
        zone &&
        Math.hypot(
          motor.position[0] - zone.center[0],
          motor.position[2] - zone.center[2],
        ) <= Math.min(3, zone.radius)
      );
    });
  }
  private requestAction(action: Action): void {
    const now = performance.now();
    if (this.phase !== "playing" || now - this.actionAt < 250) return;
    this.actionAt = now;
    this.actionMessage = "";
    this.send({
      kind: "action",
      data: { request_id: ++this.requestId, control_epoch: this.epoch, action },
    });
  }
  fire(yaw: number, pitch: number): void {
    if (!this.driving)
      this.requestAction({
        kind: "fire",
        data: { aim_yaw: yaw, aim_pitch: pitch },
      });
  }
  reload(): void {
    if (!this.driving) this.requestAction({ kind: "reload", data: {} });
  }
  useTerminal(): void {
    const template = this.terminal(performance.now());
    if (template)
      this.requestAction({
        kind: "accept_mission",
        data: { template_id: template.id },
      });
  }
  cancelMission(): void {
    this.requestAction({ kind: "cancel_mission", data: {} });
  }
  interactionPrompt(now: number): string {
    const terminal = this.terminal(now);
    const job =
      terminal && !this.mission ? " · F accept courier · 150 credits" : "";
    if (this.driving)
      return `E exit when stopped · WASD drive · Space handbrake${job}`;
    const target = this.target(now);
    return target?.state.kind === "vehicle"
      ? target.state.data.disabled || target.state.data.health === 0
        ? "Vehicle disabled · awaiting a clear parking respawn"
        : target.state.data.driver_id
          ? "Vehicle occupied"
          : `E enter ${target.descriptor.template_id} · Car health ${target.state.data.health}${job}`
      : terminal && !this.mission
        ? "E / F accept courier · 150 credits"
        : "Walk to the left door of a parked car · E enter";
  }
  interact(): void {
    const now = performance.now();
    if (this.phase !== "playing" || now - this.actionAt < 250) return;
    const target = this.target(now);
    if (!this.driving && !target) {
      this.useTerminal();
      return;
    }
    const targetId = target?.id ?? 0;
    this.actionAt = now;
    this.actionMessage = "";
    this.send({
      kind: "action",
      data: {
        request_id: ++this.requestId,
        control_epoch: this.epoch,
        action: this.driving
          ? { kind: "exit_vehicle", data: {} }
          : { kind: "enter_vehicle", data: { vehicle_id: targetId } },
      },
    });
  }
  remoteSamples(now: number) {
    const actors = this.replicas
      .sampleAll(this.clock.estimate(now) - 3, now)
      .filter((r) => r.id !== this.player);
    const controlled = actors.find((a) => a.id === this.controlled);
    const predicted = this.presentation(now);
    if (controlled && predicted) controlled.motor = predicted;
    for (const actor of actors) {
      if (
        actor.state.kind !== "character" ||
        actor.state.data.mode !== "driving"
      )
        continue;
      const vehicleId = actor.state.data.vehicle_id;
      const car = actors.find((a) => a.id === vehicleId);
      if (!car) continue;
      const template = this.assets?.manifest.vehicle_templates.find(
        (t) => t.id === car.descriptor.template_id,
      );
      const offset = template?.seat_offset ?? [0, 0.8, -0.25];
      const [x, y, z] = offset;
      actor.motor = {
        ...car.motor,
        position: [
          car.motor.position[0] +
            Math.cos(car.motor.yaw) * (x ?? 0) +
            Math.sin(car.motor.yaw) * (z ?? 0),
          car.motor.position[1] + (y ?? 0),
          car.motor.position[2] -
            Math.sin(car.motor.yaw) * (x ?? 0) +
            Math.cos(car.motor.yaw) * (z ?? 0),
        ],
      };
    }
    return actors;
  }
  metrics(now: number) {
    if (now - this.rateAt >= 1000) {
      const seconds = (now - this.rateAt) / 1000;
      this.rates = {
        down: (this.receivedBytes - this.rateBytes.received) / seconds,
        up: (this.sentBytes - this.rateBytes.sent) / seconds,
      };
      this.rateBytes = { received: this.receivedBytes, sent: this.sentBytes };
      this.rateAt = now;
    }
    return {
      player: this.player,
      epoch: this.epoch,
      tick: this.serverTick,
      predicted: this.local?.tick ?? 0,
      lead: this.clock.lead,
      rtt: this.clock.rtt,
      correction: this.local?.correction ?? 0,
      p95: this.local?.p95 ?? 0,
      replay: this.local?.replaySteps ?? 0,
      history: this.local?.history.size ?? 0,
      remotes: this.remoteSamples(now).filter(
        (a) => a.descriptor.entity_kind === "player",
      ).length,
      stale: now - this.lastMessageAt > 500,
      ...this.rates,
    };
  }
  /** Bounded scripted input test; gameplay keyboard/mouse controls are T08. */
  probeMovement(): void {
    if (
      this.phase !== "playing" ||
      this.movement ||
      this.movementEnd ||
      !this.motor
    )
      return;
    const start = [...this.motor.position];
    let tick =
      this.serverTick +
      Math.floor(((performance.now() - this.tickReceivedAt) * 30) / 1000) +
      3;
    let remaining = 30;
    this.log(
      "Movement probe: scheduling 30 forward inputs; server applies one per tick",
    );
    this.movement = setInterval(() => {
      if (this.phase !== "playing") {
        this.stopMovement();
        return;
      }
      this.send(
        {
          kind: "input_batch",
          data: {
            control_epoch: this.epoch,
            frames: [
              {
                tick: tick++,
                move_x: 0,
                move_z: 32767,
                look_yaw: 0,
                look_pitch: 0,
                buttons: 0,
              },
            ],
          },
        },
        "unreliable",
      );
      if (--remaining === 0) {
        clearInterval(this.movement);
        this.movement = undefined;
        this.movementEnd = setTimeout(() => {
          this.movementEnd = undefined;
          if (this.phase !== "playing" || !this.motor) return;
          const moved = Math.hypot(
            ...this.motor.position.map((v, i) => v - (start[i] ?? v)),
          );
          this.log(
            `Movement probe complete: server reports ${moved.toFixed(3)} m displacement; missing inputs brake to neutral`,
          );
        }, 500);
      }
    }, 1000 / 30);
  }
  private stopMovement(): void {
    clearInterval(this.movement);
    clearTimeout(this.movementEnd);
    this.movement = undefined;
    this.movementEnd = undefined;
  }
  close(reason = "Left diagnostic"): void {
    if (this.session && this.transport.state === "ready")
      this.send({ kind: "leave", data: {} });
    this.transport.close(reason);
  }
}
