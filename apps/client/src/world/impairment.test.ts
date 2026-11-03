import { expect, it } from "vitest";
import {
  encodeServer,
  decodeServer,
  type EntityShow,
  type ServerMessage,
} from "@port-meridian/protocol";
import {
  Scheduler,
  profiles,
} from "../../../../packages/protocol/tests/support/impairment";
import { ReplicaStore } from "./replica";
function show(id: number, visibility: number, tick: number): EntityShow {
  return {
    baseline_id: null,
    visibility_id: visibility,
    state_tick: tick,
    descriptor: {
      id,
      entity_kind: "player",
      template_id: "player_greybox",
      display_name: "Test",
      tshirt_colour: 0xe4a25b,
    },
    state: {
      kind: "character",
      data: {
        motor: {
          position: [tick / 30, 0, id * 3],
          velocity: [1, 0, 0],
          yaw: 0,
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
it.each(profiles)(
  "preserves independent visibility and bounded presentation under $name delivery",
  (profile) => {
    const store = new ReplicaStore();
    const net = new Scheduler<{ bytes: Uint8Array; reliable: boolean }>(
      profile,
    );
    const enqueue = (message: ServerMessage, now: number, reliable: boolean) =>
      net.send(
        {
          bytes: encodeServer(
            { v: 4, world_id: "world", session_id: "session", message },
            reliable ? "reliable" : "unreliable",
          ),
          reliable,
        },
        now,
        reliable,
      );
    if (profile.name === "disconnect") {
      store.show(show(1, 1, 0), 0);
      store.show(show(2, 1, 0), 0);
    }
    let visible = true,
      visibility = 1;
    const receive = (now: number) => {
      for (const p of net.drain(now)) {
        const m = decodeServer(
          p.bytes,
          p.reliable ? "reliable" : "unreliable",
        ).message;
        if (m.kind === "entity_show") store.show(m.data, now);
        if (m.kind === "entity_hide") {
          store.hide(m.data);
          visible = false;
        }
        if (m.kind === "snapshot") store.snapshot(m.data, now);
        if (m.kind === "entity_show" && m.data.descriptor.id === 1)
          visible = true;
      }
    };
    enqueue({ kind: "entity_show", data: show(1, visibility, 0) }, 0, true);
    enqueue({ kind: "entity_show", data: show(2, 1, 0) }, 0, true);
    for (let tick = 1; tick <= 600; tick++) {
      const now = (tick * 1000) / 30;
      if (tick === 150)
        enqueue(
          {
            kind: "entity_hide",
            data: {
              entity_id: 1,
              visibility_id: visibility,
              server_tick: tick,
              reason: "out_of_range",
            },
          },
          now,
          true,
        );
      if (tick === 300) {
        visibility++;
        enqueue(
          { kind: "entity_show", data: show(1, visibility, tick) },
          now,
          true,
        );
      }
      if (tick % 3 === 0) {
        for (const id of [1, 2]) {
          const s = show(id, id === 1 ? visibility : 1, tick);
          enqueue(
            {
              kind: "snapshot",
              data: {
                server_tick: tick,
                records: [
                  {
                    entity_id: id,
                    visibility_id: s.visibility_id,
                    state: s.state,
                  },
                ],
              },
            },
            now,
            false,
          );
        }
      }
      receive(now);
      const actors = store.sampleAll(tick - 3, now);
      expect(actors.length).toBeLessThanOrEqual(2);
      expect(actors.some((a) => a.id === 1)).toBe(visible && store.size === 2);
      for (const actor of actors)
        expect(actor.motor.position.every(Number.isFinite)).toBe(true);
      expect(net.pending).toBeLessThan(100);
    }
    receive(22000);
    expect(store.sampleAll(720, 24000).every((a) => a.stale)).toBe(true);
    if (profile.name === "disconnect")
      expect(net.stats.dropped).toBe(net.stats.sent);
    else
      expect(
        store.sampleAll(600, 20000).find((a) => a.id === 2)?.motor.position[0],
      ).toBeGreaterThan(18);
    // Reliable re-entry with a new visibility invalidates every retained old packet, including a future-tick one.
    store.hide({
      entity_id: 1,
      visibility_id: visibility,
      server_tick: 700,
      reason: "disconnected",
    });
    store.show(show(1, visibility + 1, 701), 23000);
    const stale = show(1, visibility, 900);
    store.snapshot(
      {
        server_tick: 900,
        records: [
          { entity_id: 1, visibility_id: visibility, state: stale.state },
        ],
      },
      23000,
    );
    expect(
      store.sampleAll(701, 23000).find((a) => a.id === 1)?.motor.position[0],
    ).toBeCloseTo(701 / 30);
    console.info(
      `Impairment ${profile.name} seed=0x19c0ffee ${JSON.stringify(net.stats)}`,
    );
  },
);
it("replays the same seeded schedule and preserves reliable FIFO despite jitter", () => {
  const run = () => {
    const net = new Scheduler<number>(profiles[4]);
    for (let i = 0; i < 100; i++) net.send(i, i * 10, i % 2 === 0);
    return { packets: net.drain(10000), stats: net.stats };
  };
  const first = run();
  expect(run()).toEqual(first);
  expect(first.packets.filter((i) => i % 2 === 0)).toEqual(
    Array.from({ length: 50 }, (_, i) => i * 2),
  );
  expect(first.stats.dropped).toBeGreaterThan(0);
});
