import { expect, it } from "vitest";
import { mapPoint, zoomMap } from "./city-map";
it("keeps north up through pan and bounds local zoom without changing world coordinates", () => {
  const v = { x: 30, z: 50, zoom: 2 };
  expect(mapPoint(30, 50, v, 800, 600)).toEqual([400, 300]);
  expect(mapPoint(30, 0, v, 800, 600)[1]).toBeLessThan(300);
  expect(mapPoint(80, 50, v, 800, 600)[0]).toBeGreaterThan(400);
  expect(zoomMap(v, 100).zoom).toBe(8);
  expect(zoomMap(v, 0.01).zoom).toBe(1);
  expect(v.zoom).toBe(2);
});
