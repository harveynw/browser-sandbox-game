import type { InputFrame } from "@port-meridian/protocol";
import { defaults, type Bindings, type Control } from "../game/settings";
export class InputState {
  bindings: Bindings = { ...defaults };
  private map = false;
  readonly keys = new Set<string>();
  yaw = 0;
  pitch = -0.15;
  sensitivity = 0.002;
  invert = false;
  active = false;
  aiming = false;
  private fire = false;
  private held = false;
  private blocked = false;
  private weaponSelection: number | null = null;
  private cycle = 0;
  get triggerHeld(): boolean {
    return this.active && this.aiming && this.held && !this.blocked;
  }
  stopFiring(): void {
    this.held = false;
    this.fire = false;
    this.blocked = true;
  }
  consumeWeapon(): number | null {
    const n = this.weaponSelection;
    this.weaponSelection = null;
    return this.active ? n : null;
  }
  consumeCycle(): number {
    const n = this.cycle;
    this.cycle = 0;
    return this.active ? n : 0;
  }
  wheel(delta: number): void {
    if (this.active) this.cycle = delta > 0 ? 1 : -1;
  }
  private reload = false;
  private jump = false;
  private interact = false;
  private job = false;
  press(code: string, repeat = false): void {
    if (!this.active || !this.handles(code)) return;
    const action = this.action(code);
    this.keys.add(code);
    if (action === "map" && !repeat) this.map = true;
    if (action === "fire" && !repeat && this.aiming && !this.blocked) {
      this.fire = true;
      this.held = true;
    }
    if (action?.startsWith("weapon") && !repeat)
      this.weaponSelection = Number(action.slice(-1)) - 1;
    if (action === "aim" && !repeat) {
      this.aiming = !this.aiming;
      if (!this.aiming) this.stopFiring();
    }
    if (action === "reload" && !repeat) this.reload = true;
    if (action === "jump" && !repeat) this.jump = true;
    if (action === "interact" && !repeat) this.interact = true;
    if (action === "job" && !repeat) this.job = true;
  }
  private action(code: string): Control | undefined {
    const direct = (Object.keys(this.bindings) as Control[]).find(
      (a) => this.bindings[a] === code,
    );
    if (direct) return direct;
    const alternate: Record<string, Control> = {
      ArrowUp: "forward",
      ArrowDown: "backward",
      ArrowLeft: "left",
      ArrowRight: "right",
      ShiftRight: "sprint",
    };
    const a = alternate[code];
    return a && this.bindings[a] === defaults[a] ? a : undefined;
  }
  handles(code: string): boolean {
    return this.action(code) !== undefined;
  }
  down(action: Control): boolean {
    return this.active && [...this.keys].some((k) => this.action(k) === action);
  }
  consumeMap(): boolean {
    const map = this.active && this.map;
    this.map = false;
    return map;
  }
  release(code: string): void {
    if (this.action(code) === "fire") {
      this.held = false;
      this.blocked = false;
    }
    this.keys.delete(code);
  }
  clear(): void {
    this.stopFiring();
    this.blocked = false;
    this.weaponSelection = null;
    this.cycle = 0;
    this.keys.clear();
    this.aiming = false;
    this.fire = false;
    this.reload = false;
    this.jump = false;
    this.interact = false;
    this.job = false;
    this.map = false;
    this.active = false;
  }
  look(x: number, y: number): void {
    if (!this.active) return;
    this.yaw =
      ((((this.yaw - x * this.sensitivity + Math.PI) % (2 * Math.PI)) +
        2 * Math.PI) %
        (2 * Math.PI)) -
      Math.PI;
    this.pitch = Math.max(
      -1.1,
      Math.min(
        0.65,
        this.pitch + y * this.sensitivity * (this.invert ? -1 : 1),
      ),
    );
  }
  mouseButton(button: number, down: boolean): void {
    if (!this.active) return;
    if (button === 2) {
      this.aiming = down;
      if (!down) this.stopFiring();
    }
    if (button === 0 && !down) {
      this.held = false;
      this.blocked = false;
    }
    if (button === 0 && down && this.aiming && !this.blocked) {
      this.fire = true;
      this.held = true;
    }
  }
  consumeFire(): boolean {
    const fire = this.active && this.aiming && this.fire;
    this.fire = false;
    return fire;
  }
  consumeReload(): boolean {
    const reload = this.active && this.reload;
    this.reload = false;
    return reload;
  }
  consumeInteract(): boolean {
    const value = this.active && this.interact;
    this.interact = false;
    return value;
  }
  consumeJob(): boolean {
    const value = this.active && this.job;
    this.job = false;
    return value;
  }
  sample(driving = false): Omit<InputFrame, "tick"> {
    const down = (action: Control) => this.down(action);
    const result = {
      move_x: (Number(down("right")) - Number(down("left"))) * 32767,
      move_z: (Number(down("forward")) - Number(down("backward"))) * 32767,
      look_yaw: this.yaw,
      look_pitch: this.pitch,
      buttons:
        (!this.aiming && down("sprint") ? 1 : 0) |
        (!driving && this.active && this.aiming ? 8 : 0) |
        (driving ? (down("jump") ? 4 : 0) : this.active && this.jump ? 2 : 0),
    };
    this.jump = false;
    return result;
  }
}
export interface InputBinding {
  activateDragLook(): void;
  dispose(): void;
}
/** A locked canvas, or an explicitly focused drag-look canvas, owns gameplay input. */
export function bindInput(
  canvas: HTMLCanvasElement,
  state: InputState,
  paused: () => void,
): InputBinding {
  const abort = new AbortController();
  const options = { signal: abort.signal };
  let dragMode = false,
    dragging = false,
    previousX = 0,
    previousY = 0;
  canvas.tabIndex = 0;
  const clear = () => {
    state.clear();
    dragging = false;
    canvas.style.cursor = "";
    paused();
  };
  const ownsFocus = () =>
    document.pointerLockElement === canvas ||
    (dragMode && document.activeElement === canvas);
  document.addEventListener(
    "pointerlockchange",
    () => {
      if (document.pointerLockElement === canvas && !document.hidden) {
        dragMode = false;
        state.active = true;
      } else if (!dragMode) clear();
    },
    options,
  );
  document.addEventListener(
    "visibilitychange",
    () => {
      if (document.hidden) clear();
    },
    options,
  );
  document.addEventListener(
    "focusin",
    () => {
      if (state.active && !ownsFocus()) clear();
    },
    options,
  );
  window.addEventListener("blur", clear, options);
  window.addEventListener(
    "keydown",
    (event) => {
      if (!state.active || !ownsFocus()) return;
      if (event.code === "Escape") {
        clear();
        if (document.pointerLockElement === canvas) document.exitPointerLock();
        return;
      }
      if (state.handles(event.code)) {
        event.preventDefault();
        state.press(event.code, event.repeat);
      }
    },
    options,
  );
  window.addEventListener(
    "keyup",
    (event) => {
      state.release(event.code);
      if (state.active && ownsFocus() && state.handles(event.code))
        event.preventDefault();
    },
    options,
  );
  canvas.addEventListener(
    "mousedown",
    (event) => {
      if (ownsFocus()) state.mouseButton(event.button, true);
      if (dragMode && state.active && event.button === 0 && !state.aiming) {
        dragging = true;
        previousX = event.clientX;
        previousY = event.clientY;
        event.preventDefault();
      }
    },
    options,
  );
  window.addEventListener(
    "mouseup",
    (event) => {
      state.mouseButton(event.button, false);
      dragging = false;
    },
    options,
  );
  document.addEventListener(
    "mousemove",
    (event) => {
      if (document.pointerLockElement === canvas)
        state.look(event.movementX, event.movementY);
      else if (dragMode && dragging && ownsFocus()) {
        state.look(event.clientX - previousX, event.clientY - previousY);
        previousX = event.clientX;
        previousY = event.clientY;
      }
    },
    options,
  );
  canvas.addEventListener(
    "wheel",
    (event) => {
      if (state.active && ownsFocus()) {
        event.preventDefault();
        state.wheel(event.deltaY);
      }
    },
    { ...options, passive: false },
  );
  canvas.addEventListener(
    "contextmenu",
    (event) => {
      if (state.active) event.preventDefault();
    },
    options,
  );
  return {
    activateDragLook() {
      state.clear();
      dragMode = true;
      dragging = false;
      canvas.focus({ preventScroll: true });
      state.active = true;
      canvas.style.cursor = "grab";
    },
    dispose() {
      abort.abort();
      state.clear();
      if (document.pointerLockElement === canvas) document.exitPointerLock();
    },
  };
}
