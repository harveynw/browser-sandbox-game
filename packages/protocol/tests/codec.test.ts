import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { encode } from "@msgpack/msgpack";
import { describe, expect, test } from "vitest";
import {
  decodeClient,
  decodeServer,
  encodeClient,
  encodeServer,
  ProtocolError,
  protocolLimits as limits,
} from "../src/index";
import type { ClientEnvelope, ServerEnvelope, Delivery } from "../src/index";
interface Fixture {
  name: string;
  direction: "client" | "server";
  delivery: Delivery;
  valid: boolean;
  hex: string;
  value: unknown;
}
interface Verification {
  name: string;
  ok: boolean;
  value?: unknown;
}
const root = fileURLToPath(new URL("../../../", import.meta.url));
const manifest = JSON.parse(
  readFileSync(
    new URL("../../../tests/fixtures/protocol/manifest.json", import.meta.url),
    "utf8",
  ),
) as { fixtures: Fixture[] };
const positive = manifest.fixtures.filter((f) => f.valid),
  negative = manifest.fixtures.filter((f) => !f.valid);
function fixture(name: string): Fixture {
  const found = manifest.fixtures.find((f) => f.name === name);
  if (!found) throw new Error(`Missing fixture ${name}`);
  return found;
}
const bytes = (hex: string) => Uint8Array.from(Buffer.from(hex, "hex"));
function decodeFixture(f: Fixture): unknown {
  return f.direction === "client"
    ? decodeClient(bytes(f.hex))
    : decodeServer(bytes(f.hex), f.delivery);
}
function closeValues(actual: unknown, expected: unknown): void {
  if (typeof actual === "number" && typeof expected === "number") {
    if (Number.isSafeInteger(actual) && Number.isSafeInteger(expected))
      expect(actual).toBe(expected);
    else
      expect(Math.abs(actual - expected)).toBeLessThanOrEqual(
        Math.max(1, Math.abs(expected)) * 1e-6,
      );
  } else if (Array.isArray(actual) && Array.isArray(expected)) {
    expect(actual).toHaveLength(expected.length);
    actual.forEach((v: unknown, i: number) => closeValues(v, expected[i]));
  } else if (
    actual &&
    expected &&
    typeof actual === "object" &&
    typeof expected === "object"
  ) {
    const a = actual as Record<string, unknown>,
      e = expected as Record<string, unknown>;
    expect(Object.keys(a).sort()).toEqual(Object.keys(e).sort());
    for (const k of Object.keys(e)) closeValues(a[k], e[k]);
  } else expect(actual).toEqual(expected);
}
describe("small Rust reference corpus", () => {
  test.each(positive)("$name interoperates", (f) =>
    closeValues(decodeFixture(f), f.value),
  );
  test.each(negative)("$name fails decoding", (f) =>
    expect(() => decodeFixture(f)).toThrow(ProtocolError),
  );
});
function rustVerify(all: Fixture[]): Verification[] {
  return JSON.parse(
    execFileSync(
      "cargo",
      [
        "run",
        "--quiet",
        "-p",
        "xtask",
        "--locked",
        "--offline",
        "--",
        "verify-wire",
      ],
      {
        cwd: root,
        input: JSON.stringify(all),
        encoding: "utf8",
        timeout: 120000,
        maxBuffer: 1024 * 1024,
      },
    ),
  ) as Verification[];
}
test("Rust decodes TypeScript's ordinary MessagePack encoding in both directions", () => {
  const encoded = positive.map((f) => ({
    ...f,
    hex: Buffer.from(
      f.direction === "client"
        ? encodeClient(f.value as ClientEnvelope)
        : encodeServer(f.value as ServerEnvelope, f.delivery),
    ).toString("hex"),
  }));
  const all = [...encoded, ...negative],
    results = rustVerify(all);
  expect(results).toHaveLength(all.length);
  results.forEach((r, i) => {
    const f = all[i];
    if (!f) throw new Error("missing fixture");
    expect(r.name).toBe(f.name);
    expect(r.ok, f.name).toBe(f.valid);
    if (f.valid) closeValues(r.value, f.value);
  });
}, 120_000);
test("the authoritative Rust decoder rejects invalid client values sent by JavaScript", () => {
  const base = fixture("input_batch");
  const invalid: Fixture[] = [];
  for (const bad of [NaN, Infinity, 20]) {
    const value = structuredClone(base.value) as ClientEnvelope;
    if (value.message.kind !== "input_batch") throw new Error("fixture");
    const first = value.message.data.frames[0];
    if (!first) throw new Error("frame");
    first.look_pitch = bad;
    invalid.push({
      ...base,
      name: `bad_pitch_${bad}`,
      valid: false,
      hex: Buffer.from(encodeClient(value)).toString("hex"),
    });
  }
  const value = structuredClone(base.value) as ClientEnvelope;
  if (value.message.kind !== "input_batch") throw new Error("fixture");
  value.message.data.frames = [];
  invalid.push({
    ...base,
    name: "empty_input_batch",
    valid: false,
    hex: Buffer.from(encodeClient(value)).toString("hex"),
  });
  expect(rustVerify(invalid).every((r) => !r.ok)).toBe(true);
}, 120_000);
test("maximum realtime messages fit the 1100-byte packet budget", () => {
  for (const name of ["max_input_batch", "max_owner_state"]) {
    const f = fixture(name);
    const packed =
      f.direction === "client"
        ? encodeClient(f.value as ClientEnvelope)
        : encodeServer(f.value as ServerEnvelope, f.delivery);
    expect(packed.length).toBeLessThanOrEqual(limits.unreliable_bytes);
  }
});
test("browser decoding trusts server relationships and leaves them to the owning systems", () => {
  const value = structuredClone(
    fixture("owner_driving").value,
  ) as ServerEnvelope;
  if (
    value.message.kind !== "owner_state" ||
    value.message.data.state.kind !== "vehicle"
  )
    throw new Error("fixture");
  value.message.data.state.data.driver_id = 3;
  expect(decodeServer(encodeServer(value))).toEqual(value);
});
test("frame limits and the library's declared collection/string limits remain", () => {
  expect(() => decodeServer(new Uint8Array(limits.reliable_bytes + 1))).toThrow(
    ProtocolError,
  );
  expect(() =>
    decodeServer(new Uint8Array(limits.unreliable_bytes + 1), "unreliable"),
  ).toThrow(ProtocolError);
  expect(() =>
    decodeClient(Uint8Array.from([0xdd, 255, 255, 255, 255])),
  ).toThrow(ProtocolError);
  expect(() =>
    decodeClient(
      encode({ v: 3, data: "x".repeat(limits.max_string_bytes + 1) }),
    ),
  ).toThrow(ProtocolError);
  const f = fixture("input_batch");
  const value = f.value as ClientEnvelope;
  expect(() =>
    encodeClient({
      ...value,
      message: {
        kind: "input_batch",
        data: {
          control_epoch: 1,
          frames: Array.from({ length: 100 }, () =>
            value.message.kind === "input_batch"
              ? value.message.data.frames[0]
              : undefined,
          ),
        },
      },
    } as ClientEnvelope),
  ).toThrow(ProtocolError);
});
