import { expect, it } from "vitest";
import { PerspectiveCamera, Vector3 } from "three";
import { FollowCamera } from "./camera";
import type { StaticSphereQuery } from "../simulation/bridge";
it("shortens immediately at walls, eases outward, and uses the movement yaw and downward pitch convention", () => {
  const camera = new PerspectiveCamera(65),
    follow = new FollowCamera();
  let hit: number | null = null;
  let query: StaticSphereQuery | undefined;
  const world = {
    spherecastStatic: (q: StaticSphereQuery) => {
      query = q;
      return {
        version: 1,
        hit:
          hit === null
            ? null
            : {
                collider_id: 1,
                distance: hit,
                point: [0, 0, 0] as [number, number, number],
                normal: [0, 0, 1] as [number, number, number],
              },
      };
    },
  };
  const feet = [0, 0, 0];
  const full = follow.update(camera, feet, 0, -0.15, 0.1, world, true);
  expect(query).toMatchObject({ radius: 0.2, origin: [0, 1.45, 0] });
  expect(camera.position.z).toBeGreaterThan(4);
  expect(camera.position.x).toBeCloseTo(0);
  expect(camera.position.y).toBeGreaterThan(1.45);
  camera.updateMatrixWorld();
  const head = new Vector3(0, 1.74, 0).project(camera);
  const foot = new Vector3(0, 0, 0).project(camera);
  expect(head.x).toBeCloseTo(0, 8);
  expect(head.y).toBeLessThan(0);
  expect(Math.abs(head.y)).toBeLessThan(0.03);
  expect(foot.y).toBeLessThan(-0.5);
  hit = 0.7;
  expect(follow.update(camera, feet, 0, -0.15, 0.016, world)).toBeCloseTo(0.6);
  hit = null;
  const eased = follow.update(camera, feet, 0, -0.15, 0.016, world);
  expect(eased).toBeGreaterThan(0.6);
  expect(eased).toBeLessThan(full);
  follow.update(camera, feet, Math.PI / 2, 0.3, 0.1, world, true);
  expect(camera.position.x).toBeGreaterThan(4);
  expect(camera.position.y).toBeLessThan(1.45);
  // The character's vertical centreline stays horizontally centred during orbit,
  // pitch changes and collision shortening, including away from the world origin.
  for (const yaw of [0, 0.7, -1.8, Math.PI]) {
    for (const pitch of [-0.8, -0.15, 0.5]) {
      for (const obstruction of [null, 0.7]) {
        hit = obstruction;
        follow.update(camera, [12, 0.02, -25], yaw, pitch, 0.016, world, true);
        camera.updateMatrixWorld();
        const projected = new Vector3(12, 0.92, -25).project(camera);
        expect(projected.x).toBeCloseTo(0, 8);
      }
    }
  }
  hit = 0;
  expect(follow.update(camera, feet, 0, 0, 0.016, world)).toBe(0);
});

it("lifts the van driving view above its taller body while keeping compact and walking framing", () => {
  const camera = new PerspectiveCamera(65),
    follow = new FollowCamera();
  let origin: number[] = [];
  const world = {
    spherecastStatic: (q: StaticSphereQuery) => {
      origin = q.origin;
      return { version: 1, hit: null };
    },
  };
  follow.update(camera, [0, 0, 0], 0, -0.15, 0.1, world, true, true, 2.2);
  expect(origin[1]).toBeCloseTo(1.9);
  expect(camera.position.y).toBeGreaterThan(2.9);
  camera.updateMatrixWorld();
  expect(new Vector3(0, 2.3, 0).project(camera).y).toBeCloseTo(0);
  follow.update(camera, [0, 0, 0], 0, -0.15, 0.1, world, true, true, 1.4);
  expect(origin[1]).toBeCloseTo(1.1);
});

it("aligns aim view with the authoritative chest-origin ray", () => {
  const camera = new PerspectiveCamera(65),
    follow = new FollowCamera();
  const yaw = 0.8,
    pitch = -0.2,
    feet = [10, 0, 20];
  const distance = follow.update(
    camera,
    feet,
    yaw,
    pitch,
    0.1,
    { spherecastStatic: () => ({ version: 1, hit: null }) },
    true,
    false,
    1.4,
    true,
  );
  expect(distance).toBeLessThanOrEqual(0.1);
  const direction = new Vector3();
  camera.getWorldDirection(direction);
  expect(direction.x).toBeCloseTo(-Math.sin(yaw) * Math.cos(pitch));
  expect(direction.y).toBeCloseTo(Math.sin(pitch));
  expect(direction.z).toBeCloseTo(-Math.cos(yaw) * Math.cos(pitch));
});
