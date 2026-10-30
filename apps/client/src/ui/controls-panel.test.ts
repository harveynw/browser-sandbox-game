import { expect, it } from "vitest";
import { defaults, rebind } from "../game/settings";
import { controlGroups } from "./controls-panel";

it("shows movement, vehicle, combat and menu controls with their current bindings", () => {
  const groups = controlGroups(defaults);
  expect(groups.map((group) => group.title)).toEqual([
    "Movement",
    "Vehicles",
    "Combat",
    "Chat",
    "World & menus",
  ]);
  const rows = groups.flatMap((group) => group.rows);
  expect(rows).toContainEqual(["W / A / S / D · Arrows", "Move / steer"]);
  expect(rows).toContainEqual(["E", "Enter / exit vehicle"]);
  expect(rows).toContainEqual(["RMB / Q", "Hold / toggle aim"]);
  expect(rows).toContainEqual(["LMB / X", "Fire while aiming"]);
  expect(rows).toContainEqual(["Esc", "Pause / menu"]);
});

it("refreshes remapped keys without retaining old defaults", () => {
  const rows = controlGroups(rebind(defaults, "interact", "KeyZ")).flatMap(
    (group) => group.rows,
  );
  expect(rows).toContainEqual(["Z", "Enter / exit vehicle"]);
  expect(rows).not.toContainEqual(["E", "Enter / exit vehicle"]);
});
