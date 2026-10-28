import { defaults, keyLabel, type Bindings } from "../game/settings";

/** Current bindings, shared by the help panel and its readable key labels. */
export function controlGroups(bindings: Bindings) {
  const key = (action: keyof Bindings) => keyLabel(bindings[action]);
  return [
    {
      title: "Movement",
      rows: [
        [
          [key("forward"), key("left"), key("backward"), key("right")].join(
            " / ",
          ) + " · Arrows",
          "Move / steer",
        ],
        [key("sprint"), "Sprint"],
        [key("jump"), "Jump"],
        ["Mouse / drag", "Look around"],
      ],
    },
    {
      title: "Vehicles",
      rows: [
        [key("interact"), "Enter / exit vehicle"],
        [key("jump"), "Handbrake"],
      ],
    },
    {
      title: "Combat",
      rows: [
        [`RMB / ${key("aim")}`, "Hold / toggle aim"],
        [`LMB / ${key("fire")}`, "Fire while aiming"],
        [key("reload"), "Reload"],
      ],
    },
    {
      title: "World & menus",
      rows: [
        [key("job"), "Jobs / repair"],
        [key("map"), "City map"],
        ["Esc", "Pause / menu"],
      ],
    },
  ];
}

export class ControlsPanel {
  readonly element = document.createElement("details");
  private readonly list = document.createElement("div");
  private readonly context = document.createElement("p");
  constructor() {
    this.element.className = "game-metrics game-controls";
    const summary = document.createElement("summary");
    summary.textContent = "Controls";
    this.list.className = "control-groups";
    this.context.className = "control-context";
    this.context.hidden = true;
    this.element.append(summary, this.list, this.context);
    this.update(defaults);
  }
  update(bindings: Bindings): void {
    this.list.replaceChildren(
      ...controlGroups(bindings).map((group) => {
        const section = document.createElement("section");
        const title = document.createElement("h3");
        title.textContent = group.title;
        const list = document.createElement("dl");
        for (const [keys, action] of group.rows) {
          const term = document.createElement("dt"),
            key = document.createElement("kbd"),
            description = document.createElement("dd");
          key.textContent = keys ?? "";
          term.append(key);
          description.textContent = action ?? "";
          list.append(term, description);
        }
        section.append(title, list);
        return section;
      }),
    );
  }
  setContext(prompt: string, tutorial: string): void {
    const text = [prompt, tutorial].filter(Boolean).join("\n\n");
    if (this.context.textContent !== text) this.context.textContent = text;
    this.context.hidden = !text;
  }
}
