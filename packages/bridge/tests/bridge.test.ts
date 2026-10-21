import { readFile } from "node:fs/promises";
import { beforeAll, describe, expect, it } from "vitest";
import { encode } from "@msgpack/msgpack";
import init, { WasmPredictionWorld } from "../../sim-wasm/sim_wasm";
import {
  BridgeError,
  PredictionWorld,
  type PredictedFrame,
} from "../../../apps/client/src/simulation/bridge";
import { runCharacterTrace } from "../../../apps/client/src/simulation/trace";
import fixture from "../../../tests/fixtures/simulation/character-trace.json";
import source from "../../../content/source/slice.json";
import type { CollisionPackage, VehicleTemplate } from "@port-meridian/content";
import type { CharacterMotor } from "@port-meridian/protocol";

const initial: PredictedFrame = {
  version: 1,
  tick: 0,
  control_epoch: 2,
  controlled_entity_id: 7,
  motor: fixture.initial_motor as CharacterMotor,
};
const input = {
  tick: 1,
  move_x: 0,
  move_z: 32767,
  look_yaw: 0,
  look_pitch: 0,
  buttons: 0,
};
async function ready(): Promise<PredictionWorld> {
  const world = await PredictionWorld.create();
  world.loadCollision(
    fixture.collision as CollisionPackage,
    fixture.sim_config,
  );
  world.reset(initial);
  return world;
}
beforeAll(async () => {
  await init({
    module_or_path: await readFile(
      new URL("../../sim-wasm/sim_wasm_bg.wasm", import.meta.url),
    ),
  });
});
describe("actual release WASM bridge", () => {
  it("matches all native checkpoints through serialized one-step calls", async () => {
    const result = await runCharacterTrace();
    expect(result.ticks).toBe(1800);
    expect(result.checkpoints).toBe(13);
    expect(result.maxPositionError).toBeLessThanOrEqual(0.01);
    expect(result.maxVelocityError).toBeLessThanOrEqual(0.01);
  });
  it("rejects missing initialization, invalid DTOs and duplicate/future ticks without mutation", async () => {
    const empty = await PredictionWorld.create();
    expect(() => empty.currentState()).toThrow(BridgeError);
    expect(() => empty.reset(initial)).toThrow("collision not loaded");
    empty.free();
    const world = await ready();
    try {
      expect(() => world.step({ ...input, tick: 2 })).toThrow(
        "identity mismatch",
      );
      expect(world.currentState()).toEqual(initial);
      expect(() => world.step({ ...input, move_x: -32768 })).toThrow(
        "Invalid bridge DTO",
      );
      world.step(input);
      const after = world.currentState();
      expect(() => world.step(input)).toThrow("identity mismatch");
      expect(world.currentState()).toEqual(after);
    } finally {
      world.free();
    }
  });
  it("preserves world/state on invalid reload/reset, supports epoch reset, and frees idempotently", async () => {
    const world = await ready();
    try {
      world.step(input);
      const after = world.currentState();
      expect(() =>
        world.loadCollision(
          { ...(fixture.collision as CollisionPackage), schema_version: 2 },
          fixture.sim_config,
        ),
      ).toThrow();
      expect(() =>
        world.reset({
          ...initial,
          motor: { ...initial.motor, position: [0, -0.5, 0] },
        }),
      ).toThrow();
      expect(world.currentState()).toEqual(after);
      world.reset({ ...initial, tick: 20, control_epoch: 3 });
      expect(world.step({ ...input, tick: 21 }).control_epoch).toBe(3);
      world.loadCollision(
        fixture.collision as CollisionPackage,
        fixture.sim_config,
      );
      expect(() => world.currentState()).toThrow("motor not reset");
    } finally {
      world.free();
      world.free();
    }
    expect(() => world.currentState()).toThrow("freed");
  });
  it("supports bounded static rays and vehicle proxy collision", async () => {
    const world = await ready();
    try {
      world.setVehicleProxies({ version: 1, proxies: [] });
      world.setVehicleProxies({
        version: 1,
        proxies: [
          {
            entity_id: 9,
            template: source.vehicle_templates[0] as VehicleTemplate,
            motor: {
              position: [4, 0, 0],
              velocity: [0, 0, 0],
              yaw: 0,
              speed: 0,
              steering: 0,
            },
          },
        ],
      });
      expect(() =>
        world.reset({
          ...initial,
          motor: { ...initial.motor, position: [4, 0, 0] },
        }),
      ).not.toThrow();
      world.setVehicleProxies({ version: 1, proxies: [] });
      expect(() =>
        world.reset({
          ...initial,
          motor: { ...initial.motor, position: [4, 0, 0] },
        }),
      ).not.toThrow();
      const hit = world.raycastStatic({
        version: 1,
        origin: [0, 1, 0],
        direction: [0, 0, -2],
        max_distance: 20,
      }).hit;
      expect(hit?.distance).toBeCloseTo(7.9, 4);
      expect(
        world.raycastStatic({
          version: 1,
          origin: [0, 1, 0],
          direction: [0, 1, 0],
          max_distance: 20,
        }).hit,
      ).toBeNull();
      expect(() =>
        world.raycastStatic({
          version: 1,
          origin: [0, 1, 0],
          direction: [0, 0, 0],
          max_distance: 20,
        }),
      ).toThrow();
      expect(() =>
        world.raycastStatic({
          version: 1,
          origin: [0, 1, 0],
          direction: [0, 0, 1],
          max_distance: NaN,
        }),
      ).toThrow();
      expect(() =>
        world.setVehicleProxies({ version: 2, proxies: [] }),
      ).toThrow();
    } finally {
      world.free();
    }
  });
  it("Rust boundary rejects malformed/oversized/trailing buffers and stale entity identities", () => {
    const raw = new WasmPredictionWorld();
    try {
      raw.load_collision(encode(fixture.collision), encode(fixture.sim_config));
      raw.reset(encode(initial));
      const before = raw.current_state();
      for (const bytes of [
        new Uint8Array(),
        new Uint8Array(16385),
        new Uint8Array([0xc1]),
        new Uint8Array([...encode(initial), 0]),
      ]) {
        expect(() => raw.reset(bytes)).toThrow();
        expect(raw.current_state()).toEqual(before);
      }
      for (const request of [
        { version: 1, control_epoch: 3, controlled_entity_id: 7, input },
        { version: 1, control_epoch: 2, controlled_entity_id: 8, input },
        {
          version: 1,
          control_epoch: 2,
          controlled_entity_id: 7,
          input,
          extra: true,
        },
      ])
        expect(() => raw.step(encode(request))).toThrow();
      const proxyMotor = {
        position: [0, 0, 0],
        speed: 0,
        yaw: 0,
        steering: 0,
        velocity: [0, 0, 0],
      };
      // A missing template is rejected without changing the active motor.
      expect(() =>
        raw.set_vehicle_proxies(
          encode({
            version: 1,
            proxies: [
              { entity_id: 1, template_id: "compact", motor: proxyMotor },
            ],
          }),
        ),
      ).toThrow();
      expect(raw.current_state()).toEqual(before);
    } finally {
      raw.free();
    }
  });
});

it("sweeps a camera sphere through the real WASM BVH and rejects invalid radius without changing the motor", async () => {
  const world = await ready();
  try {
    const ray = {
      version: 1,
      origin: [0, 2, 0] as [number, number, number],
      direction: [0, 0, -1] as [number, number, number],
      max_distance: 10,
    };
    const point = world.raycastStatic(ray).hit;
    const sphere = world.spherecastStatic({ ...ray, radius: 0.2 }).hit;
    expect(point).not.toBeNull();
    expect(sphere).not.toBeNull();
    if (!point || !sphere) throw new Error("Expected wall hit");
    expect(sphere.distance).toBeCloseTo(point.distance - 0.2, 3);
    expect(() => world.spherecastStatic({ ...ray, radius: -1 })).toThrow();
    expect(() => world.spherecastStatic({ ...ray, radius: NaN })).toThrow();
    // Ground is a physical collider with flags=1, not a vision-only blocker.
    const floor = world.spherecastStatic({
      ...ray,
      origin: [0, 1.45, 0],
      direction: [0, -1, 0],
      radius: 0.2,
    }).hit;
    expect(floor?.distance).toBeCloseTo(1.25, 3);
    expect(world.currentState()).toEqual(initial);
  } finally {
    world.free();
  }
});
