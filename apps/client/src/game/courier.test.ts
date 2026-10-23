import { expect, it } from "vitest";
import type { MissionView } from "@port-meridian/protocol";
import { courierGuidance, repairGuidance } from "./courier";
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
it("uses private trial indices and recovery exit state for objective guidance", () => {
  const trial = {
    id: "trial",
    terminal_zone_id: "trial",
    duration_ticks: 3600,
    reward_credits: 200,
    objective: {
      kind: "time_trial" as const,
      data: {
        checkpoints: [
          [0, 0, 0],
          [1, 0, 0],
          [0, 0, 0],
        ] as [number, number, number][],
        gate_radius: 8,
      },
    },
  };
  expect(
    courierGuidance(
      { ...mission, checkpoint_index: 1 },
      0,
      [0, 0, 0],
      undefined,
      trial,
    ),
  ).toContain("gate 1/2");
  expect(
    courierGuidance(
      { ...mission, phase: "completed" },
      0,
      [0, 0, 0],
      undefined,
      trial,
    ),
  ).toContain("+200 credits");
  const recovery = {
    ...trial,
    reward_credits: 250,
    objective: {
      kind: "recovery" as const,
      data: { parking_ids: ["slot"], garage_zone_id: "garage" },
    },
  };
  expect(
    courierGuidance(
      { ...mission, checkpoint_index: 0 },
      0,
      [0, 0, 0],
      undefined,
      recovery,
    ),
  ).toContain("Collect your secured");
  expect(
    courierGuidance(
      { ...mission, checkpoint_index: 2 },
      0,
      [0, 0, 0],
      undefined,
      recovery,
    ),
  ).toContain("exit blocked");
});

it("distinguishes repair dwell, uncommitted payment and compensating refunds", () => {
  const r = {
    vehicle_id: 1,
    zone_id: "repair",
    phase: "dwell" as const,
    price: 50,
    dwell_ticks: 30,
    code: "ok" as const,
  };
  expect(repairGuidance(r)).toContain("50%");
  expect(repairGuidance({ ...r, phase: "debit_pending" })).toContain(
    "payment pending",
  );
  expect(repairGuidance({ ...r, phase: "refund_pending" })).toContain(
    "refund of 50 credits pending",
  );
  expect(repairGuidance({ ...r, phase: "completed" })).toContain(
    "Repair complete",
  );
  expect(
    repairGuidance({ ...r, phase: "cancelled", code: "unavailable" }),
  ).toContain("no charge");
});
