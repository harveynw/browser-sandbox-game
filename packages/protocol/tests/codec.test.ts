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
  validateClient,
  validateClientContext,
  validateServer,
  ProtocolError,
} from "../src/index";
import type { ClientContext, Delivery } from "../src/index";
import { scan } from "../src/scan";
import limits from "../src/generated/limits.json";

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
const positive = manifest.fixtures.filter((f) => f.valid);
const negative = manifest.fixtures.filter((f) => !f.valid);
function fixture(name: string): Fixture {
  const result = manifest.fixtures.find((f) => f.name === name);
  if (!result) throw new Error("Missing fixture " + name);
  return result;
}
function bytes(hex: string): Uint8Array {
  return Uint8Array.from(Buffer.from(hex, "hex"));
}
function decodeFixture(f: Fixture): unknown {
  return f.direction === "client"
    ? decodeClient(bytes(f.hex))
    : decodeServer(bytes(f.hex), f.delivery);
}
function closeValues(actual: unknown, expected: unknown): void {
  if (typeof actual === "number" && typeof expected === "number") {
    if (Number.isSafeInteger(actual) && Number.isSafeInteger(expected)) {
      expect(actual).toBe(expected);
      return;
    }
    expect(Math.abs(actual - expected)).toBeLessThanOrEqual(
      Math.max(1, Math.abs(expected)) * 1e-6,
    );
  } else if (Array.isArray(actual) && Array.isArray(expected)) {
    expect(actual.length).toBe(expected.length);
    actual.forEach((v: unknown, i: number) => closeValues(v, expected[i]));
  } else if (
    actual !== null &&
    expected !== null &&
    typeof actual === "object" &&
    typeof expected === "object"
  ) {
    const a = actual as Record<string, unknown>;
    const e = expected as Record<string, unknown>;
    expect(Object.keys(a).sort()).toEqual(Object.keys(e).sort());
    for (const key of Object.keys(e)) closeValues(a[key], e[key]);
  } else expect(actual).toEqual(expected);
}

describe("Rust encoded reference fixtures", () => {
  test.each(positive)("$name decodes with its real wire shape", (f) =>
    closeValues(decodeFixture(f), f.value),
  );
  test.each(negative)("$name is rejected", (f) =>
    expect(() => decodeFixture(f)).toThrow(ProtocolError),
  );
});

test("TS encodes every variant; Rust decodes and validates them plus the negative corpus", () => {
  const encoded = positive.map((f) => {
    const packed =
      f.direction === "client"
        ? encodeClient(validateClient(f.value))
        : encodeServer(validateServer(f.value, f.delivery), f.delivery);
    return { ...f, hex: Buffer.from(packed).toString("hex") };
  });
  const floatIntegers = {
    ...fixture("ping"),
    name: "ts_float_encoded_integers",
    hex: Buffer.from(
      encode(fixture("ping").value, { forceIntegerToFloat: true }),
    ).toString("hex"),
  };
  closeValues(decodeClient(bytes(floatIntegers.hex)), floatIntegers.value);
  const all = [...encoded, floatIntegers, ...negative];
  const output = execFileSync(
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
      maxBuffer: 8 * 1024 * 1024,
    },
  );
  const results = JSON.parse(output) as Verification[];
  expect(results).toHaveLength(all.length);
  results.forEach((result, i) => {
    const input = all[i];
    if (!input) throw new Error("Missing case");
    expect(result.name).toBe(input.name);
    expect(result.ok, input.name).toBe(input.valid);
    if (input.valid) closeValues(result.value, input.value);
  });
});

test("maximum-size realtime envelopes fit even when JS emits float64", () => {
  for (const name of ["max_input_batch", "max_owner_state"]) {
    const f = fixture(name);
    const packed =
      f.direction === "client"
        ? encodeClient(validateClient(f.value))
        : encodeServer(validateServer(f.value, f.delivery), f.delivery);
    expect(packed.length).toBeLessThanOrEqual(limits.unreliable_bytes);
  }
});

