import { readFile, readdir, writeFile } from "node:fs/promises";
import { gzipSync } from "node:zlib";
import { resolve, join } from "node:path";
import process from "node:process";
import console from "node:console";
const root = resolve("apps/client/public/content");
const indexBytes = await readFile(join(root, "city.json"));
const index = JSON.parse(indexBytes.toString("utf8"));
const manifestPath = join(root, index.manifest_url);
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
const base = resolve(manifestPath, "..");
const assets = await Promise.all(
  (await readdir(base))
    .filter((n) => n.endsWith(".json"))
    .sort()
    .map(async (name) => {
      const bytes = await readFile(join(base, name));
      return { name, raw: bytes.length, gzip: gzipSync(bytes).length };
    }),
);
const spawn = manifest.spawns[0].position;
const initialChunks = manifest.chunks.filter(
  (c) =>
    spawn[0] >= c.bounds.min[0] &&
    spawn[0] < c.bounds.max[0] &&
    spawn[2] >= c.bounds.min[2] &&
    spawn[2] < c.bounds.max[2],
);
const initial = new Set([
  "manifest.json",
  manifest.collision_url,
  manifest.sim_config_url,
  manifest.debug_url,
  manifest.backdrop.url,
  manifest.minimap.url,
  ...manifest.vehicle_visuals.map((v) => v.url),
  ...initialChunks.map((c) => c.visual_url),
]);
const parsedChunks = await Promise.all(
  manifest.chunks.map(async (c) =>
    JSON.parse(await readFile(join(base, c.visual_url), "utf8")),
  ),
);
const backdrop = JSON.parse(
  await readFile(join(base, manifest.backdrop.url), "utf8"),
);
let fallbackInstances = 0;
for (const p of backdrop.primitives) {
  if (["ocean", "heights_distant_hill"].includes(p.id)) {
    fallbackInstances++;
    continue;
  }
  for (const c of manifest.chunks) {
    if (
      p.center.every(
        (v, a) =>
          Math.min(v + p.half_extents[a], c.bounds.max[a]) >
          Math.max(v - p.half_extents[a], c.bounds.min[a]),
      )
    )
      fallbackInstances++;
  }
}
const largestResident = parsedChunks
  .map((c) => c.primitives.length)
  .sort((a, b) => b - a)
  .slice(0, 24)
  .reduce((a, b) => a + b, 0);
let buildCodeGzip = 0;
try {
  buildCodeGzip += gzipSync(
    await readFile("apps/client/dist/index.html"),
  ).length;
  for (const name of await readdir("apps/client/dist/assets")) {
    if (/\.(js|css|wasm)$/.test(name))
      buildCodeGzip += gzipSync(
        await readFile(join("apps/client/dist/assets", name)),
      ).length;
  }
} catch {
  /* Build first for a code-inclusive download upper bound. */
}
const collision = JSON.parse(
  await readFile(join(base, manifest.collision_url), "utf8"),
);
const sum = (files, key) => files.reduce((n, a) => n + a[key], 0);
const report = {
  contentHash: index.content_hash,
  districts: manifest.districts.length,
  chunks: manifest.chunks.length,
  staticColliders: collision.colliders.length,
  files: assets.length,
  rawBytes: sum(assets, "raw"),
  gzipBytes: sum(assets, "gzip"),
  initialRawBytes:
    indexBytes.length +
    sum(
      assets.filter((a) => initial.has(a.name)),
      "raw",
    ),
  initialGzipBytes:
    gzipSync(indexBytes).length +
    sum(
      assets.filter((a) => initial.has(a.name)),
      "gzip",
    ),
  allBuildCodeGzipBytes: buildCodeGzip,
  conservativeFirstPlayGzipBytes:
    buildCodeGzip +
    gzipSync(indexBytes).length +
    sum(
      assets.filter((a) => initial.has(a.name)),
      "gzip",
    ),
  collisionBytes: assets.find((a) => a.name === manifest.collision_url).raw,
  maxDetailedResidentChunks: 24,
  maxConcurrentRequests: 4,
  maxRetainedChunkMatrixBytes: (fallbackInstances + largestResident) * 64,
  minimapPixelBytes: (384 * 384 + 256 * 256) * 4,
  assets,
};
if (process.argv[2])
  await writeFile(process.argv[2], JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify({ ...report, assets: undefined }, null, 2));
