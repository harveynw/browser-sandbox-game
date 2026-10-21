import { describe, expect, it } from "vitest";
import type { EntityShow, StateRecord } from "@port-meridian/protocol";
import { ReplicaStore } from "./replica";
function show(id = 1, visibility = 1, tick = 0, x = 0): EntityShow {
  return {
    baseline_id: null,
    visibility_id: visibility,
    state_tick: tick,
    descriptor: {
      id,
      entity_kind: "player",
      template_id: "player_greybox",
      display_name: "Test Player",
    },
    state: {
      kind: "character",
      data: {
        motor: {
          position: [x, 0, 0],
          velocity: [3, 0, 0],
          yaw: 3.1,
          grounded: true,
          jump_cooldown_ticks: 0,
        },
        health: 100,
        mode: "on_foot",
        vehicle_id: null,
      },
    },
  };
}
const record = (s: EntityShow): StateRecord => ({
  entity_id: s.descriptor.id,
  visibility_id: s.visibility_id,
  state: s.state,
});
describe("visibility and independent snapshots", () => {
  it("discards pre-spawn/post-hide/old-visibility snapshots and preserves other records at the same tick", () => {
    const store = new ReplicaStore();
    store.snapshot({ server_tick: 3, records: [record(show())] }, 0);
    expect(store.size).toBe(0);
    store.show(show(1), 0);
    store.show(show(2), 0);
    store.snapshot(
      { server_tick: 3, records: [record(show(1, 1, 3, 0.3))] },
      100,
    );
    store.snapshot(
      { server_tick: 3, records: [record(show(2, 1, 3, 0.6))] },
      100,
    );
    expect(store.sampleAll(3, 100).map((r) => r.motor.position[0])).toEqual([
      0.3, 0.6,
    ]);
    store.hide({
      entity_id: 1,
      visibility_id: 1,
      server_tick: 4,
      reason: "out_of_range",
    });
    store.snapshot(
      { server_tick: 6, records: [record(show(1, 1, 6, 100))] },
      200,
    );
    expect(store.size).toBe(1);
    store.show(show(1, 2, 6, 1), 200);
    store.hide({
      entity_id: 1,
      visibility_id: 1,
      server_tick: 4,
      reason: "out_of_range",
    });
    store.snapshot(
      { server_tick: 9, records: [record(show(1, 1, 9, 100))] },
      300,
    );
    store.snapshot(
      { server_tick: 5, records: [record(show(1, 2, 5, 100))] },
      300,
    );
    expect(
      store.sampleAll(6, 300).find((r) => r.id === 1)?.motor.position[0],
    ).toBe(1);
    store.snapshot(
      { server_tick: 12, records: [record(show(1, 2, 12, 1.6))] },
      400,
    );
    expect(
      store.sampleAll(12, 400).find((r) => r.id === 1)?.motor.position[0],
    ).toBe(1.6);
    expect(store.ignored).toBe(4);
  });
  it("interpolates shortest yaw, bounds extrapolation, holds stale actors and snaps mode/teleport transitions", () => {
    const store = new ReplicaStore();
    store.show(show(), 0);
    const next = show(1, 1, 3, 0.3);
    if (next.state.kind === "character") next.state.data.motor.yaw = -3.1;
    store.snapshot({ server_tick: 3, records: [record(next)] }, 100);
    const halfway = store.sampleAll(1.5, 100)[0];
    expect(halfway?.motor.position[0]).toBeCloseTo(0.15);
    expect(Math.abs(halfway?.motor.yaw ?? 0)).toBeCloseTo(Math.PI);
    expect(store.sampleAll(100, 1000)[0]?.stale).toBe(true);
    expect(store.sampleAll(100, 1000)[0]?.motor.position[0]).toBeCloseTo(0.6);
    const teleport = show(1, 1, 6, 100);
    store.snapshot({ server_tick: 6, records: [record(teleport)] }, 200);
    expect(store.sampleAll(4, 200)[0]?.motor.position[0]).toBe(100);
    const downed = show(1, 1, 9, 100.1);
    if (downed.state.kind === "character") {
      downed.state.data.mode = "downed";
      downed.state.data.health = 0;
    }
    store.snapshot({ server_tick: 9, records: [record(downed)] }, 300);
    expect(store.sampleAll(7, 300)[0]?.motor.position[0]).toBe(100.1);
  });
  it("keeps inert presentation during a baseline and bounds timeline/visibility storage", () => {
    const store = new ReplicaStore();
    store.show(show(), 0);
    store.beginBaseline(0, 0);
    store.snapshot(
      { server_tick: 30, records: [record(show(1, 1, 30, 100))] },
      100,
    );
    expect(store.sampleAll(30, 100)[0]?.motor.position[0]).toBe(0);
    store.show(show(1, 2, 30, 1), 100);
    store.finishBaseline();
    for (let tick = 31; tick < 200; tick++)
      store.snapshot(
        { server_tick: tick, records: [record(show(1, 2, tick, 1))] },
        tick * 33,
      );
    expect(store.sampleAll(0, 7000)[0]?.motor.position[0]).toBe(1);
    for (let id = 2; id <= 96; id++) store.show(show(id), 0);
    expect(() => store.show(show(97), 0)).toThrow("overflow");
    store.clear();
    expect(store.size).toBe(0);
  });
});

