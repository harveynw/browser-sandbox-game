import type { MissionCard } from "../game/courier";
type WeaponId = "pistol" | "rifle" | "machine_gun";
const weaponArt: Record<WeaponId, { name: string; path: string }> = {
  pistol: {
    name: "Pistol",
    path: "M17 18H77V23H82V29H49L45 34H39L35 51H22L27 30H17ZM43 30L41 36H50L53 30H49L47 33H45L46 30Z",
  },
  rifle: {
    name: "Rifle",
    path: "M3 26L23 29H34V23H48V18H57V23H79V25H96V29H80V32H61L56 37L61 48H49L45 35H38L33 44H25L28 33L3 38ZM58 33H66V38H60ZM68 30H75V48H66Z",
  },
  machine_gun: {
    name: "Machine gun",
    path: "M2 25L22 28H31V22H44V18H58V22H72V25H96V29H73V33H57L53 37L56 49H45L42 35H34L30 42H24L26 33L2 38ZM59 34H74V46H59ZM77 30L88 49H84L75 34ZM44 15H61V18H44Z",
  },
};
export interface HudModel {
  guidance: string;
  objectiveLabel: string;
  mission: MissionCard | null;
  credits: number;
  health: number;
  heat: number;
  pursuit: string;
  magazine: number;
  totalAmmo: number;
  totalAmmoCapacity: number;
  capacity?: number;
  speedMph?: number | null;
  weapons?: readonly { id: WeaponId; equipped: boolean; owned: boolean }[];
  vehicleHealth: { current: number; maximum: number } | null;
  warning: string;
}
/** Values are read-only presentation snapshots, refreshed by the app at at most 10 Hz. */
export class GameHud {
  readonly element = document.createElement("section");
  get lowerPanels(): readonly HTMLElement[] {
    return [this.vitals, this.speedometer];
  }
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
  private ammoRow = document.createElement("div");
  private vehicleFill = document.createElement("i");
  private vehicleBar = document.createElement("div");
  private vehicleRow = document.createElement("div");
  private totalAmmoBar = document.createElement("div");
  private totalAmmoFill = document.createElement("i");
  private weapons = document.createElement("div");
  private weaponSlots = new Map<WeaponId, HTMLDivElement>();
  private speedometer = document.createElement("div");
  private speedValue = document.createElement("strong");
  private speedArc: SVGPathElement;
  private wanted = document.createElement("div");
  private warning = document.createElement("p");
  private previousHeat = 0;
  private notice = "";
  private noticeUntil = 0;
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
    this.weapons.className = "hud-weapons";
    this.weapons.setAttribute("role", "group");
    this.weapons.setAttribute("aria-label", "Weapons and equip keys");
    for (const [index, id] of (
      ["pistol", "rifle", "machine_gun"] as const
    ).entries()) {
      const slot = document.createElement("div");
      slot.className = "hud-weapon";
      slot.title = `${weaponArt[id].name} · ${index + 1} to equip`;
      // Authored silhouettes are static local SVG; server/player text never becomes markup.
      slot.innerHTML = `<svg viewBox="0 0 100 64" aria-hidden="true"><path fill="currentColor" fill-rule="evenodd" d="${weaponArt[id].path}"/></svg><span class="hud-weapon-key">${index + 1}</span>`;
      this.weaponSlots.set(id, slot);
      this.weapons.append(slot);
    }
    this.speedometer.className = "hud-speedometer";
    this.speedometer.hidden = true;
    this.speedometer.setAttribute("role", "meter");
    this.speedometer.setAttribute("aria-label", "Vehicle speed");
    this.speedometer.setAttribute("aria-valuemin", "0");
    this.speedometer.setAttribute("aria-valuemax", "120");
    const ticks = Array.from({ length: 25 }, (_, i) => {
      const angle = ((135 + (i * 270) / 24) * Math.PI) / 180;
      const inner = i % 4 === 0 ? 41 : 45;
      return `<line x1="${64 + Math.cos(angle) * inner}" y1="${64 + Math.sin(angle) * inner}" x2="${64 + Math.cos(angle) * 49}" y2="${64 + Math.sin(angle) * 49}"/>`;
    }).join("");
    this.speedometer.innerHTML = `<svg viewBox="0 0 128 128" aria-hidden="true"><g class="hud-speed-ticks">${ticks}</g><path class="hud-speed-track" d="M25.11 102.89 A55 55 0 1 1 102.89 102.89"/><path class="hud-speed-arc" pathLength="100" d="M25.11 102.89 A55 55 0 1 1 102.89 102.89"/><text x="23" y="119">0</text><text x="105" y="119">120</text></svg>`;
    const arc =
      this.speedometer.querySelector<SVGPathElement>(".hud-speed-arc");
    if (!arc) throw new Error("Speed dial arc missing");
    this.speedArc = arc;
    const speedCenter = document.createElement("div");
    speedCenter.className = "hud-speed-center";
    const speedUnit = document.createElement("span");
    speedUnit.textContent = "mph";
    speedCenter.append(this.speedValue, speedUnit);
    this.speedometer.append(speedCenter);
    this.warning.className = "hud-warning";
    this.wanted.className = "hud-wanted";
    for (const [label, bar, fill] of [
      ["Health", this.healthBar, this.healthFill],
      ["Ammo", this.ammoBar, this.ammoFill],
      ["Vehicle", this.vehicleBar, this.vehicleFill],
    ] as const) {
      const row =
        label === "Vehicle"
          ? this.vehicleRow
          : label === "Ammo"
            ? this.ammoRow
            : document.createElement("div");
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
    const ammoTrack = document.createElement("div");
    ammoTrack.className = "hud-ammo-split";
    this.ammoBar.replaceWith(ammoTrack);
    ammoTrack.append(this.ammoBar, this.totalAmmoBar);
    this.ammoBar.setAttribute("aria-label", "Magazine");
    this.ammoBar.title = "Rounds in magazine";
    this.totalAmmoBar.className = "hud-bar hud-bar-ammo-total";
    this.totalAmmoBar.setAttribute("aria-label", "Total ammunition");
    this.totalAmmoBar.setAttribute("role", "progressbar");
    this.totalAmmoBar.setAttribute("aria-valuemin", "0");
    this.totalAmmoBar.title = "Total rounds remaining, including magazine";
    this.totalAmmoBar.append(this.totalAmmoFill);
    this.vitals.append(this.weapons, this.speedometer);
    this.element.append(
      this.objective,
      this.wallet,
      this.wanted,
      this.vitals,
      this.warning,
    );
  }
  update(m: HudModel): void {
    this.ammoRow.hidden = m.speedMph != null;
    this.weapons.hidden = m.speedMph != null;
    for (const [id, slot] of this.weaponSlots) {
      const state = m.weapons?.find((weapon) => weapon.id === id);
      const equipped = state?.equipped ?? id === "pistol";
      slot.classList.toggle("equipped", equipped);
      slot.classList.toggle("unowned", state?.owned === false);
      slot.setAttribute(
        "aria-label",
        `${weaponArt[id].name}${equipped ? ", equipped" : ""}${state?.owned === false ? ", unavailable" : ""}`,
      );
    }
    this.speedometer.hidden = m.speedMph == null;
    if (m.speedMph != null) {
      const mph = Math.round(
        Number.isFinite(m.speedMph) ? Math.max(0, m.speedMph) : 0,
      );
      this.speedValue.textContent = String(mph);
      this.speedometer.setAttribute(
        "aria-valuenow",
        String(Math.min(120, mph)),
      );
      this.speedometer.setAttribute("aria-valuetext", `${mph} miles per hour`);
      this.speedArc.style.strokeDashoffset = String(
        100 - Math.min(1, mph / 120) * 100,
      );
    }
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
    this.wanted.classList.toggle(
      "searching",
      m.pursuit.startsWith("Searching"),
    );
    if (heat > this.previousHeat) {
      this.notice =
        [
          "",
          "Police alerted",
          "Armed police responding",
          "Maximum police response",
        ][heat] ?? "";
      this.noticeUntil = performance.now() + 3500;
    }
    this.previousHeat = heat;
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
    const totalCapacity = Math.max(1, m.totalAmmoCapacity);
    const total = Math.max(0, Math.min(totalCapacity, m.totalAmmo));
    this.totalAmmoFill.style.width = `${(total / totalCapacity) * 100}%`;
    this.totalAmmoBar.setAttribute("aria-valuemax", String(totalCapacity));
    this.totalAmmoBar.setAttribute("aria-valuenow", String(total));
    this.totalAmmoBar.setAttribute(
      "aria-valuetext",
      `${total} rounds including the magazine`,
    );
    const capacity = Math.max(1, m.capacity ?? 12);
    const magazine = Math.max(0, Math.min(capacity, m.magazine));
    this.ammoBar.setAttribute("aria-valuemax", String(capacity));
    this.healthFill.style.width = `${health}%`;
    this.ammoFill.style.width = `${(magazine / capacity) * 100}%`;
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
    this.warning.textContent =
      m.warning || (performance.now() < this.noticeUntil ? this.notice : "");
    this.warning.hidden = !this.warning.textContent;
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
