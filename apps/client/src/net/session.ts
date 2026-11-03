import { decodeServer, encodeClient } from "@port-meridian/protocol";
import type {
  ClientMessage,
  ChatLine,
  ServerEnvelope,
  CharacterMotor,
  InputFrame,
  OwnerState,
  PlayerStatus,
  MissionView,
  RepairView,
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
  chat?: (line: ChatLine) => void;
  chatError?: (text: string) => void;
  tshirtColour?: (colour: number) => void;
  appearanceError?: () => void;
  dryFire?: (weapon: "pistol" | "rifle" | "machine_gun") => void;
  worldEvent?: (event: WorldEvent, own: boolean) => void;
  assets?: (assets: LoadedPackage, queries: PredictionWorld) => void;
  owner?: (owner: OwnerState) => void;
  playerStatus?: (status: PlayerStatus) => void;
  phase?: (phase: "loading" | "playing" | "closed") => void;
}
/** Own admission, protocol scope, readiness and authoritative samples. */
export class GameSession {
  private readonly transport = new BrowserTransport();
  private readonly startedAt = performance.now();
  private joinMs: number | undefined;
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
  private baselineReady = false;
  private readonly shown = new Set<number>();
  readonly clock = new ServerClock();
  readonly replicas = new ReplicaStore();
  private local: LocalPrediction | undefined;
  private predictionWorkMs = 0;
  private measurePrediction<T>(work: () => T): T {
    const started = performance.now();
    try {
      return work();
    } finally {
      this.predictionWorkMs += performance.now() - started;
    }
  }
  takePredictionWork(): number {
    const ms = this.predictionWorkMs;
    this.predictionWorkMs = 0;
    return ms;
  }
  private lastMessageAt = performance.now();
  private resyncAt = -Infinity;
  private resyncPending = false;
  private statusRevision = 0;
  private readonly seenEvents = new Set<number>();
  private repair: RepairView | null = null;
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
  private requestedColour: number | undefined;
  private savedColour: number;
  private savingColour = false;
  private requestId = 0;
  private readonly weaponBloom = new Map<
    string,
    { value: number; at: number }
  >();
  spreadDegrees(moving: boolean, now = performance.now()): number {
    const d = this.assets?.manifest.weapons.find(
      (w) => w.id === this.equippedWeapon,
    );
    if (!d) return 0.6;
    const bloom = this.weaponBloom.get(this.equippedWeapon);
    return (
      d.spread_degrees * (moving ? d.moving_multiplier : 1) +
      Math.max(
        0,
        (bloom?.value ?? 0) -
          ((now - (bloom?.at ?? now)) / 1000) * d.bloom_recovery,
      )
    );
  }
  private weaponStatus: PlayerStatus | undefined;
  private triggerSequence = 0;
  private triggerAt = -Infinity;
  private triggerHeld = false;
  private triggerNeedsRelease = false;
  private lastDry = -Infinity;
  private fireAt = -Infinity;
  get weaponCapacity(): number {
    return (
      this.assets?.manifest.weapons.find((w) => w.id === this.equippedWeapon)
        ?.capacity ?? 12
    );
  }
  weaponSummary(now: number): string {
    const s = this.weaponStatus;
    if (!s) return "Pistol · 12 / 36";
    const names = {
      pistol: "Pistol",
      rifle: "Rifle",
      machine_gun: "Machine gun",
    };
    const slot = s.inventory.find((w) => w.weapon_id === s.equipped_weapon);
    const d = this.assets?.manifest.weapons.find(
      (w) => w.id === s.equipped_weapon,
    );
    const until = s.reload_end_tick ?? s.equip_end_tick;
    const duration =
      s.reload_end_tick !== null ? d?.reload_ticks : d?.equip_ticks;
    const progress =
      until !== null && duration
        ? ` · ${s.reload_end_tick !== null ? "Reloading" : "Equipping"} ${Math.round(Math.max(0, Math.min(1, 1 - (until - this.estimatedTick(now)) / duration)) * 100)}%`
        : "";
    return `${names[s.equipped_weapon]} · ${slot?.magazine ?? 0} / ${slot?.reserve ?? 0}${progress} · ${s.inventory.map((w, i) => `${i + 1}:${w.owned ? names[w.weapon_id] : "—"}`).join("  ")}`;
  }
  selectWeapon(slot: number): void {
    const id = this.weaponStatus?.inventory[slot];
    if (id?.owned) this.equipWeapon(id.weapon_id);
  }
  cycleWeapon(direction: number): void {
    const owned = this.weaponStatus?.inventory.filter((s) => s.owned) ?? [];
    if (!owned.length) return;
    const i = owned.findIndex((s) => s.weapon_id === this.equippedWeapon);
    const next = owned[(i + direction + owned.length) % owned.length];
    if (next) this.equipWeapon(next.weapon_id);
  }
  updateTrigger(
    held: boolean,
    yaw = 0,
    pitch = 0,
    now = performance.now(),
  ): void {
    if (!held) this.triggerNeedsRelease = false;
    if (
      held &&
      !this.triggerNeedsRelease &&
      this.equippedWeapon === "machine_gun" &&
      this.weaponStatus?.magazine === 0
    )
      this.dryFire(now);
    held =
      held &&
      !this.triggerNeedsRelease &&
      !this.driving &&
      this.phase === "playing" &&
      this.equippedWeapon === "machine_gun";
    if (held === this.triggerHeld && (!held || now - this.triggerAt < 100))
      return;
    const pressed = held && !this.triggerHeld;
    this.triggerHeld = held;
    this.triggerAt = now;
    if (this.phase === "playing")
      this.send({
        kind: "trigger",
        data: {
          control_epoch: this.epoch,
          sequence: ++this.triggerSequence,
          held,
          pressed,
          weapon_id: this.equippedWeapon,
          aim_yaw: yaw,
          aim_pitch: pitch,
        },
      });
  }
  private stopTrigger(): void {
    this.updateTrigger(false);
    this.triggerNeedsRelease = true;
  }
  private equippedWeapon: "pistol" | "rifle" | "machine_gun" = "pistol";
  private actionAt = -Infinity;
  actionMessage = "";
  private motor: CharacterMotor | undefined;
  private ownerLogAt = 0;
  private ownerTick = 0;
  constructor(
    private readonly log: (text: string) => void,
    private readonly status: (text: string) => void,
    private readonly events: SessionEvents = {},
    private readonly displayName = "Local Player",
    private readonly tshirtColour = 0xe4a25b,
  ) {
    this.savedColour = tshirtColour;
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
  /** Pausing only releases input; the live session can still save cosmetic changes. */
  updateTshirtColour(colour: number): void {
    if (
      this.phase !== "playing" ||
      !Number.isInteger(colour) ||
      colour < 0 ||
      colour > 0xffffff
    )
      return;
    this.requestedColour = colour;
    if (!this.savingColour) void this.saveTshirtColour();
  }
  private async saveTshirtColour(): Promise<void> {
    this.savingColour = true;
    try {
      // Coalesce colour-picker drags; serialize writes so the last selection wins.
      while (this.requestedColour !== undefined && this.phase === "playing") {
        const colour = this.requestedColour;
        this.requestedColour = undefined;
        if (colour === this.savedColour) continue;
        const result = await this.api("/api/appearance", {
          tshirt_colour: colour,
        });
        if (
          !result ||
          typeof result !== "object" ||
          !("profile" in result) ||
          !result.profile ||
          typeof result.profile !== "object" ||
          !("tshirt_colour" in result.profile) ||
          result.profile.tshirt_colour !== colour
        )
          throw new Error("Appearance save was not confirmed");
        this.savedColour = colour;
      }
    } catch (error) {
      if (!this.abort.signal.aborted) {
        this.log(`T-shirt colour save failed: ${String(error)}`);
        this.events.appearanceError?.();
      }
    } finally {
      this.savingColour = false;
    }
  }
  sendChat(text: string): boolean {
    return this.phase === "playing" && this.send({ kind: "chat", data: text });
  }
  private async api(path: string, body: unknown = {}): Promise<unknown> {
    const deadline = performance.now() + 5000;
    let response: Response;
    do {
      response = await fetch(path, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: AbortSignal.any([
          this.abort.signal,
          AbortSignal.timeout(
            Math.max(1, Math.ceil(deadline - performance.now())),
          ),
        ]),
      });
      if (response.status !== 202) break;
      await response.body?.cancel();
      if (performance.now() >= deadline)
        throw new Error("Profile storage timed out. Retry joining.");
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(done, 100);
        const abort = () => {
          clearTimeout(timer);
          reject(this.abort.signal.reason);
        };
        const signal = this.abort.signal;
        function done() {
          signal.removeEventListener("abort", abort);
          resolve();
        }
        signal.addEventListener("abort", abort, { once: true });
        if (signal.aborted) abort();
      });
    } while (response.status === 202);
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
        (tick, controlled, omitVehicles = false) => {
          if (omitVehicles) {
            prediction.setVehicleProxies({ version: 1, proxies: [] });
            return;
          }
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
        (ticket.ws_url !== "ws://127.0.0.1:3000" &&
          ticket.ws_url !== `wss://${location.host}/ws`)
      )
        throw new Error("Invalid join-ticket response");
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
      v: 4,
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
      info.build_id !== "protocol-simple-local" ||
      info.content_hash !== this.assets.manifest.content_hash ||
      info.sim_config_hash !== this.assets.manifest.sim_config_hash
    )
      throw new Error("Server build/content mismatch; reload assets");
    this.world = this.hello.world_id;
    this.status("Authenticating one-use guest ticket…");
    this.send({
      kind: "client_hello",
      data: {
        protocol_version: 4,
        build_id: info.build_id,
        content_hash: info.content_hash,
        sim_config_hash: info.sim_config_hash,
        join_ticket: this.ticket,
        display_name: this.displayName,
        tshirt_colour: this.tshirtColour,
      },
    });
    this.ticket = undefined;
  }
  private receive(channel: Channel, bytes: Uint8Array): void {
    if (this.phase === "closed") return;
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
      this.savedColour = message.data.profile.tshirt_colour;
      this.events.tshirtColour?.(message.data.profile.tshirt_colour);
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
        this.stopTrigger();
        if (this.baselineOpen || message.data.baseline_id <= this.baseline)
          throw new Error("Duplicate or overlapping baseline");
        this.baselineReady = false;
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
          this.baselineOpen
            ? message.data.baseline_id !== this.baseline
            : message.data.baseline_id !== null
        )
          throw new Error("Wrong baseline show");
        if (this.baselineOpen && this.shown.has(message.data.descriptor.id))
          throw new Error("Duplicate visible entity");
        this.shown.add(message.data.descriptor.id);
        this.replicas.show(message.data, performance.now());
        if (
          message.data.descriptor.id === this.player &&
          message.data.descriptor.tshirt_colour != null
        ) {
          this.savedColour = message.data.descriptor.tshirt_colour;
          this.events.tshirtColour?.(message.data.descriptor.tshirt_colour);
        }
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
        this.baselineReady = true;
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
          !this.baselineReady ||
          message.data.baseline_id !== this.baseline ||
          message.data.owner.player_id !== this.player
        )
          throw new Error("Invalid StartPlay");
        const owner = message.data.owner;
        clearTimeout(this.joinTimeout);
        this.joinMs ??= performance.now() - this.startedAt;
        this.phase = "playing";
        this.events.phase?.("playing");
        this.epoch = owner.control_epoch;
        this.controlled = owner.controlled_entity_id;
        this.driving = owner.state.kind === "vehicle";
        this.ownerTick = owner.server_tick;
        this.motor = owner.player_state.motor;
        this.observeTick(owner.server_tick);
        this.measurePrediction(() =>
          this.local?.install(owner, performance.now()),
        );
        this.events.owner?.(owner);
        this.log(
          `StartPlay: player ${this.player}, epoch ${this.epoch}, server tick ${owner.server_tick}`,
        );
        this.ping("reliable");
        this.ping("unreliable");
        break;
      }
      case "chat":
        this.events.chat?.(message.data);
        break;
      case "chat_error":
        this.events.chatError?.(message.data);
        break;
      case "player_status":
        if (message.data.revision < this.statusRevision) return;
        if (
          message.data.equipped_weapon !== this.equippedWeapon ||
          message.data.reload_end_tick !== null ||
          message.data.equip_end_tick !== null ||
          message.data.respawn_tick !== null
        )
          this.stopTrigger();

        this.weaponStatus = message.data;
        this.equippedWeapon = message.data.equipped_weapon;
        this.statusRevision = message.data.revision;
        this.mission = message.data.mission;
        this.repair = message.data.repair;
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
          this.measurePrediction(() =>
            this.local?.reconcile(message.data, performance.now()),
          );
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
        this.measurePrediction(() =>
          this.local?.reconcile(message.data, performance.now()),
        );
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
        this.stopTrigger();
        this.epoch = message.data.control_epoch;
        this.controlled = message.data.controlled_entity_id;
        this.driving = message.data.state.kind === "vehicle";
        this.ownerTick = message.data.server_tick;
        this.motor = message.data.player_state.motor;
        this.measurePrediction(() =>
          this.local?.install(message.data, performance.now()),
        );
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
          if (event.kind === "shot" && event.data.shooter_id === this.player) {
            const now = performance.now();
            const d = this.assets?.manifest.weapons.find(
              (w) => w.id === event.data.weapon_id,
            );
            if (d) {
              const previous = this.weaponBloom.get(d.id);
              this.weaponBloom.set(d.id, {
                value: Math.min(
                  d.bloom_max,
                  Math.max(
                    0,
                    (previous?.value ?? 0) -
                      ((now - (previous?.at ?? now)) / 1000) * d.bloom_recovery,
                  ) + d.bloom_per_shot,
                ),
                at: now,
              });
            }
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
    this.baselineReady = false;
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
    if (this.measurePrediction(() => this.local?.advance(now, sample).length)) {
      const frames = this.local?.batch() ?? [];
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
      .filter(
        (a) =>
          a.state.kind === "vehicle" && a.descriptor.entity_kind !== "police",
      )
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
  get currentRepair(): RepairView | null {
    return this.repair;
  }
  nearbyJobs(now: number) {
    const motor = this.presentation(now);
    if (!motor || !this.assets || this.mission) return [];
    if (
      this.repair &&
      ["dwell", "debit_pending", "refund_pending"].includes(this.repair.phase)
    )
      return [];
    return this.assets.manifest.mission_templates.filter((t) =>
      this.assets?.manifest.zones.some(
        (z) =>
          z.kind === "terminal" &&
          z.allowed_templates.includes(t.id) &&
          Math.hypot(
            motor.position[0] - z.center[0],
            motor.position[2] - z.center[2],
          ) <= Math.min(3, z.radius),
      ),
    );
  }

  private terminal(now: number) {
    return this.nearbyJobs(now)[0];
  }
  repairZone(now: number) {
    const motor = this.presentation(now);
    return this.driving && motor
      ? this.assets?.manifest.zones.find(
          (z) =>
            z.kind === "repair" &&
            Math.hypot(
              motor.position[0] - z.center[0],
              motor.position[2] - z.center[2],
            ) <= z.radius,
        )
      : undefined;
  }
  acceptJob(templateId: string): void {
    this.requestAction({
      kind: "accept_mission",
      data: { template_id: templateId },
    });
  }
  private requestAction(action: Action): void {
    const now = performance.now();
    if (
      this.phase !== "playing" ||
      (action.kind !== "fire" && now - this.actionAt < 250)
    )
      return;
    if (action.kind !== "fire") this.actionAt = now;
    this.actionMessage = "";
    this.send({
      kind: "action",
      data: { request_id: ++this.requestId, control_epoch: this.epoch, action },
    });
  }
  private dryFire(now: number): void {
    if (now - this.lastDry <= 500 || this.phase !== "playing" || this.driving)
      return;
    this.lastDry = now;
    this.actionMessage = "No ammunition";
    this.events.dryFire?.(this.equippedWeapon);
  }
  fire(yaw: number, pitch: number): void {
    if (this.equippedWeapon === "machine_gun") return;
    const now = performance.now();
    const definition = this.assets?.manifest.weapons.find(
      (w) => w.id === this.equippedWeapon,
    );
    if (now - this.fireAt < ((definition?.fire_ticks ?? 9) * 1000) / 30) return;
    this.fireAt = now;
    if (this.weaponStatus?.magazine === 0) {
      this.dryFire(now);
      return;
    }
    if (!this.driving)
      this.requestAction({
        kind: "fire",
        data: {
          weapon_id: this.equippedWeapon,
          aim_yaw: yaw,
          aim_pitch: pitch,
        },
      });
  }
  equipWeapon(weaponId: "pistol" | "rifle" | "machine_gun"): void {
    this.stopTrigger();
    if (!this.driving)
      this.requestAction({
        kind: "equip_weapon",
        data: { weapon_id: weaponId },
      });
  }
  reload(): void {
    this.stopTrigger();
    if (!this.driving) this.requestAction({ kind: "reload", data: {} });
  }
  useTerminal(): void {
    const now = performance.now(),
      zone = this.repairZone(now);
    if (zone) {
      this.requestAction({ kind: "repair", data: { zone_id: zone.id } });
      return;
    }
    const template = this.terminal(now);
    if (template) this.acceptJob(template.id);
  }
  cancelMission(): void {
    this.requestAction({ kind: "cancel_mission", data: {} });
  }
  interactionPrompt(now: number): string {
    const terminal = this.terminal(now);
    const job = terminal && !this.mission ? " · F choose a job" : "";
    if (this.driving && this.repairZone(now))
      return "F repair · 50 credits · stop for 2 seconds · clear heat required";
    if (this.driving)
      return `E exit when stopped · WASD drive · Space handbrake${job}`;
    const target = this.target(now);
    const secured = target?.descriptor.display_name?.startsWith("Secured ");
    return target?.state.kind === "vehicle"
      ? target.state.data.disabled || target.state.data.health === 0
        ? "Vehicle disabled · awaiting a clear parking respawn"
        : target.state.data.driver_id
          ? "Vehicle occupied"
          : `E enter ${target.descriptor.template_id} · Car health ${target.state.data.health}${secured ? " · Secured: police witnesses add heat" : ""}${job}`
      : terminal && !this.mission
        ? "F choose a job"
        : "Walk to the left door of a parked car · E enter";
  }
  interact(): void {
    const now = performance.now();
    if (this.phase !== "playing" || now - this.actionAt < 250) return;
    const target = this.target(now);
    if (!this.driving && !target) return;
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
    if (controlled && predicted) {
      controlled.motor = predicted;
      if (controlled.state.kind === "vehicle")
        controlled.steering =
          this.local?.vehicleSteering ?? controlled.state.data.motor.steering;
    }
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
      joinMs: this.joinMs,
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
  /** Bounded scripted input test for connection diagnostics. */
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
