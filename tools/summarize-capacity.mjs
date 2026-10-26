import process from "node:process";
import console from "node:console";
import { readFile, writeFile } from "node:fs/promises";

const input = process.argv[2];
if (!input)
  throw new Error(
    "Usage: node tools/summarize-capacity.mjs REPORT.json [SUMMARY.json]",
  );
const run = JSON.parse(await readFile(input, "utf8"));
if (run.samples.length < 2)
  throw new Error("Need two capacity samples for byte rates");
const first = run.samples[0],
  last = run.samples.at(-1);
const seconds = last.elapsed_s - first.elapsed_s;
const rate = (a, b) => (b - a) / seconds / 1024;
const quantiles = (values) => {
  values.sort((a, b) => a - b);
  const q = (p) =>
    values[Math.max(0, Math.ceil(p * values.length) - 1)] ?? null;
  return {
    samples: values.length,
    p50_ms: q(0.5),
    p95_ms: q(0.95),
    p99_ms: q(0.99),
    max_ms: values.at(-1) ?? null,
  };
};
const peers = last.server.peers.map((peer) => {
  const before = first.server.peers.find((p) => p.player_id === peer.player_id);
  return {
    player: peer.player_id,
    up_kib_s: before
      ? rate(before.traffic.up_bytes, peer.traffic.up_bytes)
      : null,
    down_kib_s: before
      ? rate(before.traffic.down_bytes, peer.traffic.down_bytes)
      : null,
  };
});
const rss = run.samples
  .map((s) => s.server_rss_kib)
  .filter((n) => typeof n === "number");
const summary = {
  completed: run.completed ?? false,
  profile: run.profile ?? "loopback",
  seed: run.seed ?? 424242,
  measured_interval_s: seconds,
  requested_s: run.requested_s,
  build: last.server.build,
  sampled_players: {
    min: Math.min(...run.samples.map((s) => s.server.population.players)),
    max: Math.max(...run.samples.map((s) => s.server.population.players)),
  },
  final_population: last.server.population,
  server_tick: last.server.populated_tick_work ?? last.server.tick_work,
  server_systems_since_startup: last.server.systems,
  ticks_per_wall_second: (last.server.tick - first.server.tick) / seconds,
  application_egress_kib_s: rate(
    first.server.world.egress_bytes,
    last.server.world.egress_bytes,
  ),
  peers,
  native_peers: last.bots.map((bot) => {
    const before = first.bots.find((b) => b.player === bot.player);
    return {
      player: bot.player,
      join_ms: bot.join_ms,
      up_sent_by_peer_kib_s: before
        ? rate(before.up_game_bytes, bot.up_game_bytes)
        : null,
      down_received_by_peer_kib_s: before
        ? rate(before.down_game_bytes, bot.down_game_bytes)
        : null,
      displacement_path_m: bot.distance_m - (before?.distance_m ?? 0),
      relay: bot.relay ?? null,
    };
  }),
  // Latest ping at each 10-second poll, not every ping. Retain that sampling limitation.
  sampled_native_rtc_rtt: quantiles(
    run.samples.flatMap((s) =>
      s.bots.map((b) => b.rtc_rtt_ms).filter((n) => n > 0),
    ),
  ),
  sampled_owner_lag_ms: quantiles(
    run.samples.flatMap((s) =>
      s.bots.map((b) => (Math.max(0, s.server.tick - b.tick) * 1000) / 30),
    ),
  ),
  server_rss_kib: rss.length
    ? {
        first: rss[0],
        last: rss.at(-1),
        min: Math.min(...rss),
        max: Math.max(...rss),
      }
    : null,
  world_counters: last.server.world,
  final_queues: last.server.queues,
  scope:
    run.scope ??
    "Cumulative histograms since server startup; game bytes before transport. Not a performance qualification.",
};
const text = JSON.stringify(summary, null, 2) + "\n";
if (process.argv[3]) await writeFile(process.argv[3], text);
console.log(text);
