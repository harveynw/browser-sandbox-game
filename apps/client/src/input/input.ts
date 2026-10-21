import type { InputFrame } from "@port-meridian/protocol";
const bound = new Set([
  "KeyW",
  "KeyA",
  "KeyS",
  "KeyD",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ShiftLeft",
  "ShiftRight",
  "Space",
  "KeyE",
]);
export class InputState {
  readonly keys = new Set<string>();
  yaw = 0;
  pitch = -0.15;
  sensitivity = 0.002;
  invert = false;
  active = false;
  private jump = false;
  private interact = false;
  press(code: string, repeat = false): void {
    if (!this.active || !bound.has(code)) return;
    this.keys.add(code);
    if (code === "Space" && !repeat) this.jump = true;
    if (code === "KeyE" && !repeat) this.interact = true;
  }
  release(code: string): void {
    this.keys.delete(code);
  }
  clear(): void {
    this.keys.clear();
    this.jump = false;
    this.interact = false;
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
  consumeInteract(): boolean {
    const value = this.active && this.interact;
    this.interact = false;
    return value;
  }
  sample(driving = false): Omit<InputFrame, "tick"> {
    const down = (...codes: string[]) =>
      this.active && codes.some((c) => this.keys.has(c));
    const result = {
      move_x:
        (Number(down("KeyD", "ArrowRight")) -
          Number(down("KeyA", "ArrowLeft"))) *
        32767,
      move_z:
        (Number(down("KeyW", "ArrowUp")) - Number(down("KeyS", "ArrowDown"))) *
        32767,
      look_yaw: this.yaw,
      look_pitch: -this.pitch,
      buttons:
        (down("ShiftLeft", "ShiftRight") ? 1 : 0) |
        (driving ? (down("Space") ? 4 : 0) : this.active && this.jump ? 2 : 0),
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
      if (bound.has(event.code)) {
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
      if (state.active && ownsFocus() && bound.has(event.code))
        event.preventDefault();
    },
    options,
  );
  canvas.addEventListener(
    "mousedown",
    (event) => {
      if (dragMode && state.active && event.button === 0) {
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
    () => {
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
