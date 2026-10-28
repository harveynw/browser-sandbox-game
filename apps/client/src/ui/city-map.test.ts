import { expect, it } from "vitest";
import { mapPoint, zoomMap, mapDrawOrder } from "./city-map";
it("keeps north up through pan and bounds local zoom without changing world coordinates", () => {
  const v = { x: 30, z: 50, zoom: 2 };
  expect(mapPoint(30, 50, v, 800, 600)).toEqual([400, 300]);
  expect(mapPoint(30, 0, v, 800, 600)[1]).toBeLessThan(300);
  expect(mapPoint(80, 50, v, 800, 600)[0]).toBeGreaterThan(400);
  expect(zoomMap(v, 100).zoom).toBe(8);
  expect(zoomMap(v, 0.01).zoom).toBe(1);
  expect(v.zoom).toBe(2);
});

it("keeps crossing roads clear of either street's pavements regardless of authoring order", () => {
  const rect = (
    id: string,
    role: import("@port-meridian/content").Primitive["role"],
    x: number,
    z: number,
    width: number,
    depth: number,
  ): import("@port-meridian/content").Primitive => ({
    id,
    role,
    center: [x, 0, z],
    half_extents: [width, 0.01, depth],
    color: "#000000",
    collider_id: null,
  });
  const horizontal = [
    rect("horizontal", "road", 0, 0, 10, 1),
    rect("north-pavement", "sidewalk", 0, 2, 10, 1),
    rect("south-pavement", "sidewalk", 0, -2, 10, 1),
  ];
  const vertical = [
    rect("vertical", "road", 0, 0, 1, 10),
    rect("east-pavement", "sidewalk", 2, 0, 1, 10),
    rect("west-pavement", "sidewalk", -2, 0, 1, 10),
  ];
  for (const input of [
    [...horizontal, ...vertical],
    [...vertical, ...horizontal],
  ]) {
    const before = [...input],
      ordered = mapDrawOrder(input);
    const top = (x: number, z: number) =>
      ordered
        .filter(
          (p) =>
            Math.abs(x - p.center[0]) < p.half_extents[0] &&
            Math.abs(z - p.center[2]) < p.half_extents[2],
        )
        .at(-1)?.role;
    for (const [x, z] of [
      [-0.5, 2],
      [0.5, -2],
      [2, 0.5],
      [-2, -0.5],
      [0, 0],
    ] as const)
      expect(top(x, z)).toBe("road");
    expect(top(2, 2)).toBe("sidewalk");
    expect(input).toEqual(before);
  }
});
