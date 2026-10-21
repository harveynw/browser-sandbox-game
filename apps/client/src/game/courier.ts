import type { MissionView } from "@port-meridian/protocol";
export function courierGuidance(
  mission: MissionView | null,
  tick: number,
  position: readonly number[] | undefined,
  terminalPosition?: readonly number[],
): string {
  if (!mission) {
    const distance =
      position && terminalPosition
        ? `${Math.round(Math.hypot((terminalPosition[0] ?? 0) - (position[0] ?? 0), (terminalPosition[2] ?? 0) - (position[2] ?? 0)))} m`
        : "nearby";
    return `Courier: cyan Quay terminal · ${distance} · E / F accepts · 150 credits`;
  }
  if (mission.phase === "completed")
    return "Delivery complete! +150 credits · Find the Quay terminal for another job.";
  if (mission.phase === "failed")
    return "Courier ended — failed or cancelled. Return to the Quay terminal to try again.";
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
  return `Courier → Market · ${distance ?? "?"} m · ${seconds}s left · Follow the gold route to its marker. Stop inside its ring for 1 second (below 7 km/h). Esc → Cancel courier.`;
}
