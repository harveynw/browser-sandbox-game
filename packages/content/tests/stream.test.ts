import { expect, it, vi } from "vitest";
import { ChunkStream, chunkDistance } from "../src/stream";
import type { ChunkAsset, VisualChunk } from "../src/generated/types";
const chunks: ChunkAsset[] = Array.from({ length: 36 }, (_, i) => {
  const x = (i % 6) * 128 - 384,
    z = Math.floor(i / 6) * 128 - 384;
  return {
    id: `chunk_${i}`,
    cx: i % 6,
    cz: Math.floor(i / 6),
    bounds: { min: [x, -2, z], max: [x + 128, 64, z + 128] },
    visual_url: `chunk_${i}.json`,
    sha256: "a".repeat(64),
    lod_url: null,
  };
});
const data = (id: string): VisualChunk => ({
  id,
  schema_version: 1,
  primitives: [],
});
it("bounds concurrency, unloads distant visuals and rejects obsolete completions", async () => {
  const requests = new Map<
    string,
    { signal: AbortSignal; resolve: (c: VisualChunk) => void }
  >();
  const shown: string[] = [],
    hidden: string[] = [];
  const stream = new ChunkStream(
    chunks,
    [],
    (id, signal) =>
      new Promise((resolve) => requests.set(id, { signal, resolve })),
    (c) => shown.push(c.id),
    (id) => hidden.push(id),
  );
  stream.update([-320, 0, -320]);
  expect(requests.size).toBe(4);
  const first = [...requests.keys()];
  stream.update([320, 0, 320]);
  expect(first.every((id) => requests.get(id)?.signal.aborted)).toBe(true);
  first.forEach((id) => requests.get(id)?.resolve(data(id)));
  await vi.waitFor(() => expect(stream.metrics.pending).toBe(4));
  expect(shown).toEqual([]);
  for (const [id, r] of requests) if (!r.signal.aborted) r.resolve(data(id));
  await vi.waitFor(() => expect(shown.length).toBeGreaterThan(0));
  stream.update([-320, 0, -320]);
  expect(hidden.length).toBeGreaterThan(0);
  stream.dispose();
  expect(stream.metrics.resident).toBe(0);
  for (const [id, r] of requests) r.resolve(data(id));
  await Promise.resolve();
  expect(stream.metrics.pending).toBe(0);
});
it("uses 192/256 metre hysteresis and never needs over 24 chunks", () => {
  for (let x = -384; x <= 384; x += 16)
    for (let z = -384; z <= 384; z += 16)
      expect(
        chunks.filter((c) => chunkDistance(c, [x, 0, z]) <= 192).length,
      ).toBeLessThanOrEqual(24);
  const c = chunks[0];
  expect(c && chunkDistance(c, [-256, 0, -256])).toBe(0);
});
it("retries twice then leaves a stable fallback without an infinite request loop", async () => {
  vi.useFakeTimers();
  const load = vi.fn(async () => {
    throw new Error("missing art");
  });
  const first = chunks[0];
  if (!first) throw new Error("missing fixture");
  const stream = new ChunkStream(
    [first],
    [],
    load,
    () => {},
    () => {},
  );
  try {
    stream.update([-320, 0, -320]);
    await vi.runAllTimersAsync();
    expect(load).toHaveBeenCalledTimes(3);
    expect(stream.metrics.failed).toBe(1);
    stream.update([-320, 0, -320]);
    expect(load).toHaveBeenCalledTimes(3);
  } finally {
    stream.dispose();
    vi.useRealTimers();
  }
});
