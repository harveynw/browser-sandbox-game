import { readFile } from "node:fs/promises";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { decodeClient, encodeServer } from "@port-meridian/protocol";
import type {
  EntityShow,
  OwnerState,
  ServerMessage,
} from "@port-meridian/protocol";
import { loadPackage, type LoadedPackage } from "@port-meridian/content";
import init from "../../sim-wasm/sim_wasm";
import trace from "../../../tests/fixtures/simulation/character-trace.json";
import type { Channel } from "../../../apps/client/src/net/transport";
import { GameSession } from "../../../apps/client/src/net/session";

const boundary = vi.hoisted(() => {
  class Transport {
    static latest: Transport;
    state = "idle";
    bytes: ((c: Channel, b: Uint8Array) => void) | undefined;
    closed: ((reason: string) => void) | undefined;
    sent: { channel: Channel; bytes: Uint8Array }[] = [];
    constructor() {
      Transport.latest = this;
    }
    onBytes(fn: Transport["bytes"]) {
      this.bytes = fn;
    }
    onClose(fn: Transport["closed"]) {
      this.closed = fn;
    }
    async connect() {
      this.state = "ready";
    }
    send(channel: Channel, bytes: Uint8Array) {
      this.sent.push({ channel, bytes });
      return "queued";
    }
    close(reason: string) {
      if (this.state === "closed") return;
      this.state = "closed";
      this.closed?.(reason);
      this.bytes = undefined;
    }
  }
  return { Transport };
});
vi.mock("../../../apps/client/src/net/transport", () => ({
  BrowserTransport: boundary.Transport,
}));
vi.mock("@port-meridian/content", () => ({ loadPackage: vi.fn() }));
const motor = trace.initial_motor as OwnerState["player_state"]["motor"];
const state = {
  kind: "character" as const,
  data: { motor, health: 100, mode: "on_foot" as const, vehicle_id: null },
};
const owner = (tick = 10, epoch = 1): OwnerState => ({
  server_tick: tick,
  consumed_through_tick: tick,
  control_epoch: epoch,
  player_id: 1,
  controlled_entity_id: 1,
  state,
  player_state: state.data,
});
const show = (baseline_id: number | null = 1): EntityShow => ({
  baseline_id,
  visibility_id: 1,
  state_tick: 10,
  descriptor: {
    id: 1,
    entity_kind: "player",
    template_id: "player_greybox",
    display_name: "Test",
    tshirt_colour: 0xe4a25b,
  },
  state,
});
const hash = "a".repeat(64);
let live: GameSession[] = [];
function receive(
  message: ServerMessage,
  channel: Channel = "reliable",
  session = "session",
  world = "world",
) {
  boundary.Transport.latest.bytes?.(
    channel,
    encodeServer(
      {
        v: 3,
        world_id: world,
        session_id: message.kind === "server_hello" ? null : session,
        message,
      },
      channel,
    ),
  );
}
function sent() {
  return boundary.Transport.latest.sent.map(
    (p) => decodeClient(p.bytes).message,
  );
}
async function connect() {
  const phases = vi.fn(),
    owners = vi.fn(),
    status = vi.fn(),
    colours = vi.fn();
  const game = new GameSession(() => {}, status, {
    phase: phases,
    owner: owners,
    tshirtColour: colours,
  });
  live.push(game);
  await game.connect();
  receive(
    {
      kind: "server_hello",
      data: {
        protocol_version: 3,
        build_id: "protocol-simple-local",
        content_hash: hash,
        sim_config_hash: hash,
        tick_rate: 30,
        snapshot_rate: 10,
      },
    },
    "reliable",
  );
  receive({
    kind: "welcome",
    data: {
      player_id: 1,
      server_tick: 10,
      input_lead_ticks: 3,
      profile: {
        profile_id: "profile",
        display_name: "Test",
        tshirt_colour: 0xe4a25b,
        credits: 0,
        completed_jobs: 0,
      },
    },
  });
  return { game, phases, owners, status, colours };
}
function baseline(id = 1, visibility = 1, tick = 10) {
  receive({
    kind: "baseline_begin",
    data: { baseline_id: id, server_tick: tick },
  });
  receive({
    kind: "entity_show",
    data: { ...show(id), visibility_id: visibility, state_tick: tick },
  });
  receive({ kind: "baseline_end", data: { baseline_id: id, entity_count: 1 } });
}
function start(id = 1, tick = 10, epoch = 1) {
  receive({
    kind: "start_play",
    data: { baseline_id: id, owner: owner(tick, epoch) },
  });
}
beforeAll(async () => {
  await init({
    module_or_path: await readFile(
      new URL("../../sim-wasm/sim_wasm_bg.wasm", import.meta.url),
    ),
  });
});
beforeEach(() => {
  vi.useFakeTimers({ now: 0 });
  vi.stubGlobal("location", { href: "http://localhost:4173/" });
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async (path: string) =>
        new Response(
          JSON.stringify(
            path.includes("join-ticket")
              ? { join_ticket: "b".repeat(64), ws_url: "ws://127.0.0.1:3000" }
              : {},
          ),
          { status: 200 },
        ),
    ),
  );
  vi.mocked(loadPackage).mockResolvedValue({
    manifest: {
      content_hash: hash,
      sim_config_hash: hash,
      vehicle_templates: [],
    },
    collision: trace.collision,
    sim: trace.sim_config,
  } as unknown as LoadedPackage);
});
afterEach(() => {
  for (const game of live) game.close("test complete");
  live = [];
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("accepts ordered baseline once, discards early snapshots and ignores old scopes after reconnect", async () => {
  const first = await connect();
  receive(
    {
      kind: "snapshot",
      data: {
        server_tick: 12,
        records: [{ entity_id: 1, visibility_id: 1, state }],
      },
    },
    "unreliable",
  );
  expect(first.game.replicas.size).toBe(0);
  baseline();
  start();
  expect(sent().filter((m) => m.kind === "client_ready")).toHaveLength(1);
  expect(first.phases.mock.calls.map((c) => c[0])).toEqual([
    "loading",
    "loading",
    "playing",
  ]);
  const old = boundary.Transport.latest;
  const late = old.bytes;
  first.game.close();
  const next = await connect();
  baseline();
  start();
  receive(
    { kind: "disconnect", data: { code: "timeout", detail: "old" } },
    "reliable",
    "old-session",
  );
  receive(
    { kind: "owner_state", data: owner(100) },
    "unreliable",
    "session",
    "old-world",
  );
  expect(next.owners).toHaveBeenCalledTimes(1);
  expect(next.phases).not.toHaveBeenCalledWith("closed");
  // A callback already captured by an old adapter cannot mutate the replacement session.
  late?.(
    "unreliable",
    encodeServer(
      {
        v: 3,
        world_id: "world",
        session_id: "old-session",
        message: { kind: "owner_state", data: owner(101) },
      },
      "unreliable",
    ),
  );
  expect(next.owners).toHaveBeenCalledTimes(1);
});

it.each([
  "start before end",
  "missing self",
  "count mismatch",
  "wrong show",
  "overlap",
  "duplicate",
])("rejects %s baseline ordering", async (kind) => {
  await connect();
  receive({
    kind: "baseline_begin",
    data: { baseline_id: 1, server_tick: 10 },
  });
  if (kind === "start before end")
    expect(() => start()).toThrow("Invalid StartPlay");
  if (kind === "missing self")
    expect(() =>
      receive({
        kind: "baseline_end",
        data: { baseline_id: 1, entity_count: 0 },
      }),
    ).toThrow("Incomplete baseline");
  if (kind === "count mismatch") {
    receive({ kind: "entity_show", data: show() });
    expect(() =>
      receive({
        kind: "baseline_end",
        data: { baseline_id: 1, entity_count: 2 },
      }),
    ).toThrow("Incomplete baseline");
  }
  if (kind === "wrong show")
    expect(() => receive({ kind: "entity_show", data: show(null) })).toThrow(
      "Wrong baseline show",
    );
  if (kind === "overlap")
    expect(() =>
      receive({
        kind: "baseline_begin",
        data: { baseline_id: 2, server_tick: 10 },
      }),
    ).toThrow("overlapping baseline");
  if (kind === "duplicate") {
    receive({ kind: "entity_show", data: show() });
    expect(() => receive({ kind: "entity_show", data: show() })).toThrow(
      "Duplicate visible entity",
    );
  }
});

it("resumes a ten-second frame gap only after a fresh complete baseline and epoch", async () => {
  const { game, owners } = await connect();
  baseline();
  start();
  const sample = () => ({
    move_x: 0,
    move_z: 32767,
    look_yaw: 0,
    look_pitch: 0,
    buttons: 0,
  });
  game.advance(0, sample);
  const old = game.presentation(0);
  // Pings can continue while rendering is suspended: the frame gap must still stop prediction.
  await vi.advanceTimersByTimeAsync(9999);
  receive({ kind: "owner_state", data: owner(309) }, "unreliable");
  game.advance(10_000, sample);
  expect(sent().filter((m) => m.kind === "resync_request")).toHaveLength(1);
  const count = sent().filter((m) => m.kind === "input_batch").length;
  game.advance(10_100, sample);
  expect(sent().filter((m) => m.kind === "input_batch")).toHaveLength(count);
  expect(() => start()).toThrow("Invalid StartPlay");
  baseline(2, 2, 310);
  start(2, 310, 2);
  game.advance(10_100, () => ({ ...sample(), move_z: 0 }));
  expect(game.metrics(10_100).epoch).toBe(2);
  expect(game.metrics(10_100).history).toBeLessThanOrEqual(9);
  receive({ kind: "owner_state", data: owner(400, 1) }, "unreliable");
  expect(owners.mock.calls.at(-1)?.[0].control_epoch).toBe(2);
  for (let axis = 0; axis < 3; axis++)
    expect(game.presentation(10_100)?.position[axis]).toBeCloseTo(
      old?.position[axis] ?? 0,
      5,
    );
});

it("times out inert loading and releases timers and replicas", async () => {
  const { game, phases } = await connect();
  await vi.advanceTimersByTimeAsync(30_001);
  expect(phases).toHaveBeenLastCalledWith("closed");
  expect(game.replicas.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it("cancels asynchronous asset work before opening transport and ignores captured callbacks after close", async () => {
  const assets = await vi.mocked(loadPackage)(
    new URL("http://localhost/content/city.json"),
  );
  let release: ((assets: LoadedPackage) => void) | undefined;
  vi.mocked(loadPackage).mockImplementationOnce(
    () =>
      new Promise<LoadedPackage>((resolve) => {
        release = resolve;
      }),
  );
  const onAssets = vi.fn();
  const game = new GameSession(
    () => {},
    () => {},
    { assets: onAssets },
  );
  live.push(game);
  const connecting = game.connect();
  const adapter = boundary.Transport.latest;
  const late = adapter.bytes;
  game.close("cancelled");
  release?.(assets);
  await connecting;
  expect(adapter.state).toBe("closed");
  expect(onAssets).not.toHaveBeenCalled();
  expect(fetch).not.toHaveBeenCalled();
  late?.(
    "reliable",
    encodeServer(
      {
        v: 3,
        world_id: "world",
        session_id: "session",
        message: {
          kind: "welcome",
          data: {
            player_id: 1,
            server_tick: 10,
            input_lead_ticks: 3,
            profile: {
              profile_id: "profile",
              display_name: "Test",
              tshirt_colour: 0xe4a25b,
              credits: 0,
              completed_jobs: 0,
            },
          },
        },
      },
      "reliable",
    ),
  );
  expect(game.replicas.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it("saves paused-menu colour changes in order and coalesces drag updates to the latest selection", async () => {
  const { game } = await connect();
  baseline();
  start();
  let finish: ((response: Response) => void) | undefined;
  const requests: number[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((_url: string, init: RequestInit) => {
      const colour = (
        JSON.parse(String(init.body)) as { tshirt_colour: number }
      ).tshirt_colour;
      requests.push(colour);
      if (requests.length === 1)
        return new Promise<Response>((resolve) => {
          finish = resolve;
        });
      return Promise.resolve(
        new Response(JSON.stringify({ profile: { tshirt_colour: colour } }), {
          status: 200,
        }),
      );
    }),
  );
  game.updateTshirtColour(0x112233);
  game.updateTshirtColour(0x445566);
  game.updateTshirtColour(0x778899);
  expect(requests).toEqual([0x112233]);
  finish?.(
    new Response(JSON.stringify({ profile: { tshirt_colour: 0x112233 } }), {
      status: 200,
    }),
  );
  await vi.advanceTimersByTimeAsync(0);
  expect(requests).toEqual([0x112233, 0x778899]);
  game.updateTshirtColour(0x778899);
  await vi.advanceTimersByTimeAsync(0);
  expect(requests).toHaveLength(2);
});

it("updates the owner render colour from a reliable descriptor refresh without resyncing", async () => {
  const { game, colours } = await connect();
  baseline();
  start();
  const messages = sent().length;
  const changed = {
    ...show(null),
    visibility_id: 2,
    descriptor: { ...show().descriptor, tshirt_colour: 0x22cc66 },
  };
  receive({ kind: "entity_show", data: changed });
  expect(colours).toHaveBeenLastCalledWith(0x22cc66);
  expect(
    game.replicas.sampleAll(10, performance.now())[0]?.descriptor.tshirt_colour,
  ).toBe(0x22cc66);
  expect(sent()).toHaveLength(messages);
});
