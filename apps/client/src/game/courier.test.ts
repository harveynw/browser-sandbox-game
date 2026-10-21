import { expect, it } from "vitest";
import type { MissionView } from "@port-meridian/protocol";
import { courierGuidance } from "./courier";
const mission: MissionView = {
  instance_id: "mission-test",
  template_id: "courier_quay_market",
  phase: "active",
  checkpoint_index: 0,
  deadline_tick: 5400,
  target_entity_id: null,
  target_position: [-44, 0, 52],
};
it("guides to the private target with server-tick countdown and stopping requirements", () => {
  expect(courierGuidance(mission, 30, [-44, 0, 62])).toContain(
    "10 m · 179s left",
  );
  expect(courierGuidance(mission, 5500, [-44, 0, 52])).toContain("0s left");
  expect(courierGuidance(mission, 30, [-44, 0, 62])).toContain("1 second");
});
it("shows success only from committed completion and retains failure feedback", () => {
  expect(courierGuidance(mission, 0, [0, 0, 0])).not.toContain(
    "Delivery complete",
  );
  expect(
    courierGuidance({ ...mission, phase: "completed" }, 0, [0, 0, 0]),
  ).toContain("Delivery complete! +150 credits");
  expect(
    courierGuidance({ ...mission, phase: "failed" }, 0, [0, 0, 0]),
  ).toContain("failed or cancelled");
  expect(courierGuidance(null, 0, undefined)).toContain("Quay terminal");
});
