export interface HudModel {
  guidance: string;
  credits: number;
  health: number;
  heat: number;
  pursuit: string;
  ammo: string;
  vehicle: string;
  warning: string;
}
/** Values are read-only presentation snapshots, refreshed by the app at at most 10 Hz. */
export class GameHud {
  readonly element = document.createElement("section");
  private objective = document.createElement("p");
  private wallet = document.createElement("div");
  private vitals = document.createElement("div");
  private warning = document.createElement("p");
  constructor() {
    this.element.className = "game-hud";
    this.objective.className = "hud-objective";
    this.wallet.className = "hud-wallet";
    this.vitals.className = "hud-vitals";
    this.warning.className = "hud-warning";
    this.element.append(this.objective, this.wallet, this.vitals, this.warning);
  }
  update(m: HudModel): void {
    this.objective.textContent = m.guidance;
    this.wallet.textContent = `${m.credits.toLocaleString()} CR\nHeat ${"●".repeat(m.heat)}${"○".repeat(3 - m.heat)} · ${m.pursuit}`;
    this.vitals.textContent = `HEALTH ${m.health} / 100\n${m.vehicle || m.ammo}`;
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
    return "03 / Earn · Find J on the minimap. Step inside the cyan Quay circle and press F for Courier · 150 credits. M opens the city map.";
  }
}
