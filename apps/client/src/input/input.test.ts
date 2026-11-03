import { describe, expect, it } from "vitest";
import { InputState } from "./input";
describe("game controls", () => {
  it("ignores menu keys, clears held input and jump on focus loss, and requires capture again", () => {
    const input = new InputState();
    input.press("KeyW");
    expect(input.sample().move_z).toBe(0);
    input.active = true;
    input.press("KeyW");
    input.press("ShiftLeft");
    input.press("Space");
    expect(input.sample()).toMatchObject({ move_z: 32767, buttons: 3 });
    expect(input.sample().buttons).toBe(1);
    input.press("Space", true);
    expect(input.sample().buttons).toBe(1);
    input.clear();
    expect(input.sample()).toMatchObject({ move_z: 0, buttons: 0 });
    input.press("KeyW");
    expect(input.sample().move_z).toBe(0);
    input.active = true;
    expect(input.sample().move_z).toBe(0);
  });
  it("cancels opposite axes and bounds orbit/aim across large mouse deltas and inversion", () => {
    const input = new InputState();
    input.active = true;
    input.press("KeyA");
    input.press("KeyD");
    input.press("KeyS");
    expect(input.sample()).toMatchObject({ move_x: 0, move_z: -32767 });
    input.release("KeyD");
    expect(input.sample().move_x).toBe(-32767);
    input.look(1e6, 1e6);
    expect(input.yaw).toBeGreaterThanOrEqual(-Math.PI);
    expect(input.yaw).toBeLessThanOrEqual(Math.PI);
    expect(input.pitch).toBe(0.65);
    input.invert = true;
    input.look(0, 1e6);
    expect(input.pitch).toBe(-1.1);
    input.clear();
    const yaw = input.yaw;
    input.look(100, 100);
    expect(input.yaw).toBe(yaw);
  });
});

it("releases drag-look controls on blur/menu focus and removes handlers across two mounts", async () => {
  const { bindInput } = await import("./input");
  const { vi } = await import("vitest");
  const doc = Object.assign(new EventTarget(), {
    pointerLockElement: null as unknown,
    activeElement: null as unknown,
    hidden: false,
  });
  const win = new EventTarget();
  const canvas = Object.assign(new EventTarget(), {
    tabIndex: -1,
    style: { cursor: "" },
    focus: () => {
      doc.activeElement = canvas;
    },
  });
  vi.stubGlobal("document", doc);
  vi.stubGlobal("window", win);
  const key = (code: string) => {
    const e = Object.assign(new Event("keydown", { cancelable: true }), {
      code,
      repeat: false,
    });
    win.dispatchEvent(e);
    return e;
  };
  try {
    const old = new InputState();
    const pause = vi.fn();
    const first = bindInput(canvas as unknown as HTMLCanvasElement, old, pause);
    first.activateDragLook();
    expect(key("KeyW").defaultPrevented).toBe(true);
    expect(old.sample().move_z).toBe(32767);
    doc.activeElement = {};
    doc.dispatchEvent(new Event("focusin"));
    expect(old.sample().move_z).toBe(0);
    expect(key("Space").defaultPrevented).toBe(false);
    // A genuine lock and lock loss use the same neutralization boundary.
    doc.pointerLockElement = canvas;
    doc.dispatchEvent(new Event("pointerlockchange"));
    key("KeyW");
    expect(old.sample().move_z).toBe(32767);
    doc.pointerLockElement = null;
    doc.dispatchEvent(new Event("pointerlockchange"));
    expect(old.sample().move_z).toBe(0);
    first.activateDragLook();
    doc.hidden = true;
    doc.dispatchEvent(new Event("visibilitychange"));
    expect(old.active).toBe(false);
    doc.hidden = false;
    first.activateDragLook();
    key("Space");
    win.dispatchEvent(new Event("blur"));
    expect(old.sample().buttons).toBe(0);
    first.dispose();
    const count = pause.mock.calls.length;
    const current = new InputState();
    const second = bindInput(
      canvas as unknown as HTMLCanvasElement,
      current,
      () => {},
    );
    second.activateDragLook();
    key("KeyD");
    expect(current.sample().move_x).toBe(32767);
    expect(old.sample().move_x).toBe(0);
    key("Escape");
    expect(current.active).toBe(false);
    expect(pause.mock.calls.length).toBe(count);
    second.dispose();
    expect(key("Space").defaultPrevented).toBe(false);
  } finally {
    vi.unstubAllGlobals();
  }
});

