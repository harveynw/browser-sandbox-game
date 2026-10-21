import Ajv2020 from "ajv/dist/2020.js";
import type { ValidateFunction } from "ajv";
import indexSchema from "./generated/index.schema.json";
import manifestSchema from "./generated/manifest.schema.json";
import collisionSchema from "./generated/collision.schema.json";
import simSchema from "./generated/sim-config.schema.json";
import debugSchema from "./generated/debug.schema.json";
import chunkSchema from "./generated/chunk.schema.json";
import type {
  PackageIndex,
  WorldManifest,
  CollisionPackage,
  SimConfig,
  DebugOverlay,
  VisualChunk,
} from "./generated/types";
import { manifestHash, sha256 } from "./integrity";
const ajv = new Ajv2020({
  strict: true,
  allErrors: false,
  formats: {
    uint32: {
      type: "number",
      validate: (n: number) => Number.isInteger(n) && n >= 0 && n <= 0xffffffff,
    },
    uint16: {
      type: "number",
      validate: (n: number) => Number.isInteger(n) && n >= 0 && n <= 0xffff,
    },
    double: { type: "number", validate: (n: number) => Number.isFinite(n) },
  },
});
const validators = {
  index: ajv.compile<PackageIndex>(indexSchema),
  manifest: ajv.compile<WorldManifest>(manifestSchema),
  collision: ajv.compile<CollisionPackage>(collisionSchema),
  sim: ajv.compile<SimConfig>(simSchema),
  debug: ajv.compile<DebugOverlay>(debugSchema),
  chunk: ajv.compile<VisualChunk>(chunkSchema),
};
export interface LoadedPackage {
  manifest: WorldManifest;
  collision: CollisionPackage;
  sim: SimConfig;
  debug: DebugOverlay;
  chunks: VisualChunk[];
  vehicleVisuals: VisualChunk[];
}
async function boundedBytes(
  response: Response,
  limit: number,
): Promise<Uint8Array> {
  if (!response.ok || !response.body)
    throw new Error(`Content fetch failed (${response.status})`);
  const reader = response.body.getReader();
  let total = 0;
  const pieces: Uint8Array[] = [];
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      total += value.length;
      if (total > limit) throw new Error("Content asset exceeds byte budget");
      pieces.push(value);
    }
  } catch (error) {
    await reader.cancel();
    throw error;
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const piece of pieces) {
    bytes.set(piece, offset);
    offset += piece.length;
  }
  return bytes;
}
function parse<T extends { schema_version: number }>(
  bytes: Uint8Array,
  validate: ValidateFunction<T>,
): T {
  const value: unknown = JSON.parse(
    new TextDecoder("utf-8", { fatal: true }).decode(bytes),
  );
  if (!validate(value) || value.schema_version !== 1)
    throw new Error("Content schema/version validation failed");
  return value;
}
/** Fetch all simplified collision and four primitive chunks. No sockets or game server. */
export async function loadPackage(
  indexUrl: URL,
  signal?: AbortSignal,
  fetcher: typeof fetch = fetch,
): Promise<LoadedPackage> {
  const controller = new AbortController();
  const combined = signal
    ? AbortSignal.any([signal, controller.signal])
    : controller.signal;
  const request = async (url: URL, hash?: string, limit = 2 * 1024 * 1024) => {
    const bytes = await boundedBytes(
      await fetcher(url, { signal: combined, cache: "no-cache" }),
      limit,
    );
    if (hash && (await sha256(bytes)) !== hash)
      throw new Error("Content asset SHA-256 mismatch");
    return bytes;
  };
  try {
    const index = parse(
      await request(indexUrl, undefined, 4096),
      validators.index,
    );
    if (
      !/^[a-f0-9]{64}$/.test(index.content_hash) ||
      index.manifest_url !== `packages/${index.content_hash}/manifest.json` ||
      !/^[a-f0-9]{64}$/.test(index.manifest_sha256)
    )
      throw new Error("Invalid package index path/hash");
    const manifestUrl = new URL(index.manifest_url, indexUrl);
    const manifest = parse(
      await request(manifestUrl, index.manifest_sha256),
      validators.manifest,
    );
    if (
      manifest.content_hash !== index.content_hash ||
      (await manifestHash(manifest)) !== index.content_hash ||
      manifest.chunks.length !== 4 ||
      manifest.vehicle_visuals.length !== 1
    )
      throw new Error("Manifest hash/count mismatch");
    const refs = [
      { url: manifest.collision_url, hash: manifest.collision_sha256 },
      { url: manifest.sim_config_url, hash: manifest.sim_config_hash },
      { url: manifest.debug_url, hash: manifest.debug_sha256 },
      ...manifest.chunks.map((c) => ({ url: c.visual_url, hash: c.sha256 })),
      ...manifest.vehicle_visuals.map((v) => ({ url: v.url, hash: v.sha256 })),
    ];
    if (
      new Set(refs.map((r) => r.url)).size !== refs.length ||
      refs.some(
        (r) =>
          !/^[a-z0-9_-]+\.json$/.test(r.url) || !/^[a-f0-9]{64}$/.test(r.hash),
      )
    )
      throw new Error("Unsafe or duplicate asset reference");
    const bytes: Uint8Array[] = [];
    // Bound concurrency to four, including debug/config requests.
    for (let start = 0; start < refs.length; start += 4) {
      bytes.push(
        ...(await Promise.all(
          refs
            .slice(start, start + 4)
            .map((ref) => request(new URL(ref.url, manifestUrl), ref.hash)),
        )),
      );
    }
    const get = (i: number): Uint8Array => {
      const value = bytes[i];
      if (!value) throw new Error("Missing content bytes");
      return value;
    };
    const chunks = manifest.chunks.map((ref, i) => {
      const chunk = parse(get(i + 3), validators.chunk);
      if (chunk.id !== ref.id) throw new Error("Chunk ID mismatch");
      return chunk;
    });
    const vehicleVisuals = manifest.vehicle_visuals.map((ref, i) => {
      const visual = parse(
        get(3 + manifest.chunks.length + i),
        validators.chunk,
      );
      if (visual.id !== ref.id) throw new Error("Vehicle visual ID mismatch");
      return visual;
    });
    const collision = parse(get(0), validators.collision);
    const sim = parse(get(1), validators.sim);
    const debug = parse(get(2), validators.debug);
    if (JSON.stringify(debug.colliders) !== JSON.stringify(collision.colliders))
      throw new Error("Collision/debug mismatch");
    return { manifest, collision, sim, debug, chunks, vehicleVisuals };
  } catch (error) {
    controller.abort();
    throw error;
  }
}
