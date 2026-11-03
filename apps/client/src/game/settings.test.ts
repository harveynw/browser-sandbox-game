import { expect, it } from "vitest";
import { defaults, readSettings, rebind, controlText } from "./settings";
import { InputState } from "../input/input";
it("loads bounded settings and rejects corrupt or conflicting bindings", () => {
  expect(readSettings({ getItem: () => "{" }, true).reduced).toBe(true);
  const v = readSettings({
    getItem: () =>
      JSON.stringify({
        version: 1,
        master: 4,
        sensitivity: 1,
        bindings: { ...defaults, job: "KeyE" },
      }),
  });
  expect(v.master).toBe(1);
  expect(v.sensitivity).toBe(0.006);
  expect(v.bindings.job).toBe("KeyF");
});
it("swaps bindings atomically, updates prompts and never leaves old movement or edges held", () => {
  const input = new InputState();
  input.bindings = rebind({ ...defaults }, "forward", "KeyF");
  input.active = true;
  input.press("KeyW");
  expect(input.sample().move_z).toBe(0);
  expect(input.consumeJob()).toBe(true);
  input.release("KeyW");
  input.press("KeyF");
  expect(input.sample().move_z).toBe(32767);
  expect(input.consumeJob()).toBe(false);
  input.press("KeyW");
  expect(input.consumeJob()).toBe(true);
  input.clear();
  input.active = true;
  input.press("KeyM");
  expect(input.consumeMap()).toBe(true);
  expect(input.consumeMap()).toBe(false);
  expect(controlText("F jobs · E enter", input.bindings)).toBe(
    "W jobs · E enter",
  );
  expect(rebind(input.bindings, "job", "Escape")).toBe(input.bindings);
});

it("restores valid shirt colours and rejects malformed saved colours", () => {
  const saved = (colour: unknown) => ({
    getItem: () => JSON.stringify({ version: 1, tshirtColour: colour }),
  });
  expect(readSettings(saved("#123ABC")).tshirtColour).toBe("#123abc");
  for (const value of ["red", "#12345", "#1234567", 123])
    expect(readSettings(saved(value)).tshirtColour).toBe("#e4a25b");
});

it("migrates old bindings without losing a custom key that conflicts with a new slot default", () => {
  const { weapon1: _one, weapon2: _two, weapon3: _three, ...old } = defaults;
  void _one;
  void _two;
  void _three;
  const settings = readSettings({
    getItem: () =>
      JSON.stringify({ version: 1, bindings: { ...old, jump: "Digit1" } }),
  });
  expect(settings.bindings.jump).toBe("Digit1");
  expect(settings.bindings.weapon1).not.toBe("Digit1");
  expect(new Set(Object.values(settings.bindings)).size).toBe(
    Object.keys(defaults).length,
  );
});
