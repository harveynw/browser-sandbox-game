import { decode, encode } from "@msgpack/msgpack";
import { protocolLimits as limits } from "./limits";
import type {
  ClientEnvelope,
  Delivery,
  ServerEnvelope,
} from "./generated/types";
import { ProtocolError } from "./error";

function decoded(bytes: Uint8Array, limit: number): unknown {
  checkSize(bytes, limit);
  try {
    const value: unknown = decode(bytes, {
      maxStrLength: limits.max_string_bytes,
      maxArrayLength: limits.max_array_items,
      maxMapLength: limits.max_map_fields,
      maxBinLength: 0,
      maxExtLength: 0,
    });
    if (!value || typeof value !== "object" || !("v" in value) || value.v !== 4)
      throw new ProtocolError("unsupported protocol version");
    return value;
  } catch (error) {
    if (error instanceof ProtocolError) throw error;
    throw new ProtocolError("invalid MessagePack message");
  }
}
function checkSize(bytes: Uint8Array, limit: number): void {
  if (!bytes.length || bytes.length > limit)
    throw new ProtocolError("empty or oversized frame");
}
function encoded(
  value: ClientEnvelope | ServerEnvelope,
  limit: number,
): Uint8Array {
  const bytes = encode(value, { maxDepth: limits.max_depth });
  checkSize(bytes, limit);
  return bytes;
}
/** Typed convenience for tests/tools; the authoritative Rust server validates client inputs. */
export function decodeClient(bytes: Uint8Array): ClientEnvelope {
  return decoded(bytes, limits.client_bytes) as ClientEnvelope;
}
/** Server payloads are trusted after MessagePack decoding, frame limits and version checking. */
export function decodeServer(
  bytes: Uint8Array,
  delivery: Delivery = "reliable",
): ServerEnvelope {
  return decoded(bytes, serverLimit(delivery)) as ServerEnvelope;
}
export function encodeClient(value: ClientEnvelope): Uint8Array {
  return encoded(
    value,
    value.message.kind === "input_batch"
      ? limits.unreliable_bytes
      : limits.client_bytes,
  );
}
export function encodeServer(
  value: ServerEnvelope,
  delivery: Delivery = "reliable",
): Uint8Array {
  return encoded(value, serverLimit(delivery));
}
function serverLimit(delivery: Delivery): number {
  return delivery === "reliable"
    ? limits.reliable_bytes
    : limits.unreliable_bytes;
}
