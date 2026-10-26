import { readFile } from "node:fs/promises";
import { beforeAll, expect, it } from "vitest";
import { encode } from "@msgpack/msgpack";
import init from "../../sim-wasm/sim_wasm";
import { PredictionWorld } from "../../../apps/client/src/simulation/bridge";
import { LocalPrediction } from "../../../apps/client/src/simulation/prediction";
import { ServerClock } from "../../../apps/client/src/net/clock";
import { decodeServer, encodeClient } from "@port-meridian/protocol";
import type {
  CharacterMotor,
  InputFrame,
  OwnerState,
} from "@port-meridian/protocol";
import type { CollisionPackage } from "@port-meridian/content";
import fixture from "../../../tests/fixtures/simulation/character-trace.json";
const initial = fixture.initial_motor as CharacterMotor;
const owner = (tick: number, motor = initial, epoch = 1): OwnerState => ({
  server_tick: tick,
  consumed_through_tick: tick,
  control_epoch: epoch,
  player_id: 1,
  controlled_entity_id: 1,
  state: {
    kind: "character",
    data: { motor, health: 100, mode: "on_foot", vehicle_id: null },
  },
  player_state: { motor, health: 100, mode: "on_foot", vehicle_id: null },
});
const controls = {
  move_x: 0,
  move_z: 32767,
  look_yaw: 0,
  look_pitch: 0,
  buttons: 0,
};
async function world(): Promise<PredictionWorld> {
  const w = await PredictionWorld.create();
  w.loadCollision(fixture.collision as CollisionPackage, fixture.sim_config);
  return w;
}
beforeAll(async () => {
  await init({
    module_or_path: await readFile(
      new URL("../../sim-wasm/sim_wasm_bg.wasm", import.meta.url),
    ),
  });
});
it("replays real WASM inputs, retires consumed ticks, smooths small errors only in presentation and gates future epochs", async () => {
  const w = await world(),
    clock = new ServerClock();
  const failures: string[] = [];
  const p = new LocalPrediction(w, clock, (reason) => failures.push(reason));
  clock.observe(10, 0);
  p.install(owner(10), 0);
  p.advance(0, () => controls);
  expect(p.batch().map((i) => i.tick)).toEqual([11, 12, 13]);
  const physical = structuredClone(w.currentState().motor),
    visible = p.presentation(17);
  const authoritative = structuredClone(initial);
  authoritative.position[0] += 0.1;
  p.reconcile(owner(11, authoritative), 17);
  expect([...p.history.keys()]).toEqual([12, 13]);
  expect(p.replaySteps).toBe(2);
  expect(w.currentState().motor.position[0]).toBeCloseTo(
    physical.position[0] + 0.1,
    5,
  );
  expect(p.presentation(17)?.position[0]).toBeCloseTo(
    visible?.position[0] ?? 0,
    5,
  );
  expect(p.presentation(117)?.position[0]).toBeCloseTo(0.1, 5);
  p.reconcile(owner(10), 120);
  expect(w.currentState().motor.position[0]).toBeCloseTo(0.1, 5);
  p.reconcile(owner(14, initial, 2), 130);
  expect(p.epoch).toBe(1);
  p.install(owner(13, initial, 2), 140);
  expect(p.epoch).toBe(2);
  expect([...p.history.keys()].every((tick) => tick > 14)).toBe(true);
  expect(
    [...p.history.values()].every(
      (h) =>
        h.input.move_x === 0 && h.input.move_z === 0 && h.input.buttons === 0,
    ),
  ).toBe(true);
  expect(p.tick).toBeGreaterThanOrEqual(14);
  const teleport = structuredClone(initial);
  teleport.position[0] = -4;
  p.reconcile(owner(15, teleport, 2), 150);
  expect(p.presentation(150)?.position[0]).toBeCloseTo(-4, 5);
  p.advance(1000, () => controls);
  expect(failures).toEqual(["stale"]);
  w.free();
});
it("bounds the replay ring and never resumes a stopped motor without a baseline", async () => {
  const w = await world(),
    clock = new ServerClock();
  const failures: string[] = [];
  const p = new LocalPrediction(w, clock, (reason) => failures.push(reason));
  clock.observe(0, 0);
  p.install(owner(0), 0);
  for (let i = 0; i < 200 && !p.stopped; i++)
    p.advance((i * 1000) / 30, () => controls);
  expect(p.history.size).toBeLessThanOrEqual(128);
  expect(failures).toEqual(["overflow"]);
  const tick = p.tick;
  p.advance(9000, () => controls);
  expect(p.tick).toBe(tick);
  w.free();
});

