import type { MissionCard } from "../game/courier";
export interface HudModel {
  guidance: string;
  objectiveLabel: string;
  mission: MissionCard | null;
  credits: number;
  health: number;
  heat: number;
  pursuit: string;
  ammo: string;
  magazine: number;
  vehicle: string;
  vehicleHealth: { current: number; maximum: number } | null;
  warning: string;
}
/** Values are read-only presentation snapshots, refreshed by the app at at most 10 Hz. */
export class GameHud {
  readonly element = document.createElement("section");
  private objective = document.createElement("div");
  private objectiveLabel = document.createElement("span");
  private contractTitle = document.createElement("h2");
  private contractStats = document.createElement("div");
  private contractTime = document.createElement("strong");
  private contractReward = document.createElement("strong");
  private contractProgress = document.createElement("strong");
  private contractMeter = document.createElement("div");
  private contractFill = document.createElement("i");
  private objectiveText = document.createElement("p");
  private wallet = document.createElement("div");
  private vitals = document.createElement("div");
  private healthFill = document.createElement("i");
  private ammoFill = document.createElement("i");
  private healthBar = document.createElement("div");
  private ammoBar = document.createElement("div");
  private vehicleFill = document.createElement("i");
  private vehicleBar = document.createElement("div");
  private vehicleRow = document.createElement("div");
  private combat = document.createElement("p");
  private wanted = document.createElement("div");
  private warning = document.createElement("p");
  constructor() {
    this.element.className = "game-hud";
    this.objective.className = "hud-objective";
    this.objectiveLabel.className = "hud-objective-label";
    this.contractTitle.className = "hud-contract-title";
    this.contractStats.className = "hud-contract-stats";
    for (const [label, value] of [
      ["Time", this.contractTime],
      ["Reward", this.contractReward],
      ["Objective", this.contractProgress],
    ] as const) {
      const cell = document.createElement("div");
      const caption = document.createElement("span");
      caption.textContent = label;
      cell.append(caption, value);
      this.contractStats.append(cell);
    }
    this.contractMeter.className = "hud-contract-meter";
    this.contractMeter.setAttribute("aria-hidden", "true");
    this.contractMeter.append(this.contractFill);
    this.objective.append(
      this.objectiveLabel,
      this.contractTitle,
      this.contractStats,
      this.objectiveText,
      this.contractMeter,
    );
    this.wallet.className = "hud-wallet";
    this.vitals.className = "hud-vitals";
    this.warning.className = "hud-warning";
    this.wanted.className = "hud-wanted";
    this.combat.className = "hud-combat";
    for (const [label, bar, fill] of [
      ["Health", this.healthBar, this.healthFill],
      ["Ammo", this.ammoBar, this.ammoFill],
      ["Vehicle", this.vehicleBar, this.vehicleFill],
    ] as const) {
      const row =
        label === "Vehicle" ? this.vehicleRow : document.createElement("div");
      row.className = "hud-statrow";
      const caption = document.createElement("span");
      caption.textContent = label;
      bar.className = `hud-bar hud-bar-${label.toLowerCase()}`;
      bar.setAttribute("role", "progressbar");
      bar.setAttribute("aria-label", label);
      bar.setAttribute("aria-valuemin", "0");
      bar.setAttribute("aria-valuemax", label === "Ammo" ? "12" : "100");
      bar.append(fill);
      row.append(caption, bar);
      this.vitals.append(row);
    }
    this.vehicleRow.hidden = true;
    this.vitals.append(this.combat);
    this.element.append(
      this.objective,
      this.wallet,
      this.wanted,
      this.vitals,
      this.warning,
    );
  }
  update(m: HudModel): void {
    const contract = m.mission;
    this.objectiveLabel.textContent = contract
      ? contract.phase === "completed"
        ? "Contract complete"
        : contract.phase === "failed"
          ? "Contract ended"
          : contract.phase === "pending"
            ? "Delivery confirmed"
            : contract.urgent
              ? "Final 30 seconds"
              : "Contract active"
      : m.objectiveLabel;
    this.objective.dataset.phase = contract?.phase ?? "idle";
    this.objective.dataset.urgent = String(contract?.urgent ?? false);
    this.contractTitle.hidden =
      this.contractStats.hidden =
      this.contractMeter.hidden =
        !contract;
    if (contract) {
      this.contractTitle.textContent = contract.title;
      this.contractTime.textContent = contract.time;
      this.contractReward.textContent = contract.reward;
      this.contractProgress.textContent = contract.progress;
      this.contractFill.style.width = `${contract.remaining * 100}%`;
    }
    this.objectiveText.textContent = contract?.instruction ?? m.guidance;
    this.objective.hidden = !m.guidance;
    this.wallet.textContent = `${m.credits.toLocaleString()} CR`;
    const heat = Math.max(0, Math.min(3, Math.round(m.heat)));
    this.wanted.replaceChildren(
      ...Array.from({ length: 3 }, (_, index) => {
        const star = document.createElement("span");
        star.textContent = "★";
        star.className = index < heat ? "hot" : "";
        return star;
      }),
    );
    this.wanted.setAttribute(
      "aria-label",
      `Wanted level ${heat} of 3 · ${m.pursuit}`,
    );
    this.wanted.title = m.pursuit;
    const health = Math.max(0, Math.min(100, m.health));
    const magazine = Math.max(0, Math.min(12, m.magazine));
    this.healthFill.style.width = `${health}%`;
    this.ammoFill.style.width = `${(magazine / 12) * 100}%`;
    this.healthBar.setAttribute("aria-valuenow", String(health));
    this.ammoBar.setAttribute("aria-valuenow", String(magazine));
    this.vehicleRow.hidden = m.vehicleHealth === null;
    if (m.vehicleHealth) {
      const maximum = Math.max(1, m.vehicleHealth.maximum);
      const current = Math.max(0, Math.min(maximum, m.vehicleHealth.current));
      this.vehicleFill.style.width = `${(current / maximum) * 100}%`;
      this.vehicleBar.setAttribute("aria-valuemax", String(maximum));
      this.vehicleBar.setAttribute("aria-valuenow", String(current));
    }
    this.combat.textContent = [m.ammo, m.vehicle, heat > 0 ? m.pursuit : ""]
      .filter(Boolean)
      .join(" · ");
    this.combat.hidden = !this.combat.textContent;
    this.warning.textContent = m.warning;
    this.warning.hidden = !m.warning;
  }
}
export function friendlyProblem(detail: string): string {
  if (/version|mismatch|build/i.test(detail))
    return "The game has been updated. Reload this page, then choose Play.";
  if (/Graphics|WebGL|hardware/i.test(detail)) return detail;
  if (/replaced|joined again/i.test(detail))
    return "This guest joined in another tab. Use a separate browser profile for another player.";
  if (/admission|profile|storage|ticket|session|timed out/i.test(detail))
    return "Joining took too long or the save is temporarily unavailable. Choose Retry. Your committed credits are safe.";
  if (/Left game|Page closed|Rejoining/.test(detail))
    return "You left the city. Choose Retry to return.";
  return "The connection to the local game server was interrupted. Check that the server is running, then choose Retry.";
}
export class Tutorial {
  private start: readonly number[] | undefined;
  private moved = false;
  private drove = false;
  private accepted = false;
  update(
    position: readonly number[],
    driving: boolean,
    mission: boolean,
    nearCar: boolean,
  ): string {
    this.start ??= [...position];
    this.moved ||=
      Math.hypot(
        (position[0] ?? 0) - (this.start[0] ?? 0),
        (position[2] ?? 0) - (this.start[2] ?? 0),
      ) > 3;
    this.drove ||= driving;
    this.accepted ||= mission;
    if (this.accepted) return "";
    if (!this.moved)
      return "01 / Explore · WASD to walk. Mouse to look; drag if mouse capture is unavailable. Esc opens the menu.";
    if (nearCar && !this.drove)
      return "02 / Drive · Stand beside the left door and press E. Driving is optional for your first courier job.";
    return "03 / Earn · Find J on the minimap. Step inside the cyan Quay circle and press F for a courier contract · 120–220 credits.";
  }
}
