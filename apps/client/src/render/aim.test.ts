import { expect, it } from "vitest";
import { PerspectiveCamera, Vector3 } from "three";
import { crosshairAim } from "./aim";
import type { RenderActor } from "../world/replica";

const clear = { raycastStatic: () => ({ version: 1 as const, hit: null }) };
function player(position: [number, number, number]): RenderActor {
  const motor = {
    position,
    velocity: [0, 0, 0] as [number, number, number],
    yaw: 0,
    grounded: true,
    jump_cooldown_ticks: 0,
  };
  return {
    id: 2,
    stale: false,
    descriptor: {
      id: 2,
      entity_kind: "player",
      template_id: "player_greybox",
      display_name: "Target",
      tshirt_colour: 0xe4a25b,
    },
    motor,
    state: {
      kind: "character",
      data: { motor, health: 100, mode: "on_foot", vehicle_id: null },
    },
  };
}
function direction(aim: { yaw: number; pitch: number }) {
  return new Vector3(
    -Math.sin(aim.yaw) * Math.cos(aim.pitch),
    Math.sin(aim.pitch),
    -Math.cos(aim.yaw) * Math.cos(aim.pitch),
  );
}
it("converges on close and distant crosshair targets across rotated shoulder views", () => {
  for (const yaw of [0, 0.8, -2.2])
    for (const distance of [2, 5, 20, 80]) {
      const rotation = new Vector3(0, 1, 0);
      const chest = new Vector3(10, 1.45, 20);
      const camera = new PerspectiveCamera(54);
      camera.position
        .copy(chest)
        .add(new Vector3(0.5, 0, 1.65).applyAxisAngle(rotation, yaw));
      camera.lookAt(
        chest.clone().add(new Vector3(0, 0, -80).applyAxisAngle(rotation, yaw)),
      );
      const centre = camera
        .getWorldDirection(new Vector3())
        .multiplyScalar(distance)
        .add(camera.position);
      const target = player([centre.x, 0, centre.z]);
      const aim = crosshairAim(
        camera,
        [10, 0, 20],
        clear,
        [target],
        () => undefined,
      );
      const shot = direction(aim);
      // Distance from the target's capsule centreline to the chest-origin ray.
      const offset = centre.clone().sub(chest);
      expect(offset.clone().cross(shot).length()).toBeLessThan(0.35);
      const end = chest.clone().addScaledVector(shot, offset.dot(shot));
      expect(end.y).toBeCloseTo(1.45);
    }
});
it("uses the nearest surface under the crosshair without targeting through cover", () => {
  const camera = new PerspectiveCamera(54);
  camera.position.set(0.5, 1.45, 1.65);
  camera.lookAt(0, 1.45, -80);
  const view = camera.getWorldDirection(new Vector3());
  const wall = camera.position.clone().addScaledVector(view, 2);
  const target = camera.position.clone().addScaledVector(view, 8);
  const aim = crosshairAim(
    camera,
    [0, 0, 0],
    {
      raycastStatic: () => ({
        version: 1,
        hit: {
          collider_id: 1,
          distance: 2,
          point: wall.toArray(),
          normal: [0, 0, 1],
        },
      }),
    },
    [player([target.x, 0, target.z])],
    () => undefined,
  );
  expect(
    direction(aim).distanceTo(wall.sub(new Vector3(0, 1.45, 0)).normalize()),
  ).toBeLessThan(1e-8);
});
it("ignores dead players and does not attract aim to characters outside the crosshair", () => {
  const camera = new PerspectiveCamera(54);
  camera.position.set(0.5, 1.45, 1.65);
  camera.lookAt(0, 1.45, -80);
  const centre = camera
    .getWorldDirection(new Vector3())
    .multiplyScalar(5)
    .add(camera.position);
  const dead = player([centre.x, 0, centre.z]);
  if (dead.state.kind === "character") dead.state.data.health = 0;
  expect(
    crosshairAim(
      camera,
      [0, 0, 0],
      clear,
      [dead, player([5, 0, -3])],
      () => undefined,
    ),
  ).toEqual(crosshairAim(camera, [0, 0, 0], clear, [], () => undefined));
});
