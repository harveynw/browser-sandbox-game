import type { PlayerMapEntry } from "@port-meridian/protocol";

/** Server-wide roster, independent of nearby entity visibility. */
export class PlayerList {
  readonly element = document.createElement("section");
  private readonly count = document.createElement("span");
  private readonly body = document.createElement("tbody");
  private previous = "";

  constructor() {
    this.element.className = "player-list";
    this.element.hidden = true;
    this.element.setAttribute("role", "dialog");
    this.element.setAttribute("aria-label", "Online players");
    const heading = document.createElement("header");
    const title = document.createElement("h2");
    title.textContent = "Online players";
    heading.append(title, this.count);
    const table = document.createElement("table");
    const head = document.createElement("thead");
    const row = document.createElement("tr");
    for (const label of ["Player", "Ping"]) {
      const cell = document.createElement("th");
      cell.scope = "col";
      cell.textContent = label;
      row.append(cell);
    }
    head.append(row);
    table.append(head, this.body);
    this.element.append(heading, table);
  }

  update(
    players: readonly PlayerMapEntry[],
    localId: number | undefined,
  ): void {
    const key = JSON.stringify([
      localId,
      players.map((p) => [p.player_id, p.display_name, p.ping_ms]),
    ]);
    if (key === this.previous) return;
    this.previous = key;
    this.count.textContent = `${players.length} online`;
    const rows = [...players]
      .sort(
        (a, b) =>
          a.display_name.localeCompare(b.display_name) ||
          a.player_id - b.player_id,
      )
      .map((player) => {
        const row = document.createElement("tr");
        row.classList.toggle("is-you", player.player_id === localId);
        const name = document.createElement("td");
        name.textContent = player.display_name;
        if (player.player_id === localId) {
          const badge = document.createElement("span");
          badge.className = "player-list-you";
          badge.textContent = "You";
          name.append(badge);
        }
        const ping = document.createElement("td");
        ping.textContent =
          player.ping_ms == null ? "—" : `${player.ping_ms} ms`;
        ping.title =
          player.ping_ms == null
            ? "Waiting for a recent ping"
            : "Round-trip latency";
        row.append(name, ping);
        return row;
      });
    if (!rows.length) {
      const row = document.createElement("tr");
      const cell = document.createElement("td");
      cell.colSpan = 2;
      cell.textContent = "Loading player list…";
      row.append(cell);
      rows.push(row);
    }
    this.body.replaceChildren(...rows);
  }
}
