import { decode, encode } from "@msgpack/msgpack";
import { protocolLimits as limits } from "./limits";
import type {
  ClientEnvelope,
  Delivery,
  ServerEnvelope,
} from "./generated/types";
import { ProtocolError } from "./error";
import { scan } from "./scan";
import { validateClient, validateServer } from "./validate";

function decoded(bytes: Uint8Array, limit: number): unknown {
  scan(bytes, limit);
  try {
    return decode(bytes, {
      maxStrLength: limits.max_string_bytes,
      maxArrayLength: limits.max_array_items,
      maxMapLength: limits.max_map_fields,
      maxBinLength: 0,
      maxExtLength: 0,
      useBigInt64: false,
    });
  } catch {
    throw new ProtocolError("invalid MessagePack value");
  }
}

export function decodeClient(bytes: Uint8Array): ClientEnvelope {
  const value = validateClient(decoded(bytes, limits.client_bytes));
  if (
    value.message.kind === "input_batch" &&
    bytes.length > limits.unreliable_bytes
  )
    throw new ProtocolError("input batch byte limit");
  return value;
}

export function decodeServer(
  bytes: Uint8Array,
  delivery: Delivery = "reliable",
): ServerEnvelope {
  return validateServer(
    decoded(
      bytes,
      delivery === "reliable" ? limits.reliable_bytes : limits.unreliable_bytes,
    ),
    delivery,
  );
}

export function encodeClient(value: ClientEnvelope): Uint8Array {
  validateClient(value);
  const bytes = encode(value, { maxDepth: limits.max_depth + 1 });
  decodeClient(bytes);
  return bytes;
}

export function encodeServer(
  value: ServerEnvelope,
  delivery: Delivery = "reliable",
): Uint8Array {
  validateServer(value, delivery);
  const bytes = encode(value, { maxDepth: limits.max_depth + 1 });
  decodeServer(bytes, delivery);
  return bytes;
}
