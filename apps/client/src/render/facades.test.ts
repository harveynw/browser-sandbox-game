import { expect, it } from "vitest";
import { facadeDetails } from "./facades";
it("adds bounded deterministic decoration without altering collision-bearing authored primitives", () => {
  const p = {
    id: "building-1",
    role: "building" as const,
    center: [0, 5, 0] as [number, number, number],
    half_extents: [10, 5, 10] as [number, number, number],
    color: "#aaaaaa",
    collider_id: 8,
  };
  const copy = structuredClone(p),
    details = facadeDetails([p]);
  expect(details.length).toBeLessThanOrEqual(9);
  expect(details.every((p) => p.collider_id === null)).toBe(true);
  expect(p).toEqual(copy);
  expect(facadeDetails([p])).toEqual(details);
});
