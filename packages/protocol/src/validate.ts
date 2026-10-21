import { Ajv2020 } from "ajv/dist/2020.js";
import clientSchema from "./generated/client.schema.json";
import serverSchema from "./generated/server.schema.json";
import type {
  CharacterState,
  ClientEnvelope,
  ClientContext,
  Delivery,
  EntityState,
  OwnerState,
  ServerEnvelope,
} from "./generated/types";
import { ProtocolError } from "./error";
import { assertPlainData } from "./scan";

const ajv = new Ajv2020({
  strict: true,
  allErrors: false,
  validateFormats: false,
});
const clientShape = ajv.compile<ClientEnvelope>(clientSchema);
const serverShape = ajv.compile<ServerEnvelope>(serverSchema);

function scope(
  world: string | null,
  session: string | null,
  preAuth: boolean,
  either: boolean,
): void {
  if (world === null || (!either && preAuth !== (session === null)))
    throw new ProtocolError("invalid envelope scope");
}

function textSemantics(value: unknown): void {
  if (value === null || typeof value !== "object") return;
  for (const [key, child] of Object.entries(value)) {
    if (
      key === "display_name" &&
      typeof child === "string" &&
      /^\p{White_Space}|\p{White_Space}$/u.test(child)
    )
      throw new ProtocolError("display name must be trimmed");
    textSemantics(child);
  }
}

export function validateClient(value: unknown): ClientEnvelope {
  assertPlainData(value);
  if (!clientShape(value))
    throw new ProtocolError("client schema validation failed");
  const message = value.message;
  scope(
    value.world_id,
    value.session_id,
    message.kind === "client_hello",
    message.kind === "leave",
  );
  if (
    message.kind === "input_batch" &&
    message.data.frames.some(
      (frame, i, frames) => i > 0 && (frames[i - 1]?.tick ?? 0) >= frame.tick,
    )
  ) {
    throw new ProtocolError("input frames must have unique ascending ticks");
  }
  textSemantics(value);
  return value;
}

function character(value: CharacterState): void {
  if ((value.mode === "driving") !== (value.vehicle_id !== null))
    throw new ProtocolError("character mode/vehicle mismatch");
}

function state(value: EntityState): void {
  if (value.kind === "character") character(value.data);
  else if (value.data.disabled && value.data.driver_id !== null)
    throw new ProtocolError("disabled vehicle has a driver");
}

function sameCharacter(a: CharacterState, b: CharacterState): boolean {
  return (
    a.health === b.health &&
    a.mode === b.mode &&
    a.vehicle_id === b.vehicle_id &&
    a.motor.grounded === b.motor.grounded &&
    a.motor.jump_cooldown_ticks === b.motor.jump_cooldown_ticks &&
    Math.fround(a.motor.yaw) === Math.fround(b.motor.yaw) &&
    a.motor.position.every(
      (v, i) => Math.fround(v) === Math.fround(b.motor.position[i] ?? NaN),
    ) &&
    a.motor.velocity.every(
      (v, i) => Math.fround(v) === Math.fround(b.motor.velocity[i] ?? NaN),
    )
  );
}

function owner(value: OwnerState): void {
  if (value.server_tick !== value.consumed_through_tick)
    throw new ProtocolError("owner input checkpoint mismatch");
  character(value.player_state);
  state(value.state);
  const valid =
    value.state.kind === "character"
      ? value.controlled_entity_id === value.player_id &&
        value.state.data.mode !== "driving" &&
        sameCharacter(value.state.data, value.player_state)
      : value.controlled_entity_id !== value.player_id &&
        value.player_state.mode === "driving" &&
        value.player_state.vehicle_id === value.controlled_entity_id &&
        value.state.data.driver_id === value.player_id;
  if (!valid) throw new ProtocolError("owner control/occupancy mismatch");
}

export function validateServer(
  value: unknown,
  delivery: Delivery,
): ServerEnvelope {
  assertPlainData(value);
  if (!serverShape(value))
    throw new ProtocolError("server schema validation failed");
  const message = value.message;
  scope(
    value.world_id,
    value.session_id,
    message.kind === "server_hello" || message.kind === "reject",
    message.kind === "disconnect",
  );
  if (
    delivery === "unreliable" &&
    message.kind !== "snapshot" &&
    message.kind !== "owner_state" &&
    message.kind !== "pong" &&
    !(message.kind === "world_event" && message.data.kind === "impact")
  )
    throw new ProtocolError("message requires reliable delivery");
  switch (message.kind) {
    case "entity_show": {
      state(message.data.state);
      const kind = message.data.descriptor.entity_kind;
      if (
        (kind === "vehicle" && message.data.state.kind !== "vehicle") ||
        ((kind === "player" || kind === "pedestrian") &&
          message.data.state.kind !== "character")
      )
        throw new ProtocolError("descriptor/state kind mismatch");
      break;
    }
    case "snapshot": {
      const ids = new Set<number>();
      for (const record of message.data.records) {
        if (ids.has(record.entity_id))
          throw new ProtocolError("duplicate snapshot entity");
        ids.add(record.entity_id);
        state(record.state);
      }
      break;
    }
    case "owner_state":
    case "control_changed":
      owner(message.data);
      break;
    case "start_play":
      owner(message.data.owner);
      break;
    case "action_result":
      if (message.data.ok !== (message.data.code === "ok"))
        throw new ProtocolError("action result code/ok mismatch");
      break;
    case "player_status":
      if ((message.data.heat === 0) !== (message.data.pursuit === "clear"))
        throw new ProtocolError("heat/pursuit mismatch");
      break;
  }
  textSemantics(value);
  return value;
}

/** Stateless admission only; authoritative gameplay checks still belong to T07+. */
export function validateClientContext(
  value: ClientEnvelope,
  context: ClientContext,
): void {
  validateClient(value);
  if (
    value.world_id !== context.world_id ||
    value.session_id !== context.session_id ||
    context.phase === "closing"
  )
    throw new ProtocolError("stale world/session or closed phase");
  const message = value.message;
  let allowed: boolean;
  switch (message.kind) {
    case "client_hello":
      allowed = context.phase === "await_hello";
      break;
    case "client_ready":
      allowed =
        context.phase === "loading" &&
        message.data.baseline_id === context.baseline_id;
      break;
    case "input_batch":
      allowed =
        context.phase === "playing" &&
        message.data.control_epoch === context.control_epoch &&
        message.data.frames.every(
          (f) =>
            f.tick >= context.next_tick &&
            f.tick <= Math.min(4294967295, context.next_tick + 8),
        );
      break;
    case "action":
      allowed =
        context.phase === "playing" &&
        message.data.control_epoch === context.control_epoch;
      break;
    case "resync_request":
      allowed = context.phase === "playing";
      break;
    case "ping":
      allowed = context.phase === "loading" || context.phase === "playing";
      break;
    case "leave":
      allowed = true;
  }
  if (!allowed) throw new ProtocolError("message rejected by session context");
}
