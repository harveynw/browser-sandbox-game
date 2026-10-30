import { GameAudio } from "../audio/sound";
import { CityMap } from "../ui/city-map";
import { GameHud, Tutorial, friendlyProblem } from "../ui/hud";
import { ChatPanel } from "../ui/chat-panel";
import { ControlsPanel } from "../ui/controls-panel";
import { SettingsPanel } from "../ui/settings-panel";
import { readSettings, saveSettings, controlText } from "./settings";
import { Diagnostics } from "./diagnostics";
import { simulationMemoryBytes } from "../simulation/bridge";
import {
  courierGuidance,
  repairGuidance,
  missionCard,
  selectedMissionTemplate,
} from "./courier";
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
  private readonly hud = new GameHud();
  private readonly audio = new GameAudio();
  private readonly tutorial = new Tutorial();
  private readonly controls = new ControlsPanel();
  private readonly chat = new ChatPanel(
    (text) => this.session?.sendChat(text) ?? false,
    () => {
      this.input.clear();
      this.audio.pause(true);
      this.overlay.hidden = true;
      if (document.pointerLockElement === this.canvas)
        document.exitPointerLock();
    },
  );
  private cityMap: CityMap | undefined;
  private readonly mapButton = document.createElement("button");
  private readonly instructions = document.createElement("p");
  private readonly note = document.createElement("p");
  private readonly menuSummary = document.createElement("p");
  private readonly settings = (() => {
    try {
      return readSettings(
        localStorage,
        matchMedia("(prefers-reduced-motion: reduce)").matches,
      );
    } catch {
      return readSettings();
    }
  })();
  private readonly preferences: SettingsPanel;
  private completedJobs = 0;
  private lastMissionSound = "";
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
  private readonly jobChoices = document.createElement("div");
  private jobChoiceKey = "";
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
    kicker.className = "eyebrow";
    kicker.textContent = "Port Meridian / Six districts";
    this.status.setAttribute("role", "status");
    const instructions = this.instructions;
    instructions.textContent =
      "WASD or arrows to move · Shift to sprint · Space to jump · Mouse to look · Esc to pause. The online world continues while paused.";
    const note = this.note;
    note.className = "game-note";
    note.textContent =
      "E enters/exits a parked car. At a cyan terminal, F opens jobs: Quay courier / recovery, or the north ring time trial while driving. Follow the gold marker. In the green Ironworks repair zone, stop and press F to repair for 50 credits.";
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
    this.cancelCourier.textContent = "Cancel job";
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
    this.preferences = new SettingsPanel(this.settings, () =>
      this.applySettings(),
    );
    const settings = this.preferences.element;
    this.mapButton.textContent = "City map · M";
    this.mapButton.hidden = true;
    this.mapButton.onclick = () => this.openMap();
    actions.append(this.mapButton);
    this.applySettings();
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
      this.menuSummary,
      this.preferences.name,
      this.preferences.colour,
      instructions,
      this.courierInfo,
      this.jobChoices,
      actions,
      settings,
      note,
      links,
    );
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
    const panels = document.createElement("aside");
    panels.className = "game-panels";
    panels.setAttribute("aria-label", "Game panels");
    panels.append(debugPanel, this.controls.element);
    this.cross.className = "game-crosshair";
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
      this.hud.element,
      this.chat.element,
      panels,
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
        this.chat.blur();
        void this.capture();
      },
      options,
    );
    this.unbind = bindInput(this.canvas, this.input, () => {
      this.audio.pause(true);
      this.overlay.hidden = !!this.cityMap?.open || this.chat.focused;
      this.root.dataset.paused = "true";
      if (this.phase === "playing")
        this.status.textContent = "Paused — the online world continues.";
    });
    window.addEventListener(
      "keydown",
      (e) => {
        if (
          e.code !== "KeyT" ||
          e.repeat ||
          this.phase !== "playing" ||
          this.cityMap?.open ||
          e.target instanceof HTMLInputElement ||
          e.target instanceof HTMLTextAreaElement
        )
          return;
        e.preventDefault();
        e.stopImmediatePropagation();
        this.chat.focus();
      },
      { ...options, capture: true },
    );
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
    window.addEventListener(
      "resize",
      () => {
        this.renderer?.resize();
      },
      options,
    );
    window.addEventListener(
      "keydown",
      (event) => {
        if (
          this.cityMap?.open &&
          (event.code === "Escape" || event.code === this.settings.bindings.map)
        ) {
          event.preventDefault();
          this.closeMap();
        }
      },
      options,
    );
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
        "Explore six city districts. Choose Play to join the city.",
    );
    this.frame = requestAnimationFrame((now) => this.animate(now));
  }
  private applySettings(): void {
    this.input.bindings = { ...this.settings.bindings };
    this.controls.update(this.settings.bindings);
    this.input.sensitivity = this.settings.sensitivity;
    this.input.invert = this.settings.invert;
    this.reduced = this.settings.reduced;
    this.low = this.settings.quality === "low";
    this.renderer?.quality(this.low);
    this.root.dataset.reduced = String(this.reduced);
    this.root.dataset.quality = this.settings.quality;
    this.audio.configure(
      this.settings.master,
      this.settings.effects,
      this.settings.muted,
    );
    this.instructions.textContent = controlText(
      "WASD / arrows move · Shift sprint · Space jump · Mouse look · Esc menu",
      this.settings.bindings,
    );
    this.note.textContent = controlText(
      "Cyan circles: F choose a job. E enters/exits a car. Green repair zone: stop, then F · 50 credits. M opens the city map. LMB fires while aiming with RMB / Q. R reloads.",
      this.settings.bindings,
    );
    this.mapButton.textContent = controlText(
      "City map · M",
      this.settings.bindings,
    );
    saveSettings(this.settings);
    this.session?.updateTshirtColour(
      Number.parseInt(this.settings.tshirtColour.slice(1), 16),
    );
  }
  private openMap(): void {
    this.preferences.cancelRemap();
    if (this.phase !== "playing" || !this.cityMap) return;
    this.cityMap.show();
    this.input.clear();
    this.audio.pause(true);
    this.overlay.hidden = true;
    if (document.pointerLockElement === this.canvas) document.exitPointerLock();
  }
  private closeMap(): void {
    this.cityMap?.hide();
    this.overlay.hidden = false;
    this.resume.focus();
  }
  private setPhase(phase: AppPhase, message?: string): void {
    this.phase = phase;
    this.chat.element.hidden = phase !== "playing";
    if (phase !== "playing") this.chat.blur();
    this.mapButton.hidden = phase !== "playing";
    if (phase !== "playing") {
      this.audio.pause(true);
      this.cityMap?.hide();
    }
    this.root.dataset.phase = phase;
    if (message) this.status.textContent = message;
    const playing = phase === "playing";
    this.play.hidden = playing;
    this.resume.hidden = !playing;
    this.play.disabled = !["menu", "disconnected", "fatal"].includes(phase);
    this.play.textContent = phase === "menu" ? "Play" : "Retry";
    this.leave.hidden = ["menu", "disconnected", "fatal"].includes(phase);
    this.preferences.name.hidden = playing;
    this.root.dataset.paused = String(!this.input.active);
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
    this.preferences.cancelRemap();
    if (!this.preferences.nameInput.reportValidity()) return;
    this.settings.name = this.preferences.nameInput.value;
    saveSettings(this.settings);
    void this.audio.unlock();
    this.session?.close("Rejoining");
    this.chat.clear();
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
          this.setPhase("disconnected", friendlyProblem(text.slice(8)));
        else if (text.startsWith("Connecting"))
          this.setPhase("connecting", "Connecting to the city…");
        else if (text.startsWith("Authenticating"))
          this.setPhase("authenticating", "Joining your guest session…");
      },
      {
        chat: (line) => this.chat.receive(line),
        chatError: (text) => this.chat.notice(text),
        assets: (assets, queries) => {
          if (this.session !== session) return;
          this.assets = assets;
          this.queries = queries;
          this.renderer = new GameRenderer(this.canvas, assets);
          this.renderer.quality(this.low);
          this.cityMap = new CityMap(assets, () => this.closeMap());
          this.root.append(this.cityMap.element);
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
          if (
            (this.mode === "driving") !==
            (owner.player_state.mode === "driving")
          )
            this.audio.cue("door");
          this.mode = owner.player_state.mode;
          this.carHealth =
            owner.state.kind === "vehicle" ? owner.state.data.health : null;
        },
        appearanceError: () => {
          if (this.session === session)
            this.status.textContent =
              "Could not save T-shirt colour. Try selecting it again.";
        },
        tshirtColour: (colour) => {
          if (this.session === session) this.renderer?.tshirtColour(colour);
        },
        worldEvent: (event, own) => {
          this.renderer?.effect(event, own);
          this.audio.cue(
            event.kind === "shot" ? "shot" : "impact",
            event.kind === "shot" ? event.data.origin : event.data.position,
            own ? 3 : 1,
          );
          if (own && event.kind === "shot" && event.data.hit_entity_id !== null)
            this.hitUntil = performance.now() + 300;
        },
        playerStatus: (status) => {
          if (this.session === session) {
            this.credits = status.credits;
            this.completedJobs = status.completed_jobs;
            if (
              status.reload_end_tick !== null &&
              status.reload_end_tick !== this.reloadEnd
            )
              this.audio.cue("reload");
            const missionSound = status.mission
              ? `${status.mission.instance_id}:${status.mission.phase}`
              : "";
            if (
              missionSound !== this.lastMissionSound &&
              status.mission?.phase === "completed"
            )
              this.audio.cue("success");
            if (
              missionSound !== this.lastMissionSound &&
              status.mission?.phase === "failed"
            )
              this.audio.cue("failure");
            this.lastMissionSound = missionSound;
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
      this.settings.name,
      Number.parseInt(this.settings.tshirtColour.slice(1), 16),
    );
    this.session = session;
    void session.connect();
  }
  private async capture(): Promise<void> {
    this.preferences.cancelRemap();
    if (this.phase !== "playing" || this.contextLost || this.cityMap?.open)
      return;
    if (this.input.active) return;
    this.audio.pause(false);
    void this.audio.unlock();
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
    this.chat.update(now);
    if (this.disposed) return;
    this.debug.frame(
      now - this.previous,
      this.session?.takePredictionWork() ?? 0,
      this.phase === "playing" && !document.hidden,
    );
    const dt = Math.max(0, Math.min((now - this.previous) / 1000, 0.1));
    this.previous = now;
    try {
      if (!this.contextLost && this.phase === "playing") {
        if (this.input.consumeMap()) this.openMap();
        if (this.input.consumeInteract()) this.session?.interact();
        if (this.input.consumeJob()) {
          if (
            !this.session?.repairZone(now) &&
            (this.session?.nearbyJobs(now).length ?? 0) > 1
          ) {
            this.input.clear();
            if (document.pointerLockElement === this.canvas)
              document.exitPointerLock();
            this.overlay.hidden = false;
            this.status.textContent = "Choose a nearby job.";
          } else this.session?.useTerminal();
        }
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
      this.audio.pause(
        !this.input.active || this.phase !== "playing" || document.hidden,
      );
      const localMotor = this.session?.presentation(now) ?? this.motor;
      const remotes = this.session?.remoteSamples(now) ?? [];
      if (!this.contextLost && this.renderer && this.queries && localMotor) {
        const aiming =
          this.input.active &&
          this.input.aiming &&
          this.health > 0 &&
          this.mode === "on_foot";
        this.renderer.pose(
          localMotor,
          this.session?.isDriving,
          this.session?.controlledVehicleTemplate,
          dt,
          this.reduced,
          aiming,
          this.reloadEnd !== null,
          this.health <= 0 || this.mode === "arrested",
          this.input.down("jump"),
        );
        this.renderer.remotes(remotes);
        const mission = this.session?.currentMission;
        const objective = selectedMissionTemplate(
          this.assets?.manifest.mission_templates.find(
            (t) => t.id === mission?.template_id,
          ),
          mission ?? null,
        )?.objective;
        const targetRadius =
          objective?.kind === "courier"
            ? Math.min(
                5,
                this.assets?.manifest.zones.find(
                  (z) => z.id === objective.data.destination_zone_id,
                )?.radius ?? 5,
              )
            : objective?.kind === "time_trial"
              ? objective.data.gate_radius
              : 5;
        this.renderer.setMissionTarget(
          mission?.phase === "active" ? mission.target_position : null,
          objective?.kind === "courier",
          targetRadius,
        );
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
        const siren =
          this.heat > 0
            ? remotes
                .filter(
                  (a) =>
                    a.descriptor.entity_kind === "police" &&
                    !a.stale &&
                    Math.hypot(
                      a.motor.position[0] - localMotor.position[0],
                      a.motor.position[2] - localMotor.position[2],
                    ) < 60,
                )
                .sort(
                  (a, b) =>
                    Math.hypot(
                      a.motor.position[0] - localMotor.position[0],
                      a.motor.position[2] - localMotor.position[2],
                    ) -
                    Math.hypot(
                      b.motor.position[0] - localMotor.position[0],
                      b.motor.position[2] - localMotor.position[2],
                    ),
                )[0]?.motor.position
            : undefined;
        this.audio.update(
          localMotor.position,
          localMotor.yaw,
          Math.hypot(localMotor.velocity[0], localMotor.velocity[2]),
          localMotor.grounded,
          this.session?.isDriving ?? false,
          this.input.down("jump"),
          siren,
          dt,
          this.session?.controlledVehicleTemplate?.top_speed,
        );
      }
      if (now - this.lastHud > 100) {
        this.lastHud = now;
        this.cross.style.color = now < this.hitUntil ? "#ffcf68" : "";
        this.cross.textContent = now < this.hitUntil ? "×" : "+";
        this.cross.hidden = this.phase !== "playing" || !this.input.active;
        const net = this.session?.metrics(now);
        this.minimap.hidden = this.phase !== "playing";

        const streaming = this.renderer?.streamingMetrics;
        const mission = this.session?.currentMission ?? null;
        const terminal = this.assets?.manifest.zones
          .filter((z) => z.kind === "terminal")
          .sort(
            (a, b) =>
              Math.hypot(
                (localMotor?.position[0] ?? 0) - a.center[0],
                (localMotor?.position[2] ?? 0) - a.center[2],
              ) -
              Math.hypot(
                (localMotor?.position[0] ?? 0) - b.center[0],
                (localMotor?.position[2] ?? 0) - b.center[2],
              ),
          )[0];
        const choices = terminal?.allowed_templates
          .map(
            (id) =>
              this.assets?.manifest.mission_templates.find((t) => t.id === id)
                ?.objective.kind,
          )
          .map((k) =>
            k === "time_trial"
              ? "time trial (driving)"
              : k === "recovery"
                ? "recovery"
                : "courier",
          )
          .join(" / ");
        const guidance = controlText(
          courierGuidance(
            mission,
            this.session?.estimatedTick(now) ?? 0,
            localMotor?.position,
            terminal?.center,
            this.assets?.manifest.mission_templates.find(
              (t) => t.id === mission?.template_id,
            ),
            this.input.active,
            terminal?.id === "terminal_trial"
              ? "North ring"
              : terminal?.id === "terminal_civic"
                ? "Civic"
                : "Quay",
            choices,
          ),
          this.settings.bindings,
        );
        if (localMotor)
          this.cityMap?.update(
            localMotor,
            remotes,
            mission?.phase === "active" ? mission.target_position : null,
            this.assets?.manifest.mission_templates.find(
              (t) => t.id === mission?.template_id,
            )?.objective.kind === "courier",
            this.minimap,
          );
        const jobs = this.session?.nearbyJobs(now) ?? [];
        const key = jobs.map((t) => t.id).join(",");
        if (key !== this.jobChoiceKey) {
          this.jobChoiceKey = key;
          this.jobChoices.replaceChildren(
            ...jobs.map((t) => {
              const button = document.createElement("button");
              const contracts = t.variants?.length ? t.variants : [t];
              const rewards = contracts.map((v) => v.reward_credits);
              const minReward = Math.min(...rewards),
                maxReward = Math.max(...rewards);
              const payout =
                minReward === maxReward
                  ? `${minReward}`
                  : `${minReward}–${maxReward}`;
              button.textContent = `${t.objective.kind === "courier" ? "Courier" : t.objective.kind === "recovery" ? "Vehicle recovery" : "Time trial (driving)"} · ${payout} credits · ${t.objective.kind === "courier" ? "Random destination" : t.objective.kind === "time_trial" ? "Random route" : "Reserved vehicle"}`;
              button.addEventListener(
                "click",
                () => {
                  this.session?.acceptJob(t.id);
                  void this.capture();
                },
                { signal: this.abort.signal },
              );
              return button;
            }),
          );
        }
        this.jobChoices.className = "game-job-choices";
        this.courierInfo.textContent = this.phase === "playing" ? guidance : "";
        this.cancelCourier.hidden =
          this.phase !== "playing" ||
          mission?.phase !== "active" ||
          mission.target_position === null;
        const drivingStatus = this.session?.isDriving
          ? ` · ${(Math.hypot(...(localMotor?.velocity ?? [0, 0, 0])) * 3.6).toFixed(0)} km/h${this.input.active && this.input.down("jump") ? " · Handbrake" : ""}`
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
              : [
                  this.reloadEnd !== null ? "Reloading" : "",
                  this.protectedUntil !== null &&
                  estimated < this.protectedUntil
                    ? "Protected"
                    : "",
                  now < this.hitUntil ? "Hit confirmed" : "",
                ]
                  .filter(Boolean)
                  .join(" · ");
        const prompt = this.session?.interactionPrompt(now) ?? "";
        const repair = this.session?.currentRepair;
        this.hud.element.hidden =
          this.phase !== "playing" ||
          (!this.input.active && !this.chat.focused);
        this.minimap.hidden =
          this.phase !== "playing" ||
          (!this.input.active && !this.chat.focused);
        this.root.dataset.paused = String(!this.input.active);
        this.menuSummary.textContent =
          this.phase === "playing"
            ? `${this.credits.toLocaleString()} credits · ${this.completedJobs} completed ${this.completedJobs === 1 ? "job" : "jobs"} · Heat ${this.heat}/3`
            : "";
        this.menuSummary.className = "game-profile";
        this.preferences.soundStatus.textContent = this.audio.status;
        const district = this.assets?.manifest.districts.find(
          (d) =>
            localMotor &&
            localMotor.position[0] >= d.rect.min[0] &&
            localMotor.position[0] < d.rect.max[0] &&
            localMotor.position[2] >= d.rect.min[1] &&
            localMotor.position[2] < d.rect.max[1],
        );
        this.controls.setContext(
          controlText(prompt, this.settings.bindings),
          localMotor && this.completedJobs === 0
            ? controlText(
                this.tutorial.update(
                  localMotor.position,
                  this.session?.isDriving ?? false,
                  !!mission,
                  prompt.includes("Car health"),
                ),
                this.settings.bindings,
              )
            : "",
        );
        this.hud.update({
          guidance: repair ? repairGuidance(repair) : guidance,
          mission: repair
            ? null
            : missionCard(
                mission,
                this.assets?.manifest.mission_templates.find(
                  (t) => t.id === mission?.template_id,
                ),
                estimated,
                localMotor?.position,
              ),
          objectiveLabel: repair
            ? "Vehicle repair"
            : mission
              ? "Current job"
              : "Nearby jobs",
          credits: this.credits,
          health: this.health,
          heat: this.heat,
          pursuit:
            this.pursuit === "clear"
              ? "Clear"
              : this.pursuit === "arresting"
                ? `Arrest ${Math.min(100, Math.round(((estimated - (this.arrestStart ?? estimated)) / 90) * 100))}% · move`
                : this.pursuit === "searching"
                  ? "Searching · stay hidden"
                  : "Pursuit · break sight",
          ammo: controlText(combat, this.settings.bindings),
          magazine: this.magazine,
          vehicle: drivingStatus.replace(/^ · /, ""),
          vehicleHealth:
            this.session?.isDriving && this.carHealth !== null
              ? {
                  current: this.carHealth,
                  maximum:
                    this.session.controlledVehicleTemplate?.health ?? 100,
                }
              : null,
          warning: net?.stale
            ? "Connection delayed — waiting for the server"
            : (this.session?.actionMessage ?? ""),
        });
        this.root.setAttribute(
          "aria-label",
          `Port Meridian — ${district?.name ?? "city"}`,
        );
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
    this.audio.pause(true);
    this.cityMap?.dispose();
    this.cityMap = undefined;
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
    this.preferences.dispose();
    this.audio.dispose();
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
