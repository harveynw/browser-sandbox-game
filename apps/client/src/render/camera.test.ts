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
  expect(distance).toBeGreaterThan(1.6);
  expect(distance).toBeLessThan(1.8);
  const direction = new Vector3();
  camera.getWorldDirection(direction);
  camera.updateMatrixWorld();
  const rayTarget = new Vector3(...feet)
    .add(new Vector3(0, 1.45, 0))
    .addScaledVector(
      new Vector3(
        -Math.sin(yaw) * Math.cos(pitch),
        Math.sin(pitch),
        -Math.cos(yaw) * Math.cos(pitch),
      ),
      80,
    );
  const projected = rayTarget.project(camera);
  expect(projected.x).toBeCloseTo(0, 6);
  expect(projected.y).toBeCloseTo(0, 6);
});

it("eases field of view between walking/driving/aim and snaps presentation when reduced motion is selected", () => {
  const camera = new PerspectiveCamera(65),
    follow = new FollowCamera(),
    world = { spherecastStatic: () => ({ version: 1 as const, hit: null }) };
  follow.update(camera, [0, 0, 0], 0, 0, 0.016, world, false, true);
  expect(camera.fov).toBeGreaterThan(65);
  expect(camera.fov).toBeLessThan(70);
  follow.update(camera, [0, 0, 0], 0, 0, 0.016, world, true, false, 1.4, true);
  expect(camera.fov).toBe(54);
});

it("centres a near-wall chest-ray obstruction in the shoulder view", () => {
  const camera = new PerspectiveCamera(65),
    follow = new FollowCamera();
  const world = {
    spherecastStatic: () => ({ version: 1 as const, hit: null }),
    raycastStatic: () => ({
      version: 1 as const,
      hit: {
        collider_id: 1,
        distance: 0.4,
        point: [0, 1.45, -0.4] as [number, number, number],
        normal: [0, 0, 1] as [number, number, number],
      },
    }),
  };
  follow.update(camera, [0, 0, 0], 0, 0, 0.016, world, true, false, 1.4, true);
  camera.updateMatrixWorld();
  const hit = new Vector3(0, 1.45, -0.4).project(camera);
  expect(hit.x).toBeCloseTo(0);
  expect(hit.y).toBeCloseTo(0);
});