// Seeded delivery scheduler: real serialized three-frame input batches, independent owner
// messages, real WASM motors on both ends, 100 ms RTT / 20 ms jitter / 2% RTC loss.
it.each([
  { label: "supported", rtt: 100, jitter: 20, loss: 0.02, p95: 0.25 },
  { label: "stressed", rtt: 200, jitter: 50, loss: 0.05, p95: 1.0 },
])(
  "bounds static-world correction under seeded $label loss/reorder/duplicate delivery",
  async (profile) => {
    const client = await world(),
      authority = await world(),
      clock = new ServerClock();
    const failures: string[] = [];
    const p = new LocalPrediction(client, clock, (reason) =>
      failures.push(reason),
    );
    let authoritative = structuredClone(initial),
      serverTick = 0,
      sequence = 0,
      seed = 0x71c9,
      generated = 0,
      late = 0,
      conflicts = 0,
      packets = 0;
    const buffer = new Map<number, InputFrame>();
    const observedCorrections: number[] = [];
    let acceptedOwnerTick = 0;
    type Event =
      | { at: number; order: number; kind: "input"; frames: InputFrame[] }
      | { at: number; order: number; kind: "owner"; state: OwnerState }
      | { at: number; order: number; kind: "ping"; sent: number }
      | { at: number; order: number; kind: "pong"; sent: number; tick: number };
    let events: Event[] = [];
    const random = () => {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      return seed / 0x100000000;
    };
    const delay = () => profile.rtt / 2 + (random() * 2 - 1) * profile.jitter;
    const rtc = (event: Event) => {
      packets++;
      if (random() < profile.loss) return;
      if (random() < 0.05) event.at += 40;
      events.push(event);
      if (random() < 0.01)
        events.push({
          ...event,
          at: event.at + random() * 40,
          order: sequence++,
        });
    };
    authority.reset({
      version: 1,
      tick: 0,
      control_epoch: 1,
      controlled_entity_id: 1,
      motor: authoritative,
    });
    clock.observe(0, 0);
    p.install(owner(0), 0);
    try {
      for (let frame = 0; frame <= 60 * 120; frame++) {
        const now = (frame * 1000) / 120;
        if (frame % 120 === 0)
          events.push({
            at: now + delay(),
            order: sequence++,
            kind: "ping",
            sent: now,
          });
        events.sort((a, b) => a.at - b.at || a.order - b.order);
        const due = events.filter((e) => e.at <= now);
        events = events.filter((e) => e.at > now);
        for (const e of due) {
          if (e.kind === "owner") {
            if (e.state.server_tick > acceptedOwnerTick) {
              p.reconcile(e.state, now);
              observedCorrections.push(p.correction);
              acceptedOwnerTick = e.state.server_tick;
            }
          } else if (e.kind === "ping")
            events.push({
              at: now + delay(),
              order: sequence++,
              kind: "pong",
              sent: e.sent,
              tick: serverTick,
            });
          else if (e.kind === "pong") clock.pong(e.tick, e.sent, now);
          else
            for (const input of e.frames) {
              if (input.tick <= serverTick) {
                late++;
                continue;
              }
              if (input.tick > serverTick + 9) continue;
              const previous = buffer.get(input.tick);
              if (
                previous &&
                JSON.stringify(previous) !== JSON.stringify(input)
              )
                conflicts++;
              if (!previous) buffer.set(input.tick, input);
            }
        }
        while (serverTick + 1 <= Math.floor((now * 30) / 1000 + 1e-8)) {
          const tick = ++serverTick;
          authoritative = authority.step(
            buffer.get(tick) ?? {
              tick,
              move_x: 0,
              move_z: 0,
              look_yaw: authoritative.yaw,
              look_pitch: 0,
              buttons: 0,
            },
          ).motor;
          buffer.delete(tick);
          if (tick % 3 === 0) {
            const envelope = {
              v: 3,
              world_id: "test-world",
              session_id: "test-session",
              message: {
                kind: "owner_state",
                data: owner(tick, authoritative),
              },
            };
            const bytes = encode(envelope);
            expect(bytes.length).toBeLessThanOrEqual(1100);
            const decoded = decodeServer(bytes, "unreliable");
            if (decoded.message.kind !== "owner_state")
              throw new Error("owner codec");
            rtc({
              at: now + delay(),
              order: sequence++,
              kind: "owner",
              state: decoded.message.data,
            });
          }
        }
        const inputs = p.advance(now, () => {
          const phase = Math.floor(generated++ / 120) % 4;
          return {
            ...controls,
            move_x: phase === 1 ? 32767 : phase === 3 ? -32767 : 0,
            move_z: phase === 0 ? 32767 : phase === 2 ? -32767 : 0,
            buttons: generated % 90 === 0 ? 2 : 0,
          };
        });
        if (inputs.length) {
          const frames = p.batch();
          const bytes = encodeClient({
            v: 3,
            world_id: "test-world",
            session_id: "test-session",
            message: {
              kind: "input_batch",
              data: { control_epoch: 1, frames },
            },
          });
          expect(bytes.length).toBeLessThanOrEqual(1100);
          rtc({ at: now + delay(), order: sequence++, kind: "input", frames });
        }
        expect(buffer.size).toBeLessThanOrEqual(9);
        expect(p.history.size).toBeLessThanOrEqual(128);
      }
      const sortedCorrections = [...observedCorrections].sort((a, b) => a - b);
      const fullP95 =
        sortedCorrections[Math.ceil(sortedCorrections.length * 0.95) - 1] ?? 0;
      console.info(
        `Prediction ${profile.label} seeded static impairment: ${serverTick} ticks; correction p95=${fullP95.toFixed(6)} m; max=${Math.max(...observedCorrections).toFixed(6)} m; late redundant frames=${late}; packets=${packets}; conflicts=${conflicts}; resync=${failures.length}`,
      );
      expect(serverTick).toBe(1800);
      expect(p.corrections.length).toBe(256);
      expect(fullP95).toBeLessThan(profile.p95);
      expect(conflicts).toBe(0);
      expect(failures).toEqual([]);
    } finally {
      client.free();
      authority.free();
    }
  },
);

