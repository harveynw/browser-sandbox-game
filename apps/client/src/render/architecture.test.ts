import citySource from "../../../../content/source/city.json";
import { expect, it } from "vitest";
import type { District, Primitive, Roads } from "@port-meridian/content";
import { architectureDetails, buildingAppearance } from "./architecture";
import { facadeDetails } from "./facades";
const roads: Roads = {
  x: [-30, 30],
  z: [-30, 30],
  carriageway_width: 12,
  sidewalk_width: 3,
};
const district: District = {
  archetypes: [],
  id: "quay",
  name: "Quay",
  rect: { min: [-100, -100], max: [100, 100] },
  color: "#abcdef",
  architecture: {
    style: "quay",
    width: [10, 16],
    depth: [12, 20],
    floors: [2, 4],
    floor_height: 3.2,
  },
};
const building: Primitive = {
  id: "solid_8",
  role: "building",
  center: [0, 6.4, 0],
  half_extents: [8, 6.4, 10],
  color: "#abcdef",
  collider_id: 8,
};
it("keeps procedural facades deterministic, bounded and separate from collision", () => {
  const original = structuredClone(building);
  for (const style of ["quay", "industrial"] as const) {
    const d = {
      ...district,
      architecture: { ...district.architecture, style },
    };
    const details = facadeDetails([building], [d], roads);
    expect(details).toEqual(architectureDetails(building, d, roads));
    expect(details.length).toBeLessThan(260);
    expect(new Set(details.map((p) => p.id)).size).toBe(details.length);
    for (const p of details) {
      expect(p.collider_id).toBeNull();
      expect(p.half_extents.every((n) => Number.isFinite(n) && n > 0)).toBe(
        true,
      );
      expect(Math.abs(p.center[0]) + p.half_extents[0]).toBeLessThanOrEqual(
        8.75,
      );
      expect(Math.abs(p.center[2]) + p.half_extents[2]).toBeLessThanOrEqual(
        10.75,
      );
      expect(p.center[1] - p.half_extents[1]).toBeGreaterThanOrEqual(0);
      expect(p.center[1] + p.half_extents[1]).toBeLessThanOrEqual(15 + 1e-9);
    }
    expect(facadeDetails([building], [d], roads)).toEqual(details);
  }
  expect(building).toEqual(original);
  expect(buildingAppearance(building, [district]).collider_id).toBe(8);
});
it("faces entrances towards the nearest road on all four sides", () => {
  for (const [axis, sign] of [
    [0, -1],
    [0, 1],
    [2, -1],
    [2, 1],
  ] as const) {
    const p = structuredClone(building);
    p.center[axis] = sign * 15;
    const door = architectureDetails(p, district, roads).find((p) =>
      p.id.endsWith("-entrance"),
    );
    if (!door) throw new Error("Missing entrance");
    expect(Math.sign(door.center[axis] - p.center[axis])).toBe(sign);
    expect(Math.abs(door.center[axis] - p.center[axis])).toBeGreaterThan(
      p.half_extents[axis],
    );
  }
});
it("adds floors and bays instead of stretching the window modules", () => {
  const small: Primitive = {
    ...building,
    center: [0, 3.2, 0],
    half_extents: [5, 3.2, 6],
  };
  const panes = (p: Primitive) =>
    architectureDetails(p, district, roads).filter((p) =>
      p.id.includes("-pane-"),
    );
  expect(panes(building).length).toBeGreaterThan(panes(small).length);
  expect(panes(building).every((p) => p.half_extents[1] === 1.55 / 2)).toBe(
    true,
  );
});

it("keeps all authored neighbourhood recipes bounded at minimum and maximum sizes", () => {
  const source = citySource as unknown as { districts: District[] };
  const signatures = new Map<string, string>();
  for (const d of source.districts) {
    for (const size of [0, 1] as const) {
      for (const variant of [8, 9]) {
        const r = d.architecture;
        const height = r.floors[size] * r.floor_height;
        const p: Primitive = {
          ...building,
          id: `solid_${variant}`,
          center: [
            (d.rect.min[0] + d.rect.max[0]) / 2,
            height / 2,
            (d.rect.min[1] + d.rect.max[1]) / 2,
          ],
          half_extents: [r.width[size] / 2, height / 2, r.depth[size] / 2],
        };
        const localRoads = {
          ...roads,
          x: [p.center[0] - 40, p.center[0] + 40],
          z: [p.center[2] - 40, p.center[2] + 20],
        };
        const parts = facadeDetails([p], [d], localRoads);
        expect(parts).toEqual(facadeDetails([p], [d], localRoads));
        expect(parts.length).toBeGreaterThan(20);
        expect(parts.length).toBeLessThan(420);
        expect(new Set(parts.map((p) => p.id)).size).toBe(parts.length);
        for (const part of parts) {
          expect(part.collider_id).toBeNull();
          expect(
            part.half_extents.every((v) => Number.isFinite(v) && v > 0),
          ).toBe(true);
          for (const axis of [0, 2] as const)
            expect(
              Math.abs(part.center[axis] - p.center[axis]) +
                part.half_extents[axis],
            ).toBeLessThanOrEqual(p.half_extents[axis] + 0.75 + 1e-9);
          expect(part.center[1] - part.half_extents[1]).toBeGreaterThanOrEqual(
            -1e-9,
          );
          expect(part.center[1] + part.half_extents[1]).toBeLessThanOrEqual(
            height + 3,
          );
        }
        if (size === 1 && variant === 8)
          signatures.set(r.style, JSON.stringify(parts));
      }
    }
  }
  expect(signatures.size).toBe(6);
});

it("gives each new district its architectural features and street-facing entrances", () => {
  const features = {
    residential: ["balcony-rail", "lobby-door", "lift-house"],
    civic: ["pilaster", "entry-glass", "roof-pavilion"],
    market: ["shop-awning", "shop-door", "roof-tank"],
    dockside: ["cladding", "bay-door", "roof-ridge"],
  } as const;
  for (const style of Object.keys(features) as (keyof typeof features)[]) {
    const d = {
      ...district,
      architecture: { ...district.architecture, style },
    };
    const parts = facadeDetails([building], [d], roads);
    for (const feature of features[style])
      expect(parts.some((p) => p.id.includes(feature))).toBe(true);
  }
});

it("uses baked architecture across district boundaries and suppresses shared-wall details", () => {
  const identity = {
    architecture: { ...district.architecture, style: "industrial" as const },
    purpose: "depot",
    neighbours: [],
  };
  const p = { ...building, building: identity };
  expect(
    facadeDetails([p], [district], roads).some((v) =>
      v.id.includes("loading-door"),
    ),
  ).toBe(true);
  const blocked = {
    ...p,
    building: {
      ...identity,
      neighbours: [
        {
          min: [-8, 0, -12] as [number, number, number],
          max: [8, 30, -10] as [number, number, number],
        },
      ],
    },
  };
  const details = facadeDetails([blocked], [district], roads);
  expect(details.length).toBeLessThan(
    facadeDetails([p], [district], roads).length,
  );
  expect(details.some((v) => v.id.includes("0--1-frame"))).toBe(true);
});
