import { protocolLimits as limits } from "./limits";
import { ProtocolError } from "./error";

const utf8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** Check structure and declared sizes before a MessagePack decoder allocates. */
export function scan(bytes: Uint8Array, byteLimit: number): void {
  if (bytes.byteLength === 0 || bytes.byteLength > byteLimit) {
    throw new ProtocolError("empty or oversized frame");
  }
  const scanner = new Scanner(bytes);
  scanner.value(0);
  if (!scanner.finished()) throw new ProtocolError("trailing MessagePack data");
}

class Scanner {
  private offset = 0;
  private nodes = 0;
  private readonly view: DataView;

  constructor(private readonly bytes: Uint8Array) {
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  finished(): boolean {
    return this.offset === this.bytes.length;
  }

  private take(count: number): number {
    const offset = this.offset;
    if (offset + count > this.bytes.length)
      throw new ProtocolError("truncated frame");
    this.offset += count;
    return offset;
  }

  private unsigned(count: number): number {
    const offset = this.take(count);
    if (count === 1) return this.view.getUint8(offset);
    if (count === 2) return this.view.getUint16(offset);
    return this.view.getUint32(offset);
  }

  private bump(depth: number): void {
    this.nodes++;
    if (depth > limits.max_depth || this.nodes > limits.max_nodes) {
      throw new ProtocolError("MessagePack complexity limit");
    }
  }

  private string(marker: number): string {
    let count: number;
    if (marker >= 0xa0 && marker <= 0xbf) count = marker & 0x1f;
    else if (marker === 0xd9) count = this.unsigned(1);
    else if (marker === 0xda) count = this.unsigned(2);
    else if (marker === 0xdb) count = this.unsigned(4);
    else throw new ProtocolError("map keys must be strings");
    if (count > limits.max_string_bytes)
      throw new ProtocolError("string length limit");
    const offset = this.take(count);
    try {
      return utf8.decode(this.bytes.subarray(offset, offset + count));
    } catch {
      throw new ProtocolError("invalid UTF-8");
    }
  }

  private array(count: number, depth: number): void {
    if (count > limits.max_array_items)
      throw new ProtocolError("array length limit");
    for (let i = 0; i < count; i++) this.value(depth + 1);
  }

  private map(count: number, depth: number): void {
    if (count > limits.max_map_fields)
      throw new ProtocolError("map length limit");
    const keys = new Set<string>();
    for (let i = 0; i < count; i++) {
      this.bump(depth + 1);
      const key = this.string(this.unsigned(1));
      if (
        keys.has(key) ||
        ["__proto__", "constructor", "prototype"].includes(key)
      ) {
        throw new ProtocolError("duplicate or reserved map key");
      }
      keys.add(key);
      this.value(depth + 1);
    }
  }

  value(depth: number): void {
    this.bump(depth);
    const marker = this.unsigned(1);
    if (marker <= 0x7f || marker >= 0xe0 || [0xc0, 0xc2, 0xc3].includes(marker))
      return;
    if (marker >= 0x80 && marker <= 0x8f) return this.map(marker & 0xf, depth);
    if (marker >= 0x90 && marker <= 0x9f)
      return this.array(marker & 0xf, depth);
    if (
      (marker >= 0xa0 && marker <= 0xbf) ||
      (marker >= 0xd9 && marker <= 0xdb)
    ) {
      this.string(marker);
      return;
    }
    if (marker >= 0xcc && marker <= 0xce) {
      this.unsigned(2 ** (marker - 0xcc));
      return;
    }
    if (marker === 0xcf || marker === 0xd3) {
      const offset = this.take(8);
      const value =
        marker === 0xcf
          ? this.view.getBigUint64(offset)
          : this.view.getBigInt64(offset);
      const max = BigInt(limits.max_safe_integer);
      if (value > max || value < -max)
        throw new ProtocolError("unsafe integer");
      return;
    }
    if (marker >= 0xd0 && marker <= 0xd2) {
      this.take(2 ** (marker - 0xd0));
      return;
    }
    if (marker === 0xca || marker === 0xcb) {
      const offset = this.take(marker === 0xca ? 4 : 8);
      const value =
        marker === 0xca
          ? this.view.getFloat32(offset)
          : this.view.getFloat64(offset);
      if (!Number.isFinite(value) || Math.abs(value) > limits.max_float) {
        throw new ProtocolError("non-finite or unrepresentable float");
      }
      return;
    }
    if (marker === 0xdc || marker === 0xdd)
      return this.array(this.unsigned(marker === 0xdc ? 2 : 4), depth);
    if (marker === 0xde || marker === 0xdf)
      return this.map(this.unsigned(marker === 0xde ? 2 : 4), depth);
    throw new ProtocolError(
      "unsupported MessagePack marker (binary/extensions forbidden)",
    );
  }
}

/** Reject non-wire JS values before the encoder can coerce or omit them. */
export function assertPlainData(value: unknown): void {
  let nodes = 0;
  const ancestors = new Set<object>();
  const encoder = new TextEncoder();
  function visit(item: unknown, depth: number): void {
    nodes++;
    if (nodes > limits.max_nodes || depth > limits.max_depth)
      throw new ProtocolError("value complexity limit");
    if (item === null || typeof item === "boolean") return;
    if (typeof item === "number") {
      if (!Number.isFinite(item) || Math.abs(item) > limits.max_float)
        throw new ProtocolError("invalid number");
      return;
    }
    if (typeof item === "string") {
      if (
        item.length > limits.max_string_bytes ||
        encoder.encode(item).length > limits.max_string_bytes ||
        [...item].some((c) => {
          const n = c.codePointAt(0);
          return n !== undefined && n >= 0xd800 && n <= 0xdfff;
        })
      ) {
        throw new ProtocolError("invalid string");
      }
      return;
    }
    if (typeof item !== "object")
      throw new ProtocolError("unsupported JS value");
    if (ancestors.has(item)) throw new ProtocolError("cyclic value");
    const array = Array.isArray(item);
    const prototype: unknown = Object.getPrototypeOf(item);
    if (
      array
        ? prototype !== Array.prototype
        : prototype !== Object.prototype && prototype !== null
    ) {
      throw new ProtocolError("wire values must be plain objects or arrays");
    }
    const keys = Reflect.ownKeys(item);
    if (array) {
      if (
        item.length > limits.max_array_items ||
        keys.length !== item.length + 1
      )
        throw new ProtocolError("invalid array");
    } else if (keys.length > limits.max_map_fields)
      throw new ProtocolError("map length limit");
    ancestors.add(item);
    for (const key of keys) {
      if (array && key === "length") continue;
      if (
        typeof key !== "string" ||
        ["__proto__", "constructor", "prototype"].includes(key)
      )
        throw new ProtocolError("invalid map key");
      if (array && !/^(0|[1-9][0-9]*)$/.test(key))
        throw new ProtocolError("invalid array key");
      const descriptor = Object.getOwnPropertyDescriptor(item, key);
      if (!descriptor || !descriptor.enumerable || !("value" in descriptor))
        throw new ProtocolError("accessors/hidden fields forbidden");
      if (!array) {
        visit(key, depth + 1);
      }
      visit(descriptor.value as unknown, depth + 1);
    }
    ancestors.delete(item);
  }
  visit(value, 0);
}