it("cannot resurrect hidden actors under seeded lost, duplicated and reordered snapshot delivery", () => {
  const store = new ReplicaStore();
  store.show(show(1, 1), 0);
  store.show(show(2, 1), 0);
  let seed = 0x7123;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 0x100000000;
  };
  const packets: { at: number; tick: number; record: StateRecord }[] = [];
  for (let tick = 1; tick <= 120; tick++) {
    if (tick === 30)
      store.hide({
        entity_id: 1,
        visibility_id: 1,
        server_tick: tick,
        reason: "out_of_range",
      });
    if (tick === 60) store.show(show(1, 2, tick, 6), tick * 33);
    if (tick < 30 || tick >= 60) {
      if (random() >= 0.02) {
        const packet = {
          at: tick + Math.floor(random() * 20),
          tick,
          record: record(show(1, tick < 30 ? 1 : 2, tick, tick / 10)),
        };
        packets.push(packet);
        if (random() < 0.1) packets.push({ ...packet, at: packet.at + 10 });
      }
    }
    for (const packet of packets.filter((p) => p.at === tick).reverse())
      store.snapshot(
        { server_tick: packet.tick, records: [packet.record] },
        tick * 33,
      );
    if (tick >= 30 && tick < 60) expect(store.size).toBe(1);
    expect(store.sampleAll(tick, tick * 33).some((r) => r.id === 2)).toBe(true);
    expect(store.size).toBeLessThanOrEqual(2);
  }
  store.hide({
    entity_id: 1,
    visibility_id: 2,
    server_tick: 121,
    reason: "disconnected",
  });
  for (const packet of packets)
    store.snapshot(
      { server_tick: packet.tick, records: [packet.record] },
      5000,
    );
  expect(store.sampleAll(150, 5000).map((r) => r.id)).toEqual([2]);
});

it("interpolates vehicle poses and rejects old occupancy snapshots after a reliable visibility refresh", () => {
  const store = new ReplicaStore();
  const car: EntityShow = {
    baseline_id: null,
    visibility_id: 1,
    state_tick: 0,
    descriptor: {
      id: 9,
      entity_kind: "vehicle",
      template_id: "compact",
      display_name: null,
    },
    state: {
      kind: "vehicle",
      data: {
        motor: {
          position: [0, 0, 0],
          velocity: [3, 0, 0],
          yaw: 0,
          speed: 3,
          steering: 0,
        },
        health: 100,
        driver_id: null,
        disabled: false,
      },
    },
  };
  store.show(car, 0);
  const moved = structuredClone(car);
  moved.state.data.motor.position = [0.6, 0, 0];
  store.snapshot({ server_tick: 6, records: [record(moved)] }, 200);
  expect(store.sampleAll(3, 200)[0]?.motor.position[0]).toBeCloseTo(0.3);
  const occupied = structuredClone(moved);
  occupied.visibility_id = 2;
  occupied.state_tick = 6;
  if (occupied.state.kind === "vehicle") occupied.state.data.driver_id = 7;
  store.show(occupied, 200);
  store.snapshot({ server_tick: 100, records: [record(car)] }, 300);
  expect(store.sampleAll(6, 300)[0]?.state).toMatchObject({
    kind: "vehicle",
    data: { driver_id: 7 },
  });
});
