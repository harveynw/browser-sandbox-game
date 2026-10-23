import { Diagnostics } from "./diagnostics";
import { simulationMemoryBytes } from "../simulation/bridge";
import { courierGuidance } from "./courier";
import type {
  CharacterMode,
  CharacterMotor,
  Pursuit,
} from "@port-meridian/protocol";
import type { LoadedPackage } from "@port-meridian/content";
import type { PredictionWorld } from "../simulation/bridge";
import { GameSession } from "../net/session";
import { InputState, bindInput, type InputBinding } from "../input/input";
import { GameRenderer } from "../render/renderer";
import { FollowCamera } from "../render/camera";
export type AppPhase =
  | "boot"
  | "menu"
  | "loading_assets"
  | "connecting"
  | "authenticating"
  | "loading_world"
  | "playing"
  | "resyncing"
  | "disconnected"
  | "fatal";

export class GameApp {
  private phase: AppPhase = "boot";
  private readonly input = new InputState();
  private readonly camera = new FollowCamera();
  private readonly abort = new AbortController();
  private readonly canvas = document.createElement("canvas");
  private readonly overlay = document.createElement("section");
  private readonly status = document.createElement("p");
  private readonly play = document.createElement("button");
  private readonly resume = document.createElement("button");
  private readonly leave = document.createElement("button");
  private carHealth: number | null = null;
  private magazine = 12;
  private heat = 0;
  private pursuit: Pursuit = "clear";
  private arrestStart: number | null = null;
  private mode: CharacterMode = "on_foot";
  private reloadEnd: number | null = null;
  private respawnTick: number | null = null;
  private protectedUntil: number | null = null;
  private hitUntil = 0;
  private readonly cross = document.createElement("div");
  private readonly hud = document.createElement("div");
  private readonly minimap = document.createElement("canvas");
  private readonly debug = new Diagnostics();
  private renderer: GameRenderer | undefined;
  private queries: PredictionWorld | undefined;
  private assets: LoadedPackage | undefined;
  private session: GameSession | undefined;
  private motor: CharacterMotor | undefined;
  private frame = 0;
  private previous = performance.now();
  private lastHud = 0;
  private unbind: InputBinding;
  private disposed = false;
  private low = false;
  private reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  private credits = 0;
  private health = 100;
  private contextLost = false;
  private dragLook = false;
  private connection = "";
  private ownerTick = 0;
  private controlEpoch = 0;
  private cameraDistance = 0;
  private recoveryTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly reload = document.createElement("button");
  private readonly cancelCourier = document.createElement("button");
  private readonly courierInfo = document.createElement("p");
  constructor(private readonly root: HTMLElement) {
    document.title = "Port Meridian — explore the city";
    document.body.classList.add("game-mode");
    root.className = "game-app";
    this.canvas.setAttribute("aria-label", "Port Meridian third-person game");
    this.overlay.className = "game-menu";
    const title = document.createElement("h1");
    title.textContent = "Port Meridian";
    const kicker = document.createElement("p");
    kicker.className = "stage";
    kicker.textContent = "Port Meridian / Six districts";
    this.status.setAttribute("role", "status");
    const instructions = document.createElement("p");
    instructions.textContent =
      "WASD or arrows to move · Shift to sprint · Space to jump · Mouse to look · Esc to pause. The online world continues while paused.";
    const note = document.createElement("p");
    note.className = "game-note";
    note.textContent =
      "Walk to a parked car, press E at its left door, then drive. Find the cyan Quay terminal and press E / F for a courier job. Deliver to the gold Market marker for 150 credits.";
    this.play.textContent = "Play";
    this.resume.textContent = "Resume / mouse look";
    this.leave.textContent = "Leave game";
    const actions = document.createElement("div");
    actions.className = "actions";
    this.reload.textContent = "Reload page";
    this.reload.hidden = true;
    this.reload.addEventListener("click", () => location.reload(), {
      signal: this.abort.signal,
    });
    this.cancelCourier.textContent = "Cancel courier";
    this.cancelCourier.hidden = true;
    this.cancelCourier.addEventListener(
      "click",
      () => this.session?.cancelMission(),
      { signal: this.abort.signal },
    );
    actions.append(
      this.play,
      this.resume,
      this.leave,
      this.cancelCourier,
      this.reload,
    );
    const settings = document.createElement("details");
    const summary = document.createElement("summary");
    summary.textContent = "Settings";
    settings.append(summary);
    const label = (
      text: string,
      input: HTMLInputElement | HTMLSelectElement,
    ) => {
      const row = document.createElement("label");
      row.append(text, input);
      settings.append(row);
    };
    const sensitivity = document.createElement("input");
    sensitivity.type = "range";
    sensitivity.min = "0.0005";
    sensitivity.max = "0.006";
    sensitivity.step = "0.0001";
    sensitivity.value = "0.002";
    sensitivity.addEventListener(
      "input",
      () => {
        this.input.sensitivity = Number(sensitivity.value);
      },
      { signal: this.abort.signal },
    );
    label("Mouse sensitivity", sensitivity);
    const check = (
      text: string,
      initial: boolean,
      apply: (v: boolean) => void,
    ) => {
      const el = document.createElement("input");
      el.type = "checkbox";
      el.checked = initial;
      el.addEventListener("change", () => apply(el.checked), {
        signal: this.abort.signal,
      });
      label(text, el);
    };
    check("Invert mouse Y", false, (v) => {
      this.input.invert = v;
    });
    check("Reduced motion", this.reduced, (v) => {
      this.reduced = v;
    });
    const quality = document.createElement("select");
    for (const name of ["Medium", "Low"]) {
      const opt = document.createElement("option");
      opt.textContent = name;
      quality.append(opt);
    }
    quality.addEventListener(
      "change",
      () => {
        this.low = quality.value === "Low";
        this.renderer?.quality(this.low);
      },
      { signal: this.abort.signal },
    );
    label("Graphics quality", quality);
    const links = document.createElement("p");
    links.className = "game-links";
    for (const [href, text] of [
      ["/map", "Map inspector"],
      ["/simulation", "Movement trace"],
      ["/diagnostic", "Connection diagnostic"],
    ]) {
      const a = document.createElement("a");
      a.href = href ?? "/";
      a.textContent = text ?? "";
      links.append(a, " ");
    }
    this.overlay.append(
      kicker,
      title,
      this.status,
      instructions,
      this.courierInfo,
      actions,
      settings,
      note,
      links,
    );
    this.hud.className = "game-hud";
    const debugPanel = document.createElement("details");
    debugPanel.className = "game-metrics";
    const debugSummary = document.createElement("summary");
    debugSummary.textContent = "Diagnostics";
    const resync = document.createElement("button");
    resync.textContent = "Resync world";
    resync.addEventListener("click", () => this.session?.requestResync(), {
      signal: this.abort.signal,
    });
    debugPanel.append(debugSummary, this.debug.element, resync);
    this.cross.className = "game-debug-cross";
    this.cross.setAttribute("aria-hidden", "true");
    this.cross.hidden = true;
    this.minimap.hidden = true;
    this.minimap.width = 256;
    this.minimap.height = 256;
    this.minimap.className = "city-minimap";
    this.minimap.setAttribute("role", "img");
    this.minimap.setAttribute("aria-label", "City minimap");
    root.replaceChildren(
      this.canvas,
      this.minimap,
      this.cross,
      this.hud,
      debugPanel,
      this.overlay,
    );
    const options = { signal: this.abort.signal };
    this.play.addEventListener("click", () => this.join(), options);
    this.resume.addEventListener(
      "click",
      () => {
        void this.capture();
      },
      options,
    );
    this.leave.addEventListener(
      "click",
      () => this.session?.close("Left game"),
      options,
    );
    this.canvas.addEventListener(
      "click",
      () => {
        void this.capture();
      },
      options,
    );
    this.unbind = bindInput(this.canvas, this.input, () => {
      this.overlay.hidden = false;
      if (this.phase === "playing")
        this.status.textContent = "Paused — the online world continues.";
    });
    document.addEventListener(
      "pointerlockchange",
      () => {
        if (
          document.pointerLockElement === this.canvas &&
          this.phase === "playing"
        )
          this.overlay.hidden = true;
      },
      options,
    );
    document.addEventListener(
      "visibilitychange",
      () => {
        if (!document.hidden && this.phase === "playing")
          this.session?.requestResync();
      },
      options,
    );
    window.addEventListener("resize", () => this.renderer?.resize(), options);
    window.addEventListener("pagehide", () => this.dispose(), options);
    this.canvas.addEventListener(
      "webglcontextlost",
      (event) => {
        event.preventDefault();
        this.contextLost = true;
        this.input.clear();
        document.exitPointerLock();
        this.overlay.hidden = false;
        this.status.textContent = "Graphics interrupted. Waiting for recovery…";
        clearTimeout(this.recoveryTimer);
        this.recoveryTimer = setTimeout(() => {
          if (!this.contextLost) return;
          this.session?.close(
            "Graphics recovery timed out. Reload the page to retry.",
          );
          this.reload.hidden = false;
        }, 8000);
      },
      options,
    );
    this.canvas.addEventListener(
      "webglcontextrestored",
      () => {
        if (!this.assets || this.disposed) return;
        clearTimeout(this.recoveryTimer);
        try {
          this.renderer?.dispose();
          this.renderer = new GameRenderer(this.canvas, this.assets);
          this.renderer.quality(this.low);
          this.contextLost = false;
          this.session?.requestResync();
        } catch {
          this.session?.close(
            "Graphics recovery failed. Reload the page to retry.",
          );
          this.reload.hidden = false;
        }
      },
      options,
    );
    const unsupported = this.supportReason();
    this.setPhase(
      unsupported ? "fatal" : "menu",
      unsupported ??
        "Explore six city districts. Start the local game server, then choose Play.",
    );
    this.frame = requestAnimationFrame((now) => this.animate(now));
  }
  private setPhase(phase: AppPhase, message?: string): void {
    this.phase = phase;
    this.root.dataset.phase = phase;
    if (message) this.status.textContent = message;
    const playing = phase === "playing";
    this.play.hidden = playing;
    this.resume.hidden = !playing;
    this.play.disabled = !["menu", "disconnected", "fatal"].includes(phase);
    this.play.textContent = phase === "menu" ? "Play" : "Retry";
    this.leave.hidden = ["menu", "disconnected", "fatal"].includes(phase);
    if (!playing) {
      this.input.clear();
      if (document.pointerLockElement === this.canvas)
        document.exitPointerLock();
      this.overlay.hidden = false;
    }
  }
  private supportReason(): string | null {
    if (
      !["WebAssembly", "WebSocket", "RTCPeerConnection"].every(
        (key) => key in window,
      )
    )
      return "This browser needs WebAssembly, WebSocket and WebRTC support.";
    try {
      if (this.canvas.getContext("webgl2")) return null;
    } catch {
      /* Report the same actionable hardware message. */
    }
    return "This game requires WebGL2. Enable hardware acceleration or use a supported desktop browser.";
  }
  private join(): void {
    this.session?.close("Rejoining");
    this.cleanupScene();
    const unsupported = this.supportReason();
    if (unsupported) {
      this.setPhase("fatal", unsupported);
      return;
    }
    this.setPhase("loading_assets", "Loading the district…");
    const session = new GameSession(
      () => {},
      (text) => {
        if (this.disposed || this.session !== session) return;
        this.connection = text;
        if (text.startsWith("Closed:"))
          this.setPhase("disconnected", text.slice(8));
        else if (text.startsWith("Connecting"))
          this.setPhase("connecting", "Connecting to the local game server…");
        else if (text.startsWith("Authenticating"))
          this.setPhase("authenticating", "Joining your guest session…");
      },
      {
        assets: (assets, queries) => {
          if (this.session !== session) return;
          this.assets = assets;
          this.queries = queries;
          this.renderer = new GameRenderer(this.canvas, assets);
          this.renderer.quality(this.low);
        },
        phase: (phase) => {
          if (this.session !== session) return;
          if (phase === "closed") {
            this.cleanupScene();
            this.setPhase("disconnected");
          } else if (phase === "playing") {
            this.camera.reset();
            this.setPhase(
              "playing",
              "Ready. Capture the mouse to explore; Esc opens this menu.",
            );
          } else
            this.setPhase(
              this.phase === "playing" ? "resyncing" : "loading_world",
              "Synchronizing with the server…",
            );
        },
        owner: (owner) => {
          if (this.session !== session) return;
          if (owner.control_epoch !== this.controlEpoch) {
            this.controlEpoch = owner.control_epoch;
            this.input.keys.clear();
            this.input.aiming = false;
            this.input.yaw =
              owner.state.kind === "vehicle" ? 0 : owner.player_state.motor.yaw;
            this.camera.reset();
          }
          this.ownerTick = owner.server_tick;
          this.motor = owner.player_state.motor;
          this.health = owner.player_state.health;
          this.mode = owner.player_state.mode;
          this.carHealth =
            owner.state.kind === "vehicle" ? owner.state.data.health : null;
        },
        worldEvent: (event, own) => {
          this.renderer?.effect(event);
          if (own && event.kind === "shot" && event.data.hit_entity_id !== null)
            this.hitUntil = performance.now() + 300;
        },
        playerStatus: (status) => {
          if (this.session === session) {
            this.credits = status.credits;
            this.magazine = status.magazine;
            this.heat = status.heat;
            if (status.pursuit === "arresting" && this.pursuit !== "arresting")
              this.arrestStart = status.server_tick;
            if (status.pursuit !== "arresting") this.arrestStart = null;
            this.pursuit = status.pursuit;
            this.reloadEnd = status.reload_end_tick;
            this.respawnTick = status.respawn_tick;
            this.protectedUntil = status.protected_until_tick;
          }
        },
      },
    );
    this.session = session;
    void session.connect();
  }
  private async capture(): Promise<void> {
    if (this.phase !== "playing" || this.contextLost) return;
    if (this.input.active) return;
    if (!this.dragLook) {
      try {
        await this.canvas.requestPointerLock();
        if (this.disposed || this.phase !== "playing") {
          if (document.pointerLockElement === this.canvas)
            document.exitPointerLock();
          return;
        }
        if (document.pointerLockElement === this.canvas) return;
        this.dragLook = true;
      } catch {
        this.dragLook = true;
      }
    }
    if (this.phase !== "playing" || this.disposed) return;
    this.unbind.activateDragLook();
    this.overlay.hidden = true;
    this.status.textContent = "Drag the scene to look. Esc opens the menu.";
  }
  private animate(now: number): void {
    if (this.disposed) return;
    this.debug.frame(now - this.previous);
    const dt = Math.max(0, Math.min((now - this.previous) / 1000, 0.1));
    this.previous = now;
    try {
      if (!this.contextLost && this.phase === "playing") {
        if (this.input.consumeInteract()) this.session?.interact();
        if (this.input.consumeJob()) this.session?.useTerminal();
        if (this.input.consumeFire() && this.health > 0)
          this.session?.fire(this.input.yaw, this.input.pitch);
        if (this.input.consumeReload() && this.health > 0)
          this.session?.reload();
        this.session?.advance(now, () =>
          this.health > 0
            ? this.input.sample(this.session?.isDriving)
            : {
                move_x: 0,
                move_z: 0,
                look_yaw: this.input.yaw,
                look_pitch: 0,
                buttons: 0,
              },
        );
      }
      const localMotor = this.session?.presentation(now) ?? this.motor;
      const remotes = this.session?.remoteSamples(now) ?? [];
      if (!this.contextLost && this.renderer && this.queries && localMotor) {
        this.renderer.pose(
          localMotor,
          this.session?.isDriving,
          this.session?.controlledVehicleTemplate,
        );
        this.renderer.remotes(remotes);
        const mission = this.session?.currentMission;
        this.renderer.setMissionTarget(
          mission?.phase === "active" ? mission.target_position : null,
        );
        const aiming =
          this.input.active &&
          this.input.aiming &&
          this.health > 0 &&
          this.mode === "on_foot";
        const distance = this.camera.update(
          this.renderer.camera,
          localMotor.position,
          this.input.yaw + (this.session?.isDriving ? localMotor.yaw : 0),
          this.input.pitch,
          dt,
          this.queries,
          this.reduced,
          this.session?.isDriving,
          this.session?.controlledVehicleTemplate?.dimensions[1],
          aiming,
        );
        this.cameraDistance = distance;
        this.renderer.render(distance, aiming);
      }
      if (now - this.lastHud > 100) {
        this.lastHud = now;
        this.cross.style.color = now < this.hitUntil ? "#ffcf68" : "";
        this.cross.textContent = now < this.hitUntil ? "×" : "+";
        this.cross.hidden = this.phase !== "playing" || !this.input.active;
        const net = this.session?.metrics(now);
        this.minimap.hidden = this.phase !== "playing";
        if (localMotor) this.renderer?.drawMinimap(this.minimap, localMotor);
        const streaming = this.renderer?.streamingMetrics;
        const mission = this.session?.currentMission ?? null;
        const guidance = courierGuidance(
          mission,
          this.session?.estimatedTick(now) ?? 0,
          localMotor?.position,
          this.assets?.manifest.zones.find((z) => z.kind === "terminal")
            ?.center,
        );
        this.courierInfo.textContent = this.phase === "playing" ? guidance : "";
        this.cancelCourier.hidden =
          this.phase !== "playing" || mission?.phase !== "active";
        const drivingStatus = this.session?.isDriving
          ? ` · ${(Math.hypot(...(this.motor?.velocity ?? [0, 0, 0])) * 3.6).toFixed(0)} km/h · Car health ${this.carHealth}${this.input.active && this.input.keys.has("Space") ? " · Handbrake" : ""}`
          : "";
        this.debug.update({
          now,
          connection: this.connection,
          net,
          streaming,
          wasmBytes: simulationMemoryBytes(),
          ownerTick: this.ownerTick,
          motor: this.motor,
          yaw: this.input.yaw,
          pitch: this.input.pitch,
          camera: this.cameraDistance,
          remotes,
        });
        const estimated = this.session?.estimatedTick(now) ?? 0;
        const combat =
          this.mode === "arrested"
            ? `Arrested · respawn ${Math.max(0, Math.ceil(((this.respawnTick ?? estimated) - estimated) / 30))}s`
            : this.health <= 0
              ? `Downed · respawn ${Math.max(0, Math.ceil(((this.respawnTick ?? estimated) - estimated) / 30))}s`
              : `${this.magazine}/12${this.session?.isDriving ? " · Weapon holstered" : this.reloadEnd !== null ? " · Reloading" : " · RMB / Q aim · click fire · R reload"}${this.protectedUntil !== null && estimated < this.protectedUntil ? " · Spawn protected" : ""}${now < this.hitUntil ? " · Hit confirmed" : ""}`;
        this.hud.textContent =
          this.phase === "playing"
            ? `Health ${this.health} · ${combat} · Heat ${"●".repeat(this.heat)}${"○".repeat(3 - this.heat)} · ${this.pursuit === "arresting" ? `Arrest ${Math.min(100, Math.max(0, Math.round(((estimated - (this.arrestStart ?? estimated)) / 90) * 100)))}% · move to escape` : this.pursuit === "searching" ? "Searching · stay out of sight" : this.pursuit === "pursuing" ? "Police pursuit · break line of sight" : "Clear"} · Credits ${this.credits} · Players ${remotes.filter((a) => a.descriptor.entity_kind === "player").length + 1}${drivingStatus}${net?.stale ? " · Connection delayed" : ""} · ${this.input.active ? `${this.session?.interactionPrompt(now) ?? ""} · Esc menu` : "Paused · world continues"}${this.session?.actionMessage ? ` · ${this.session.actionMessage}` : ""}\n${guidance}`
            : "Port Meridian · Walking and driving";
      }
    } catch (error) {
      this.session?.close(
        error instanceof Error
          ? error.message
          : "Unable to render the district",
      );
    }
    this.frame = requestAnimationFrame((t) => this.animate(t));
  }
  private cleanupScene(): void {
    this.input.clear();
    if (document.pointerLockElement === this.canvas) document.exitPointerLock();
    this.renderer?.dispose();
    this.renderer = undefined;
    this.queries = undefined;
    this.assets = undefined;
    this.motor = undefined;
    this.contextLost = false;
    clearTimeout(this.recoveryTimer);
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.abort.abort();
    this.unbind.dispose();
    cancelAnimationFrame(this.frame);
    this.session?.close("Page closed");
    this.cleanupScene();
    document.body.classList.remove("game-mode");
    this.root.className = "";
    this.root.replaceChildren();
  }
}
export function mountGamePage(root: HTMLElement): () => void {
  const app = new GameApp(root);
  return () => app.dispose();
}
