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
        : `Enter the cyan circle to choose ${choices}.`;
    return `${terminalName} terminal${distance}\n${action}`;
  }
  template = selectedMissionTemplate(template, mission);
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
  return `Courier delivery · ${distance ?? "?"} m · ${seconds}s left · Follow the gold route to its marker. Stop inside its ring for 1 second (below 7 km/h). Esc → Cancel job.`;
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

/** Resolve only the server-selected contract; the browser never rolls objectives. */
export function selectedMissionTemplate(
  template: MissionTemplate | undefined,
  mission: MissionView | null,
): MissionTemplate | undefined {
  if (!template) return undefined;
  const variant =
    mission?.variant_index == null
      ? undefined
      : template.variants?.[mission.variant_index];
  return variant ? { ...template, ...variant } : template;
}

export interface MissionCard {
  title: string;
  phase: "active" | "pending" | "completed" | "failed";
  instruction: string;
  time: string;
  reward: string;
  progress: string;
  remaining: number;
  urgent: boolean;
}
export function missionCard(
  mission: MissionView | null,
  template: MissionTemplate | undefined,
  tick: number,
  position?: readonly number[],
): MissionCard | null {
  if (!mission || !template) return null;
  const chosen = selectedMissionTemplate(template, mission) ?? template;
  const variant =
    mission.variant_index == null
      ? undefined
      : template.variants?.[mission.variant_index];
  const kind = chosen.objective.kind;
  const title =
    variant?.name ??
    (kind === "recovery"
      ? "Docks vehicle recovery"
      : kind === "time_trial"
        ? "City ring trial"
        : "Courier delivery");
  const seconds = Math.max(0, Math.ceil((mission.deadline_tick - tick) / 30));
  const phase =
    mission.phase === "active" && !mission.target_position
      ? "pending"
      : mission.phase;
  const target = mission.target_position;
  const distance =
    target && position
      ? `${Math.round(Math.hypot(target[0] - (position[0] ?? 0), target[2] - (position[2] ?? 0)))} m`
      : "—";
  let instruction =
    "Follow the gold route. Stop inside the delivery ring for 1 second.";
  let progress = distance;
  if (chosen.objective.kind === "time_trial") {
    progress = `Gate ${Math.min(mission.checkpoint_index, chosen.objective.data.checkpoints.length - 1)}/${chosen.objective.data.checkpoints.length - 1}`;
    instruction = `Drive through the next gold gate · ${distance}.`;
  } else if (kind === "recovery") {
    progress =
      mission.checkpoint_index === 0 ? "1/2 · Collect" : "2/2 · Deliver";
    instruction =
      mission.checkpoint_index === 0
        ? `Collect your marked vehicle · ${distance}. Only the reserved vehicle counts.`
        : mission.checkpoint_index === 2
          ? "Exit blocked — clear a door to finish the delivery."
          : `Return to Quay garage · ${distance}. Stop inside and keep the doors clear.`;
  }
  if (phase === "completed")
    instruction = "Payment received. Another contract awaits at a terminal.";
  if (phase === "failed")
    instruction = "No payout. Return to a terminal for a fresh contract.";
  if (phase === "pending")
    instruction = "Objective complete — confirming your payment…";
  return {
    title,
    phase,
    instruction,
    time:
      phase === "active"
        ? `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`
        : phase === "completed"
          ? "PAID"
          : phase === "pending"
            ? "PENDING"
            : "ENDED",
    reward:
      phase === "failed"
        ? "0 CR"
        : `${phase === "completed" ? "+" : ""}${chosen.reward_credits.toLocaleString()} CR`,
    progress:
      phase === "completed"
        ? "Complete"
        : phase === "failed"
          ? "Try again"
          : phase === "pending"
            ? "Confirming"
            : progress,
    remaining:
      phase === "active"
        ? Math.max(
            0,
            Math.min(1, (mission.deadline_tick - tick) / chosen.duration_ticks),
          )
        : phase === "completed"
          ? 1
          : 0,
    urgent: phase === "active" && seconds <= 30,
  };
}