it("generates the same fixed-tick motor across 30, 60 and 144 Hz render schedules", async () => {
  const results: CharacterMotor[] = [];
  for (const fps of [30, 60, 144]) {
    const w = await world(),
      clock = new ServerClock();
    const failures: string[] = [];
    const p = new LocalPrediction(w, clock, (reason) => failures.push(reason));
    clock.observe(0, 0);
    p.install(owner(0), 0);
    try {
      for (let frame = 0; frame <= fps * 2.1; frame++)
        p.advance((frame * 1000) / fps, () => ({
          ...controls,
          buttons: p.tick + 1 === 24 ? 2 : 0,
        }));
      const state = p.history.get(60)?.motor;
      if (!state) throw new Error("Missing fixed tick 60");
      results.push(state);
      expect(failures).toEqual([]);
    } finally {
      w.free();
    }
  }
  expect(results[1]).toEqual(results[0]);
  expect(results[2]).toEqual(results[0]);
});
it("freezes an arrested owner under held controls and resumes only with the respawn epoch", async () => {
  const w = await world(),
    clock = new ServerClock();
  const p = new LocalPrediction(w, clock, () => {
    throw new Error("unexpected resync");
  });
  const arrested = owner(10);
  arrested.player_state.mode = "arrested";
  if (arrested.state.kind === "character")
    arrested.state.data.mode = "arrested";
  clock.observe(10, 0);
  p.install(arrested, 0);
  const position = structuredClone(w.currentState().motor.position);
  for (let frame = 0; frame < 5; frame++) p.advance(frame * 33, () => controls);
  expect(w.currentState().motor.position).toEqual(position);
  expect(
    [...p.history.values()].every(
      (h) => h.input.move_z === 0 && h.input.buttons === 0,
    ),
  ).toBe(true);
  const tick = p.tick;
  p.install(owner(tick, initial, 2), 165);
  p.advance(198, () => controls);
  expect(w.currentState().motor.position).not.toEqual(position);
  w.free();
});
