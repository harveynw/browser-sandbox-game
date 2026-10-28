import { expect, it } from "vitest";
import { AnimationStateMachine, carLean } from "./character";
import type { CharacterMotor } from "@port-meridian/protocol";
const motor: CharacterMotor = {
  position: [0, 0, 0],
  velocity: [0, 0, 0],
  yaw: 0,
  grounded: true,
  jump_cooldown_ticks: 0,
};
it("blends locomotion and observes jump/land edges once across repeated samples", () => {
  const a = new AnimationStateMachine();
  expect(a.update({ motor }, 0.1, 0)).toBe("idle");
  expect(a.update({ motor: { ...motor, velocity: [0, 0, 6] } }, 0.1, 0.1)).toBe(
    "walk",
  );
  a.update({ motor: { ...motor, velocity: [0, 0, 6] } }, 0.1, 0.2);
  expect(a.update({ motor: { ...motor, velocity: [0, 0, 6] } }, 0.1, 0.3)).toBe(
    "run",
  );
  const air = { ...motor, grounded: false };
  expect(a.update({ motor: air }, 0.01, 0.4)).toBe("jump_start");
  expect(a.update({ motor: air }, 0.1, 0.6)).toBe("fall");
  expect(a.update({ motor }, 0.1, 0.7)).toBe("land");
  expect(a.update({ motor }, 0.1, 0.9)).not.toBe("land");
});
it("prioritizes disabled/driving/reload and clamps cosmetic lean without changing motor", () => {
  const a = new AnimationStateMachine();
  expect(a.update({ motor, incapacitated: true, aiming: true }, 0.1, 0)).toBe(
    "incapacitated",
  );
  expect(a.update({ motor, driving: true }, 0.1, 1)).toBe("driving");
  expect(a.update({ motor, reloading: true }, 0.1, 2)).toBe("reload");
  expect(Math.abs(carLean(80, 0.6, false))).toBeCloseTo(Math.PI / 45);
  expect(carLean(80, 0.6, true)).toBe(0);
  expect(motor.position).toEqual([0, 0, 0]);
});

it("fits seated heads inside all three authored car heights", async () => {
  const { Box3, BoxGeometry, Group, MeshStandardMaterial } =
    await import("three");
  const { CharacterRig } = await import("./character");
  for (const [height, seat] of [
    [1.4, 0.8],
    [2.2, 1],
    [1.25, 0.7],
  ]) {
    const cube = new BoxGeometry(),
      material = new MeshStandardMaterial(),
      rig = new CharacterRig(cube, () => material),
      group = new Group();
    group.add(rig.root);
    group.scale.setScalar(0.6);
    group.position.y = seat ?? 0;
    rig.pose({ motor, driving: true }, 0.016, 0, true);
    group.updateMatrixWorld(true);
    expect(new Box3().setFromObject(group).max.y).toBeLessThan(height ?? 0);
    cube.dispose();
    material.dispose();
  }
});

it("applies the same shirt RGB to local and remote rigs without recolouring skin or shared materials", async () => {
  const { BoxGeometry, Mesh, MeshStandardMaterial } = await import("three");
  const { CharacterRig } = await import("./character");
  const cube = new BoxGeometry();
  const materials = new Map<
    string,
    InstanceType<typeof MeshStandardMaterial>
  >();
  const material = (colour: string) => {
    let value = materials.get(colour);
    if (!value) {
      value = new MeshStandardMaterial({ color: colour });
      materials.set(colour, value);
    }
    return value;
  };
  const local = new CharacterRig(cube, material, 0);
  const remote = new CharacterRig(cube, material, 2);
  const untouched = new CharacterRig(cube, material, 0);
  local.setShirtColour(0x123abc);
  remote.setShirtColour(0x123abc);
  const colours = (rig: InstanceType<typeof CharacterRig>) => {
    const values: string[] = [];
    rig.root.traverse((part) => {
      if (part instanceof Mesh && part.material instanceof MeshStandardMaterial)
        values.push(part.material.color.getHexString());
    });
    return values;
  };
  expect(colours(local)).toContain("123abc");
  expect(colours(remote)).toContain("123abc");
  expect(colours(local)).toContain("c79370");
  expect(colours(remote)).toContain("dbb18a");
  expect(colours(untouched)).not.toContain("123abc");
  cube.dispose();
  for (const value of materials.values()) value.dispose();
});
