import { expect, it } from "vitest";
import { Tutorial, friendlyProblem } from "./hud";
it("advances contextual help from observed movement and hides it after job acceptance", () => {
  const t = new Tutorial();
  expect(t.update([0, 0, 0], false, false, false)).toContain("01 / Explore");
  expect(t.update([4, 0, 0], false, false, true)).toContain("02 / Drive");
  expect(t.update([4, 0, 0], true, false, false)).toContain("03 / Earn");
  expect(t.update([4, 0, 0], false, true, false)).toBe("");
  expect(t.update([4, 0, 0], false, false, false)).toBe("");
});
it("turns transport/version failures into player actions without protocol details", () => {
  expect(friendlyProblem("local build mismatch")).toContain("Reload");
  expect(friendlyProblem("session timed out")).toContain("Retry");
  expect(friendlyProblem("guest joined again")).toContain("another tab");
  expect(friendlyProblem("rtc SCTP failed")).not.toContain("SCTP");
});
