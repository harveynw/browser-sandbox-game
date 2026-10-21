import { encode, decode } from "@msgpack/msgpack";
import Ajv2020 from "ajv/dist/2020.js";
import type { ValidateFunction } from "ajv";
import type { CollisionPackage, SimConfig } from "@port-meridian/content";
import type { InputFrame } from "@port-meridian/protocol";
import init, {
  WasmPredictionWorld,
} from "../../../../packages/sim-wasm/sim_wasm";
import frameSchema from "./generated/frame.schema.json";
import stepSchema from "./generated/step.schema.json";
import proxiesSchema from "./generated/proxies.schema.json";
import rayQuerySchema from "./generated/ray-query.schema.json";
import sphereQuerySchema from "./generated/sphere-query.schema.json";
import rayResultSchema from "./generated/ray-result.schema.json";
import type {
  PredictedFrame,
  StepRequest,
  ProxySet,
  StaticRayQuery,
  StaticSphereQuery,
  StaticRayResult,
} from "./generated/types";
export type * from "./generated/types";

export class BridgeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "BridgeError";
  }
}
const ajv = new Ajv2020({
  strict: true,
  formats: {
    uint32: {
      type: "number",
      validate: (n: number) => Number.isInteger(n) && n >= 0 && n <= 0xffffffff,
    },
    uint16: {
      type: "number",
      validate: (n: number) => Number.isInteger(n) && n >= 0 && n <= 0xffff,
    },
    float: { type: "number", validate: Number.isFinite },
    double: { type: "number", validate: Number.isFinite },
  },
});
const validators = {
  frame: ajv.compile<PredictedFrame>(frameSchema),
  step: ajv.compile<StepRequest>(stepSchema),
  proxies: ajv.compile<ProxySet>(proxiesSchema),
  rayQuery: ajv.compile<StaticRayQuery>(rayQuerySchema),
  sphereQuery: ajv.compile<StaticSphereQuery>(sphereQuerySchema),
  rayResult: ajv.compile<StaticRayResult>(rayResultSchema),
};
function checked<T extends { version: number }>(
  value: unknown,
  validate: ValidateFunction<T>,
): T {
  if (!validate(value) || value.version !== 1)
    throw new BridgeError("Invalid bridge DTO/version");
  return value;
}
function pack(value: unknown, limit = 16_384): Uint8Array {
  const bytes = encode(value, { maxDepth: 32 });
  if (bytes.length > limit)
    throw new BridgeError("Bridge buffer exceeds byte limit");
  return bytes;
}
function unpack<T extends { version: number }>(
  bytes: Uint8Array,
  validate: ValidateFunction<T>,
): T {
  if (bytes.length > 16_384)
    throw new BridgeError("Bridge output exceeds byte limit");
  return checked(
    decode(bytes, {
      maxStrLength: 512,
      maxArrayLength: 128,
      maxMapLength: 32,
      maxBinLength: 0,
      maxExtLength: 0,
    }),
    validate,
  );
}
let initialized: Promise<unknown> | undefined;
/** Async initialization is shared; failures allow an explicit retry. */
export async function initializeSimulation(): Promise<void> {
  initialized ??= init().catch((error: unknown) => {
    initialized = undefined;
    throw new BridgeError(`WASM load failed: ${String(error)}`);
  });
  await initialized;
}
/** Own one static BVH and one controlled character or car. Free on teardown; never rebuild on step/reset. */
export class PredictionWorld {
  private raw: WasmPredictionWorld | undefined;
  private identity:
    { control_epoch: number; controlled_entity_id: number } | undefined;
  private constructor() {
    this.raw = new WasmPredictionWorld();
  }
  static async create(): Promise<PredictionWorld> {
    await initializeSimulation();
    return new PredictionWorld();
  }
  private call<T>(action: (world: WasmPredictionWorld) => T): T {
    if (!this.raw) throw new BridgeError("Prediction world is freed");
    try {
      return action(this.raw);
    } catch (error) {
      throw error instanceof BridgeError
        ? error
        : new BridgeError(String(error));
    }
  }
  loadCollision(collision: CollisionPackage, config: SimConfig): void {
    this.call((world) =>
      world.load_collision(
        pack(collision, 2 * 1024 * 1024),
        pack(config, 2 * 1024 * 1024),
      ),
    );
    this.identity = undefined;
  }
  reset(frame: PredictedFrame): void {
    this.call((world) => world.reset(pack(checked(frame, validators.frame))));
    this.identity = {
      control_epoch: frame.control_epoch,
      controlled_entity_id: frame.controlled_entity_id,
    };
  }
  step(input: InputFrame): PredictedFrame {
    if (!this.identity) throw new BridgeError("Prediction motor is not reset");
    const request: StepRequest = { version: 1, ...this.identity, input };
    return this.call((world) =>
      unpack(
        world.step(pack(checked(request, validators.step))),
        validators.frame,
      ),
    );
  }
  currentState(): PredictedFrame {
    return this.call((world) =>
      unpack(world.current_state(), validators.frame),
    );
  }
  setVehicleProxies(set: ProxySet): void {
    this.call((world) =>
      world.set_vehicle_proxies(
        pack(checked(set, validators.proxies), 64 * 1024),
      ),
    );
  }
  raycastStatic(query: StaticRayQuery): StaticRayResult {
    return this.call((world) =>
      unpack(
        world.raycast_static(pack(checked(query, validators.rayQuery))),
        validators.rayResult,
      ),
    );
  }
  spherecastStatic(query: StaticSphereQuery): StaticRayResult {
    return this.call((world) =>
      unpack(
        world.spherecast_static(pack(checked(query, validators.sphereQuery))),
        validators.rayResult,
      ),
    );
  }
  free(): void {
    this.raw?.free();
    this.raw = undefined;
    this.identity = undefined;
  }
}