test("sender cannot silently omit/coerce JS-only values or execute getters", () => {
  const base = fixture("action_exit_vehicle").value;
  const invalid: unknown[] = [
    undefined,
    NaN,
    Infinity,
    1n,
    new Date(),
    new Map(),
    new Set(),
    new Uint8Array(),
    () => 0,
    Symbol("x"),
    "\ud800",
  ];
  for (const item of invalid) {
    const value = structuredClone(base) as Record<string, unknown>;
    value["unexpected"] = item;
    expect(() => validateClient(value)).toThrow(ProtocolError);
  }
  const cyclic: Record<string, unknown> = {};
  cyclic["self"] = cyclic;
  expect(() => validateClient(cyclic)).toThrow(ProtocolError);
  expect(() =>
    validateClient({
      get message() {
        throw new Error("getter executed");
      },
    }),
  ).toThrow(ProtocolError);
  expect(() => validateClient(Array<unknown>(1))).toThrow(ProtocolError);
});

test("unknown fields, missing required nulls, wrong empty payloads and positional structs fail", () => {
  const value = structuredClone(fixture("action_exit_vehicle").value) as Record<
    string,
    unknown
  >;
  delete value["session_id"];
  expect(() => decodeClient(encode(value))).toThrow(ProtocolError);
  expect(() =>
    decodeClient(
      encode([1, "world-0001", "session-0001", { kind: "leave", data: {} }]),
    ),
  ).toThrow(ProtocolError);
  expect(() =>
    validateClient({ ...(fixture("leave").value as object), extra: 1 }),
  ).toThrow(ProtocolError);
});

test("phase, scope, epoch, baseline and tick window admission", () => {
  const input = validateClient(fixture("input_batch").value);
  const context: ClientContext = {
    world_id: "world-0001",
    session_id: "session-0001",
    phase: "playing",
    control_epoch: 1,
    next_tick: 31,
    baseline_id: 1,
  };
  expect(() => validateClientContext(input, context)).not.toThrow();
  for (const change of [
    { phase: "loading" as const },
    { session_id: "stale" },
    { world_id: "old" },
    { control_epoch: 2 },
    { next_tick: 32 },
    { next_tick: 1 },
  ]) {
    expect(() =>
      validateClientContext(input, { ...context, ...change }),
    ).toThrow(ProtocolError);
  }
  const ready = validateClient(fixture("client_ready").value);
  expect(() =>
    validateClientContext(ready, { ...context, phase: "loading" }),
  ).not.toThrow();
  expect(() =>
    validateClientContext(ready, {
      ...context,
      phase: "loading",
      baseline_id: 2,
    }),
  ).toThrow(ProtocolError);
});

test("unreliable ownership changes are rejected", () => {
  expect(() =>
    decodeServer(bytes(fixture("control_changed").hex), "unreliable"),
  ).toThrow(ProtocolError);
});

test("preflight limits declared lengths, total nodes and UTF-8 byte lengths", () => {
  const nodes = [0xdc, 0, 128];
  for (let i = 0; i < 128; i++)
    nodes.push(0x9f, ...Array<number>(15).fill(0xc0));
  expect(() => scan(Uint8Array.from(nodes), limits.reliable_bytes)).toThrow(
    ProtocolError,
  );
  const text = [0xda, 2, 1, ...Array<number>(513).fill(97)];
  expect(() => scan(Uint8Array.from(text), limits.reliable_bytes)).toThrow(
    ProtocolError,
  );
  expect(() =>
    scan(
      Uint8Array.from([0xdc, 0, 128, ...Array<number>(128).fill(0xc0)]),
      limits.reliable_bytes,
    ),
  ).not.toThrow();
  const input = bytes(fixture("input_batch").hex);
  for (let i = 0; i < input.length; i++)
    expect(() => decodeClient(input.subarray(0, i))).toThrow(ProtocolError);
});
