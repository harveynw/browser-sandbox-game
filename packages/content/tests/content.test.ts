import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  canonicalJson,
  findRoute,
  courierRoute,
  loadPackage,
  manifestHash,
  sha256,
} from "../src/index";
import fixture from "../src/generated/canonical-fixture.json";
import type { PackageIndex, WorldManifest } from "../src/generated/types";
const root = new URL("../../../apps/client/public/content/", import.meta.url);
const indexUrl = new URL("http://localhost/content/slice.json");
const mockFetch: typeof fetch = async (input) => {
  const url =
    input instanceof URL
      ? input
      : new URL(typeof input === "string" ? input : input.url);
  const relative = url.pathname.replace(/^\/content\//, "");
  if (relative.includes("..")) throw new Error("Unsafe test path");
  const bytes = await readFile(fileURLToPath(new URL(relative, root)));
  return new Response(bytes);
};
async function index(): Promise<PackageIndex> {
  return JSON.parse(
    await readFile(new URL("slice.json", root), "utf8"),
  ) as PackageIndex;
}
describe("baked native content consumed by the browser loader", () => {
  it("loads every native artifact with schema and SHA-256 verification", async () => {
    const data = await loadPackage(indexUrl, undefined, mockFetch);
    expect(data.manifest.chunks).toHaveLength(4);
    expect(data.chunks).toHaveLength(4);
    expect(
      data.collision.colliders.filter((c) => c.role === "boundary"),
    ).toHaveLength(4);
    expect(
      data.collision.colliders.filter((c) => c.role === "landmark"),
    ).toHaveLength(3);
    expect(data.vehicleVisuals[0]?.id).toBe("compact_greybox");
    expect(await manifestHash(data.manifest)).toBe(
      (await index()).content_hash,
    );
    expect(data.sim.character.gravity).toBe(18);
  });
  it("matches the native canonical bytes and hash, including negative zero", async () => {
    expect(canonicalJson(fixture.value)).toBe(fixture.canonical);
    expect(
      await sha256(new TextEncoder().encode(canonicalJson(fixture.value))),
    ).toBe(fixture.sha256);
    expect(canonicalJson({ b: 1, a: -0 })).toBe('{"a":0,"b":1}');
    const sparse: unknown[] = [];
    sparse.length = 1;
    expect(() => canonicalJson(sparse)).toThrow("Sparse");
    for (const value of [
      NaN,
      Infinity,
      0.0000001,
      1000001,
      undefined,
      { é: 1 },
    ])
      expect(() => canonicalJson(value)).toThrow();
  });
  it("follows connected directed road edges and a separate sidewalk route", async () => {
    const { manifest } = await loadPackage(indexUrl, undefined, mockFetch);
    for (const graph of [manifest.road_graph, manifest.pedestrian_graph]) {
      const route = findRoute(graph, "terminal_access", "market_access");
      expect(route[0]).toEqual([-172, 0, 180]);
      expect(route.at(-1)).toEqual([-44, 0, 52]);
      expect(route.length).toBeGreaterThan(4);
      for (let i = 1; i < route.length; i++) {
        const a = route[i - 1],
          b = route[i];
        expect(a && b && (a[0] === b[0] || a[2] === b[2])).toBe(true);
      }
    }
    expect(() =>
      findRoute(
        { ...manifest.road_graph, edges: [] },
        "terminal_access",
        "market_access",
      ),
    ).toThrow("No connected route");
  });
  it("rejects an asset changed after baking instead of displaying a mixed package", async () => {
    const corrupt: typeof fetch = async (input, init) => {
      const response = await mockFetch(input, init);
      if (String(input).endsWith("collision.json"))
        return new Response((await response.text()) + " ");
      return response;
    };
    await expect(loadPackage(indexUrl, undefined, corrupt)).rejects.toThrow(
      "SHA-256 mismatch",
    );
  });
  it("rejects unsafe index paths before fetching outside the content package", async () => {
    let count = 0;
    const unsafe: typeof fetch = async () => {
      count++;
      return new Response(
        JSON.stringify({
          ...(await index()),
          manifest_url: "https://elsewhere.invalid/manifest.json",
        }),
      );
    };
    await expect(loadPackage(indexUrl, undefined, unsafe)).rejects.toThrow(
      "Invalid package index",
    );
    expect(count).toBe(1);
  });
  it("rejects a correctly hashed but schema-invalid manifest", async () => {
    const current = await index();
    const response = await mockFetch(new URL(current.manifest_url, indexUrl));
    const manifest = (await response.json()) as WorldManifest;
    const bad = { ...manifest, unrecognized: true };
    const bytes = new TextEncoder().encode(JSON.stringify(bad));
    const forged: typeof fetch = async (input, init) => {
      if (String(input).endsWith("slice.json"))
        return new Response(
          JSON.stringify({ ...current, manifest_sha256: await sha256(bytes) }),
        );
      if (String(input).endsWith("manifest.json")) return new Response(bytes);
      return mockFetch(input, init);
    };
    await expect(loadPackage(indexUrl, undefined, forged)).rejects.toThrow(
      "schema/version",
    );
  });
  it("caps concurrent asset requests at four and aborts when requested", async () => {
    let active = 0,
      maximum = 0;
    const measured: typeof fetch = async (input, init) => {
      active++;
      maximum = Math.max(maximum, active);
      try {
        return await mockFetch(input, init);
      } finally {
        active--;
      }
    };
    await loadPackage(indexUrl, undefined, measured);
    expect(maximum).toBeLessThanOrEqual(4);
    const controller = new AbortController();
    controller.abort();
    const cancellable: typeof fetch = async (_input, init) => {
      init?.signal?.throwIfAborted();
      throw new Error("should abort first");
    };
    await expect(
      loadPackage(indexUrl, controller.signal, cancellable),
    ).rejects.toThrow();
  });
});

it("retries transient bootstrap failures twice", async () => {
  let failures = 0;
  const transient: typeof fetch = async (input, init) => {
    if (String(input) === String(indexUrl) && failures++ < 2)
      return new Response("unavailable", { status: 503 });
    return mockFetch(input, init);
  };
  await loadPackage(indexUrl, undefined, transient);
  expect(failures).toBe(3);
});

it("boots a streamed city with complete physics and verifies later distant chunks", async () => {
  const cityUrl = new URL("http://localhost/content/city.json");
  const requests: string[] = [];
  const fetcher: typeof fetch = async (input, init) => {
    requests.push(String(input));
    return mockFetch(input, init);
  };
  const data = await loadPackage(cityUrl, undefined, fetcher, {
    streamVisuals: true,
  });
  expect(data.manifest.chunks).toHaveLength(36);
  expect(data.manifest.districts).toHaveLength(6);
  expect(data.chunks).toHaveLength(1);
  expect(data.vehicleVisuals).toHaveLength(3);
  expect(data.collision.colliders.length).toBeGreaterThan(300);
  expect(data.backdrop.id).toBe("backdrop");
  expect(data.minimap.id).toBe("minimap");
  const distant = data.manifest.chunks.find(
    (c) => !data.chunks.some((loaded) => loaded.id === c.id),
  );
  if (!distant) throw new Error("missing distant chunk");
  expect(requests.some((r) => r.endsWith(distant.visual_url))).toBe(false);
  expect((await data.loadChunk(distant.id)).id).toBe(distant.id);
  const corrupt: typeof fetch = async (input, init) =>
    String(input).endsWith(distant.visual_url)
      ? new Response("corrupt distant art")
      : mockFetch(input, init);
  const other = await loadPackage(cityUrl, undefined, corrupt, {
    streamVisuals: true,
  });
  await expect(other.loadChunk(distant.id)).rejects.toThrow("SHA-256 mismatch");
});

it("courier routes end at each assigned variant's delivery zone rather than the old Market destination", async () => {
  const data = await loadPackage(
    new URL("http://localhost/content/city.json"),
    undefined,
    mockFetch,
    { streamVisuals: true },
  );
  const template = data.manifest.mission_templates.find(
    (t) => t.id === "courier_quay_market",
  );
  expect(template?.variants).toHaveLength(3);
  const routes: string[] = [];
  for (const variant of template?.variants ?? []) {
    if (variant.objective.kind !== "courier")
      throw new Error("Expected courier variant");
    const destinationId = variant.objective.data.destination_zone_id;
    const zone = data.manifest.zones.find((z) => z.id === destinationId);
    if (!zone) throw new Error("Missing delivery zone");
    const route = courierRoute(data.manifest, [-172, 0, 180], zone.center);
    expect(route.length).toBeGreaterThan(1);
    expect(route[route.length - 1]).toEqual(zone.center);
    routes.push(JSON.stringify(route));
  }
  expect(new Set(routes).size).toBe(3);
  expect(courierRoute(data.manifest, [-172, 0, 180], [999, 0, 999])).toEqual(
    [],
  );
});
