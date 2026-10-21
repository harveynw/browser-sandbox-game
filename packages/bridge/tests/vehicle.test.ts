import { readFile } from "node:fs/promises";
import { beforeAll, expect, it } from "vitest";
import init from "../../sim-wasm/sim_wasm";
import { PredictionWorld } from "../../../apps/client/src/simulation/bridge";
import {
  LocalPrediction,
  actorMotor,
} from "../../../apps/client/src/simulation/prediction";
import { ServerClock } from "../../../apps/client/src/net/clock";
import type { CollisionPackage, VehicleTemplate } from "@port-meridian/content";
import type { OwnerState, VehicleMotor } from "@port-meridian/protocol";
import fixture from "../../../tests/fixtures/simulation/vehicle-trace.json";
import contentIndex from "../../../apps/client/public/content/slice.json";
import { InputState } from "../../../apps/client/src/input/input";
const initial = fixture.initial_motor as VehicleMotor;
beforeAll(async () => {
  await init({
    module_or_path: await readFile(
      new URL("../../sim-wasm/sim_wasm_bg.wasm", import.meta.url),
    ),
  });
});
async function world() {
  const w = await PredictionWorld.create();
  w.loadCollision(fixture.collision as CollisionPackage, fixture.sim_config);
  return w;
}
it("holds and releases the handbrake through real driving controls and WASM despite held throttle", async () => {
  for (const direction of [-1, 1]) {
    const w = await world();
    try {
      const input = new InputState();
      input.active = true;
      input.press(direction > 0 ? "KeyW" : "KeyS");
      input.press("Space");
      const motor: VehicleMotor = {
        ...initial,
        position: [0, 0, 0],
        velocity: [0, 0, -direction * 6],
        speed: direction * 6,
        yaw: 0,
        steering: 0,
      };
      w.reset({
        version: 1,
        tick: 0,
        control_epoch: 2,
        controlled_entity_id: 9,
        motor: actorMotor(motor),
        vehicle: { motor, template: fixture.template as VehicleTemplate },
      });
      for (let tick = 1; tick <= 30; tick++)
        w.step({ tick, ...input.sample(true) });
      const stopped = w.currentState();
      expect(Math.abs(stopped.vehicle?.motor.speed ?? NaN)).toBe(0);
      expect(Math.abs(stopped.motor.position[2])).toBeLessThan(2);
      for (let tick = 31; tick <= 60; tick++)
        w.step({ tick, ...input.sample(true) });
      expect(w.currentState().motor.position).toEqual(stopped.motor.position);
      input.release("Space");
      const resumed = w.step({ tick: 61, ...input.sample(true) });
      expect((resumed.vehicle?.motor.speed ?? NaN) * direction).toBeGreaterThan(
        0.2,
      );
    } finally {
      w.free();
    }
  }
});
it("keeps the real parking-to-street route flat through coasting and braking", async () => {
  const w = await PredictionWorld.create();
  try {
    const root = new URL(
      `../../../apps/client/public/content/packages/${contentIndex.content_hash}/`,
      import.meta.url,
    );
    const collision = JSON.parse(
      await readFile(new URL("collision.json", root), "utf8"),
    ) as CollisionPackage;
    w.loadCollision(collision, fixture.sim_config);
    const motor: VehicleMotor = {
      ...initial,
      position: [-152, 0, 176],
      velocity: [0, 0, 0],
      speed: 0,
      yaw: 0,
      steering: 0,
    };
    w.reset({
      version: 1,
      tick: 0,
      control_epoch: 2,
      controlled_entity_id: 9,
      motor: actorMotor(motor),
      vehicle: { motor, template: fixture.template as VehicleTemplate },
    });
    for (let tick = 1; tick <= 300; tick++) {
      const frame = w.step({
        tick,
        move_x: 0,
        move_z: tick <= 45 ? 32767 : 0,
        buttons: tick >= 90 ? 4 : 0,
        look_yaw: 0,
        look_pitch: 0,
      });
      expect(frame.motor.position[1]).toBe(0);
    }
    expect(Math.abs(w.currentState().vehicle?.motor.speed ?? NaN)).toBe(0);
  } finally {
    w.free();
  }
});
const carOwner = (tick: number, epoch = 2): OwnerState => ({
  server_tick: tick,
  consumed_through_tick: tick,
  control_epoch: epoch,
  player_id: 7,
  controlled_entity_id: 9,
  state: {
    kind: "vehicle",
    data: { motor: initial, health: 100, driver_id: 7, disabled: false },
  },
  player_state: {
    motor: { ...actorMotor(initial), position: [0, 0.8, -0.25] },
    health: 100,
    mode: "driving",
    vehicle_id: 9,
  },
});
it("matches 60 frozen native car checkpoints within one centimetre over 1800 real WASM ticks", async () => {
  const w = await world();
  let tick = 0;
  let maxError = 0;
  let maxSpeedError = 0;
  try {
    w.reset({
      version: 1,
      tick: 0,
      control_epoch: 2,
      controlled_entity_id: 9,
      motor: actorMotor(initial),
      vehicle: {
        motor: initial,
        template: fixture.template as VehicleTemplate,
      },
    });
    for (const segment of fixture.segments)
      for (let i = 0; i < segment.ticks; i++) {
        tick++;
        const frame = w.step({
          tick,
          move_x: segment.move_x,
          move_z: segment.move_z,
          buttons: segment.buttons,
          look_yaw: 0,
          look_pitch: 0,
        });
        const expected = fixture.checkpoints.find((c) => c.tick === tick);
        if (expected) {
          const error = Math.hypot(
            ...expected.motor.position.map(
              (v, j) => v - (frame.motor.position[j] ?? v),
            ),
          );
          maxError = Math.max(maxError, error);
          maxSpeedError = Math.max(
            maxSpeedError,
            Math.abs(
              (frame.vehicle?.motor.speed ?? NaN) - expected.motor.speed,
            ),
          );
        }
      }
    expect(tick).toBe(1800);
    expect(fixture.checkpoints).toHaveLength(60);
    console.info(
      `Vehicle native/WASM trace: ${tick} ticks / 60 checkpoints, maximum position error ${maxError.toFixed(6)} m, speed error ${maxSpeedError}`,
    );
    expect(maxError).toBeLessThan(0.01);
    expect(maxSpeedError).toBeLessThan(0.0005);
  } finally {
    w.free();
  }
});
it("requires reliable control installation and cannot regain a retired car from reordered owner snapshots", async () => {
  const w = await world();
  const clock = new ServerClock();
  const failures: string[] = [];
  const p = new LocalPrediction(w, clock, (r) => failures.push(r), [
    fixture.template as VehicleTemplate,
  ]);
  try {
    const onFoot: OwnerState = {
      ...carOwner(10, 1),
      controlled_entity_id: 7,
      state: {
        kind: "character",
        data: {
          motor: { ...actorMotor(initial), position: [-4, 0, 0] },
          health: 100,
          mode: "on_foot",
          vehicle_id: null,
        },
      },
      player_state: {
        motor: { ...actorMotor(initial), position: [-4, 0, 0] },
        health: 100,
        mode: "on_foot",
        vehicle_id: null,
      },
    };
    clock.observe(10, 0);
    p.install(onFoot, 0);
    p.reconcile(carOwner(12), 10);
    expect(p.epoch).toBe(1);
    expect(w.currentState().vehicle).toBeUndefined();
    p.install(carOwner(11), 20);
    expect(p.epoch).toBe(2);
    expect(w.currentState().vehicle).toBeDefined();
    p.advance(30, () => ({
      move_x: 0,
      move_z: 32767,
      look_yaw: 0,
      look_pitch: 0,
      buttons: 0,
    }));
    p.install(
      {
        ...onFoot,
        server_tick: 14,
        consumed_through_tick: 14,
        control_epoch: 3,
      },
      40,
    );
    p.reconcile(carOwner(100, 2), 45);
    expect(p.epoch).toBe(3);
    expect(w.currentState().vehicle).toBeUndefined();
    expect(p.history.size).toBeLessThan(4);
    expect(failures).toEqual([]);
  } finally {
    w.free();
  }
});
