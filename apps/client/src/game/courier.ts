import type { MissionTemplate } from "@port-meridian/content";
import type { MissionView, RepairView } from "@port-meridian/protocol";
export function courierGuidance(
  mission: MissionView | null,
  tick: number,
  position: readonly number[] | undefined,
  terminalPosition?: readonly number[],
  template?: MissionTemplate,
  inputActive = true,
  terminalName = "Quay",
  choices = "courier / recovery",
): string {
  if (!mission) {
    const metres =
      position && terminalPosition
        ? Math.hypot(
            (terminalPosition[0] ?? 0) - (position[0] ?? 0),
            (terminalPosition[2] ?? 0) - (position[2] ?? 0),
          )
        : undefined;
    const distance = metres === undefined ? "" : ` · ${Math.round(metres)} m`;
    const action =
      metres !== undefined && metres <= 3
        ? inputActive
          ? `F choose ${choices}`
          : `Choose ${choices} from this menu`
        : `Approach within 3 m to choose ${choices}`;
    return `Jobs: cyan ${terminalName} terminal${distance} · ${action} · M city map`;
  }
  const kind = template?.objective.kind ?? "courier";
  const title =
    kind === "time_trial"
      ? "Time trial"
      : kind === "recovery"
        ? "Recovery"
        : "Courier";
  const reward = template?.reward_credits ?? 150;
  if (mission.phase === "completed")
    return `${kind === "courier" ? "Delivery" : title} complete! +${reward} credits · Find a terminal for another job.`;
  if (mission.phase === "failed")
    return `${title} ended — failed or cancelled.${kind === "recovery" && mission.checkpoint_index === 2 ? " Exit blocked: car held until a door clears." : " Return to a terminal to try again."}`;
  if (!mission.target_position)
    return `${title} reward pending · Waiting for committed credits; storage may be temporarily unavailable.`;
  const seconds = Math.max(0, Math.ceil((mission.deadline_tick - tick) / 30));
  const target = mission.target_position;
  const distance =
    target && position
      ? Math.round(
          Math.hypot(
            target[0] - (position[0] ?? 0),
            target[2] - (position[2] ?? 0),
          ),
        )
      : undefined;
  if (kind === "time_trial") {
    const count =
      template?.objective.kind === "time_trial"
        ? template.objective.data.checkpoints.length - 1
        : "?";
    return `City ring trial · gate ${mission.checkpoint_index}/${count} · ${distance ?? "?"} m · ${seconds}s left · Drive through gold gates in order. Esc → Cancel job.`;
  }
  if (kind === "recovery") {
    if (mission.checkpoint_index === 2)
      return "Recovery: exit blocked — clear a door for automatic delivery. No reward until a safe exit.";
    return `Recovery · ${mission.checkpoint_index === 0 ? "Collect your secured Docks van" : "Return your reserved van to Quay garage"} · ${distance ?? "?"} m · ${seconds}s left · ${mission.checkpoint_index === 0 ? "Follow the gold marker; only your reserved van counts." : "Stop inside the garage for 1 second; keep doors clear for automatic exit."} Esc → Cancel job.`;
  }
  return `Courier → Market · ${distance ?? "?"} m · ${seconds}s left · Follow the gold route to its marker. Stop inside its ring for 1 second (below 7 km/h). Esc → Cancel job.`;
}
export function repairGuidance(repair: RepairView | null): string {
  if (!repair) return "";
  switch (repair.phase) {
    case "dwell":
      return `Repair · 50 credits · ${Math.round((repair.dwell_ticks / 60) * 100)}% · Stay stopped in the repair zone for 2 seconds.`;
    case "debit_pending":
      return "Repair payment pending · stay stopped in this car · 50 credits.";
    case "refund_pending":
      return "Repair interrupted · refund of 50 credits pending.";
    case "completed":
      return "Repair complete · full car health · 50 credits paid.";
    case "cancelled":
      return repair.code === "ok"
        ? "Repair cancelled · 50 credits refunded."
        : "Repair cancelled · no charge.";
    case "failed":
      return `Repair failed · ${repair.code.replaceAll("_", " ")} · no charge.`;
  }
}