it("edges interaction requests, holds the driving handbrake, and clears both on pause", () => {
  const input = new InputState();
  input.press("KeyE");
  expect(input.consumeInteract()).toBe(false);
  input.active = true;
  input.press("KeyE");
  expect(input.consumeInteract()).toBe(true);
  expect(input.consumeInteract()).toBe(false);
  input.press("KeyE", true);
  expect(input.consumeInteract()).toBe(false);
  input.press("Space");
  expect(input.sample(true).buttons).toBe(4);
  expect(input.sample(true).buttons).toBe(4);
  input.clear();
  input.active = true;
  expect(input.sample(true).buttons).toBe(0);
  expect(input.consumeInteract()).toBe(false);
});

it("edges terminal interaction and clears it on pause without changing movement", () => {
  const input = new InputState();
  input.press("KeyF");
  expect(input.consumeJob()).toBe(false);
  input.active = true;
  input.press("KeyF");
  expect(input.consumeJob()).toBe(true);
  expect(input.consumeJob()).toBe(false);
  input.press("KeyF", true);
  expect(input.consumeJob()).toBe(false);
  input.release("KeyF");
  input.press("KeyF");
  input.clear();
  expect(input.consumeJob()).toBe(false);
});

it("requires active aim for fire edges and clears weapon actions on blur", () => {
  const input = new InputState();
  input.mouseButton(2, true);
  input.mouseButton(0, true);
  input.press("KeyR");
  expect(input.consumeFire()).toBe(false);
  expect(input.consumeReload()).toBe(false);
  input.active = true;
  input.mouseButton(0, true);
  expect(input.consumeFire()).toBe(false);
  input.mouseButton(2, true);
  input.mouseButton(0, true);
  expect(input.consumeFire()).toBe(true);
  expect(input.consumeFire()).toBe(false);
  expect(input.sample().buttons & 8).toBe(8);
  input.press("KeyR");
  input.press("KeyR", true);
  expect(input.consumeReload()).toBe(true);
  expect(input.consumeReload()).toBe(false);
  input.mouseButton(2, false);
  expect(input.aiming).toBe(false);
  input.press("KeyQ");
  input.press("KeyQ", true);
  expect(input.aiming).toBe(true);
  input.mouseButton(0, true);
  input.press("KeyR");
  input.clear();
  input.active = true;
  expect(input.consumeFire()).toBe(false);
  expect(input.consumeReload()).toBe(false);
  expect(input.aiming).toBe(false);
});

it("keeps automatic intent held while single-shot edges remain one-shot and requires release after interruption", () => {
  const input = new InputState();
  input.active = true;
  input.mouseButton(2, true);
  input.mouseButton(0, true);
  expect(input.consumeFire()).toBe(true);
  expect(input.consumeFire()).toBe(false);
  expect(input.triggerHeld).toBe(true);
  input.stopFiring();
  expect(input.triggerHeld).toBe(false);
  input.mouseButton(0, true);
  expect(input.triggerHeld).toBe(false);
  input.mouseButton(0, false);
  input.mouseButton(0, true);
  expect(input.triggerHeld).toBe(true);
  input.mouseButton(2, false);
  input.mouseButton(2, true);
  expect(input.triggerHeld).toBe(false);
  input.clear();
  expect(input.triggerHeld).toBe(false);
});
it("queues slot and wheel selection only during active gameplay", () => {
  const input = new InputState();
  input.press("Digit2");
  input.wheel(1);
  expect(input.consumeWeapon()).toBeNull();
  expect(input.consumeCycle()).toBe(0);
  input.active = true;
  input.press("Digit3");
  expect(input.consumeWeapon()).toBe(2);
  expect(input.consumeWeapon()).toBeNull();
  input.wheel(-1);
  expect(input.consumeCycle()).toBe(-1);
  input.clear();
  expect(input.consumeCycle()).toBe(0);
});
