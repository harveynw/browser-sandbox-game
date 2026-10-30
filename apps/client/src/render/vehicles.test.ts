import { expect, it } from "vitest";
import { Box3, Group, Mesh, Vector3 } from "three";
import type { VehicleTemplate } from "@port-meridian/content";
import { VehicleModels, wheelRollDelta, steerWheels } from "./vehicles";
import city from "../../../../content/source/city.json";

it("shares merged geometry while keeping each vehicle's steering and body transforms independent", () => {
  const models = new VehicleModels();
  for (const template of city.vehicle_templates as VehicleTemplate[]) {
    const a = models.create(template, "#338899", false);
    const b = models.create(template, "#338899", false);
    expect(a.body).not.toBe(b.body);
    expect(a.body.children.length).toBeLessThanOrEqual(7);
    for (let i = 0; i < a.body.children.length; i++) {
      const first = a.body.children[i] as Mesh;
      const second = b.body.children[i] as Mesh;
      expect(first.geometry).toBe(second.geometry);
      expect(first.material).toBe(second.material);
      expect(
        Array.from(first.geometry.getAttribute("position").array).every(
          Number.isFinite,
        ),
      ).toBe(true);
    }
    const group = new Group();
    group.add(a.body, ...a.wheels);
    const bounds = new Box3().setFromObject(group);
    expect(bounds.min.y).toBeGreaterThanOrEqual(0);
    expect(bounds.max.y).toBeLessThanOrEqual(template.dimensions[1] + 0.01);
    expect(a.wheels).toHaveLength(4);
    const wheelA = a.wheels[0];
    const wheelB = b.wheels[0];
    if (!wheelA || !wheelB || !wheelA.children[0] || !wheelB.children[0])
      throw new Error("Missing animated wheel");
    wheelA.rotation.y = 0.4;
    wheelA.children[0].rotation.x = 1;
    expect(wheelB.rotation.y).toBeCloseTo(0);
    expect(wheelB.children[0].rotation.x).toBeCloseTo(0);
    expect(a.wheels.map((wheel) => wheel.position.z)).toEqual([
      -template.wheelbase / 2,
      template.wheelbase / 2,
      -template.wheelbase / 2,
      template.wheelbase / 2,
    ]);
  }
  models.dispose();
});

it("spins from predicted travel without a remote speed sample, reversing with travel direction", () => {
  const origin = new Vector3();
  expect(wheelRollDelta(origin, new Vector3(0, 0, -0.34), 0)).toBeCloseTo(-1);
  expect(wheelRollDelta(origin, new Vector3(0, 0, 0.34), 0)).toBeCloseTo(1);
  expect(
    wheelRollDelta(origin, new Vector3(-0.34, 0, 0), Math.PI / 2),
  ).toBeCloseTo(-1);
  expect(wheelRollDelta(origin, origin, 0)).toBe(0);
  expect(wheelRollDelta(origin, new Vector3(0.34, 0, 0), 0)).toBe(0);
  expect(wheelRollDelta(origin, new Vector3(0, 0, -6), 0)).toBe(0);
});

it("steers both front wheels while rear wheels and independent rolling stay intact", () => {
  const models = new VehicleModels();
  const { wheels } = models.create(undefined, "#338899", false);
  for (const wheel of wheels) {
    const roller = wheel.children[0];
    if (roller) roller.rotation.x = 1.25;
  }
  for (const angle of [-0.5, 0.5, 0]) {
    steerWheels(wheels, angle);
    expect(wheels.map((w) => w.rotation.y)).toEqual([angle, 0, angle, 0]);
    expect(wheels.map((w) => w.children[0]?.rotation.x)).toEqual([
      1.25, 1.25, 1.25, 1.25,
    ]);
  }
  models.dispose();
});
