import type { CharacterMotor } from "@port-meridian/protocol";
import {
  type CollisionPackage,
  canonicalJson,
  sha256,
} from "@port-meridian/content";
import fixture from "../../../../tests/fixtures/simulation/character-trace.json";
import { PredictionWorld, type PredictedFrame } from "./bridge";
export interface TraceResult {
  ticks: number;
  checkpoints: number;
  maxPositionError: number;
  maxVelocityError: number;
  millisecondsPerStep: number;
  finalFrame: PredictedFrame;
}
function distance(a: number[], b: number[]): number {
  return Math.hypot(...a.map((v, i) => v - (b[i] ?? NaN)));
}
/** Replay through the public byte bridge, not a WASM-only alternate trace export. */
export async function runCharacterTrace(
  signal?: AbortSignal,
): Promise<TraceResult> {
  const configHash = await sha256(
    new TextEncoder().encode(canonicalJson(fixture.sim_config)),
  );
  const collisionHash = await sha256(
    new TextEncoder().encode(canonicalJson(fixture.collision)),
  );
  if (
    configHash !== fixture.sim_config_hash ||
    collisionHash !== fixture.collision_hash
  )
    throw new Error("Trace fixture hash mismatch");
  const world = await PredictionWorld.create();
  try {
    signal?.throwIfAborted();
    world.loadCollision(
      fixture.collision as CollisionPackage,
      fixture.sim_config,
    );
    world.setVehicleProxies({ version: 1, proxies: [] });
    world.reset({
      version: 1,
      tick: 0,
      control_epoch: 1,
      controlled_entity_id: 1,
      motor: fixture.initial_motor as CharacterMotor,
    });
    let tick = 0;
    let checkpoints = 0;
    let maxPositionError = 0;
    let maxVelocityError = 0;
    let elapsed = 0;
    for (const segment of fixture.segments) {
      for (let i = 0; i < segment.ticks; i++) {
        signal?.throwIfAborted();
        const start = performance.now();
        const actual = world.step({
          tick: ++tick,
          move_x: segment.move_x,
          move_z: segment.move_z,
          look_yaw: segment.look_yaw,
          look_pitch: 0,
          buttons: segment.buttons,
        });
        elapsed += performance.now() - start;
        const expected = fixture.checkpoints.find((c) => c.tick === tick);
        if (expected) {
          checkpoints++;
          const positionError = distance(
            actual.motor.position,
            expected.motor.position,
          );
          const velocityError = distance(
            actual.motor.velocity,
            expected.motor.velocity,
          );
          maxPositionError = Math.max(maxPositionError, positionError);
          maxVelocityError = Math.max(maxVelocityError, velocityError);
          const yawDelta = actual.motor.yaw - expected.motor.yaw;
          if (
            positionError > 0.01 ||
            velocityError > 0.01 ||
            Math.abs(Math.atan2(Math.sin(yawDelta), Math.cos(yawDelta))) >
              0.001 ||
            actual.motor.grounded !== expected.motor.grounded ||
            actual.motor.jump_cooldown_ticks !==
              expected.motor.jump_cooldown_ticks
          )
            throw new Error(
              `Trace mismatch at tick ${tick}: ${positionError.toFixed(6)}m`,
            );
        }
        // Yield between batches so cancellation/navigation can release the WASM world.
        if (tick % 300 === 0)
          await new Promise<void>((resolve) => setTimeout(resolve, 0));
      }
    }
    if (tick !== 1800 || checkpoints !== fixture.checkpoints.length)
      throw new Error("Incomplete trace");
    // Exercise the static camera ray path against the fixture wall.
    const ray = world.raycastStatic({
      version: 1,
      origin: [0, 1, 0],
      direction: [0, 0, -1],
      max_distance: 20,
    });
    if (!ray.hit || ray.hit.distance < 7 || ray.hit.distance > 8)
      throw new Error("Static camera ray check failed");
    return {
      ticks: tick,
      checkpoints,
      maxPositionError,
      maxVelocityError,
      millisecondsPerStep: elapsed / tick,
      finalFrame: world.currentState(),
    };
  } finally {
    world.free();
  }
}
