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
  expect(
    courierGuidance({ ...mission, target_position: null }, 30, [0, 0, 0]),
  ).toContain("reward pending");
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
    variants: [],
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

it("shows the selected contract's payout, deadline, progress and confirmed result", async () => {
  const { missionCard } = await import("./courier");
  const city = await import("../../../../content/source/city.json");
  const template = city.default.mission_templates.find(
    (t) => t.id === "courier_quay_market",
  ) as import("@port-meridian/content").MissionTemplate;
  const selected = {
    ...mission,
    variant_index: 1,
    target_position: [48, 0, -180] as [number, number, number],
  };
  const active = missionCard(selected, template, 4500, [48, 0, -170]);
  expect(active).toMatchObject({
    title: "Civic dispatch",
    reward: "220 CR",
    time: "0:30",
    progress: "10 m",
    urgent: true,
    phase: "active",
  });
  expect(
    missionCard({ ...selected, target_position: null }, template, 6000)?.phase,
  ).toBe("pending");
  expect(
    missionCard({ ...selected, phase: "completed" }, template, 6000),
  ).toMatchObject({ reward: "+220 CR", time: "PAID", urgent: false });
  expect(
    missionCard({ ...selected, phase: "failed" }, template, 6000),
  ).toMatchObject({ reward: "0 CR", time: "ENDED", urgent: false });
  const trial = city.default.mission_templates.find(
    (t) => t.id === "trial_city_ring",
  ) as import("@port-meridian/content").MissionTemplate;
  expect(
    missionCard(
      { ...selected, template_id: trial.id, checkpoint_index: 3 },
      trial,
      0,
    )?.progress,
  ).toBe("Gate 3/6");
  expect(missionCard(null, template, 0)).toBeNull();
});
