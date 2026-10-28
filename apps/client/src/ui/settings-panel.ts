import {
  defaults,
  keyLabel,
  rebind,
  validKey,
  type Settings,
  type Control,
} from "../game/settings";
export class SettingsPanel {
  readonly element = document.createElement("details");
  readonly name = document.createElement("label");
  readonly nameInput = document.createElement("input");
  readonly colour = document.createElement("label");
  readonly colourInput = document.createElement("input");
  private colourChanged = false;
  readonly soundStatus = document.createElement("output");
  private abort = new AbortController();
  private waiting: Control | undefined;
  private buttons = new Map<Control, HTMLButtonElement>();
  private message = document.createElement("p");
  constructor(
    private readonly settings: Settings,
    changed: () => void,
  ) {
    const summary = document.createElement("summary");
    summary.textContent = "Settings & controls";
    this.element.append(summary);
    this.colour.textContent = "T-shirt";
    this.colourInput.type = "color";
    this.colourInput.value = settings.tshirtColour;
    this.colourInput.setAttribute("aria-label", "T-shirt");
    this.colour.append(this.colourInput);
    this.colour.className = "player-name";
    this.colourInput.addEventListener(
      "input",
      () => {
        this.colourChanged = true;
        settings.tshirtColour = this.colourInput.value;
        changed();
      },
      { signal: this.abort.signal },
    );
    void this.loadProfileColour(changed);
    this.name.textContent = "Player name";
    this.nameInput.value = settings.name;
    this.nameInput.maxLength = 24;
    this.nameInput.pattern = "[A-Za-z0-9 _-]{1,24}";
    this.nameInput.required = true;
    this.nameInput.setAttribute("aria-label", "Player name");
    this.name.append(this.nameInput);
    this.name.className = "player-name";
    this.nameInput.addEventListener(
      "change",
      () => {
        if (this.nameInput.checkValidity()) {
          settings.name = this.nameInput.value;
          changed();
        }
      },
      { signal: this.abort.signal },
    );
    const row = (text: string, input: HTMLElement) => {
      const label = document.createElement("label");
      label.append(text, input);
      this.element.append(label);
    };
    const range = (
      text: string,
      value: number,
      min: number,
      max: number,
      step: number,
      apply: (n: number) => void,
    ) => {
      const input = document.createElement("input");
      input.type = "range";
      input.min = String(min);
      input.max = String(max);
      input.step = String(step);
      input.value = String(value);
      input.setAttribute("aria-label", text);
      input.oninput = () => {
        apply(Number(input.value));
        changed();
      };
      row(text, input);
    };
    const check = (
      text: string,
      value: boolean,
      apply: (v: boolean) => void,
    ) => {
      const input = document.createElement("input");
      input.type = "checkbox";
      input.checked = value;
      input.onchange = () => {
        apply(input.checked);
        changed();
      };
      row(text, input);
    };
    range(
      "Mouse sensitivity",
      settings.sensitivity,
      0.0005,
      0.006,
      0.0001,
      (v) => (settings.sensitivity = v),
    );
    check("Invert mouse Y", settings.invert, (v) => (settings.invert = v));
    check("Reduced motion", settings.reduced, (v) => (settings.reduced = v));
    const quality = document.createElement("select");
    for (const value of ["medium", "low"] as const) {
      const o = document.createElement("option");
      o.value = value;
      o.textContent =
        value === "medium"
          ? "Medium · detail & shadows"
          : "Low · simpler detail, no shadows";
      quality.append(o);
    }
    quality.value = settings.quality;
    quality.onchange = () => {
      settings.quality = quality.value as Settings["quality"];
      changed();
    };
    row("Graphics quality", quality);
    range(
      "Master volume",
      settings.master,
      0,
      1,
      0.05,
      (v) => (settings.master = v),
    );
    range(
      "Effects volume",
      settings.effects,
      0,
      1,
      0.05,
      (v) => (settings.effects = v),
    );
    check("Mute audio", settings.muted, (v) => (settings.muted = v));
    this.element.append(this.soundStatus);
    const soundNote = document.createElement("p");
    soundNote.className = "game-note";
    soundNote.textContent =
      "Original synthesized effects. No music track. Sound pauses with menus and hidden tabs.";
    this.element.append(soundNote);
    const controls = document.createElement("details"),
      controlTitle = document.createElement("summary");
    controlTitle.textContent = "Remap keys";
    controls.append(controlTitle);
    const names: Record<Control, string> = {
      forward: "Forward / accelerate",
      left: "Left / steer left",
      backward: "Backward / brake",
      right: "Right / steer right",
      sprint: "Sprint",
      jump: "Jump / handbrake",
      interact: "Enter / exit car",
      job: "Jobs / repair",
      aim: "Toggle aim",
      reload: "Reload",
      map: "City map",
      fire: "Keyboard fire (while aiming)",
    };
    for (const action of Object.keys(defaults) as Control[]) {
      const label = document.createElement("label"),
        button = document.createElement("button");
      button.type = "button";
      button.onclick = () => {
        this.waiting = action;
        this.refresh();
        this.message.textContent =
          "Press a letter, number, arrow, Shift or Space. Esc cancels. Existing bindings swap.";
      };
      label.append(names[action], button);
      this.buttons.set(action, button);
      controls.append(label);
    }
    const reset = document.createElement("button");
    reset.textContent = "Reset keys";
    reset.onclick = () => {
      settings.bindings = { ...defaults };
      this.waiting = undefined;
      this.refresh();
      changed();
      this.message.textContent = "Default controls restored.";
    };
    controls.append(reset, this.message);
    this.element.append(controls);
    this.message.setAttribute("role", "status");
    window.addEventListener(
      "keydown",
      (e) => {
        if (!this.waiting) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.code === "Escape") {
          this.waiting = undefined;
          this.message.textContent = "Binding unchanged.";
        } else if (validKey(e.code)) {
          settings.bindings = rebind(settings.bindings, this.waiting, e.code);
          this.waiting = undefined;
          changed();
          this.message.textContent =
            "Binding saved. LMB fires; RMB holds aim. Esc always opens the menu.";
        }
        this.refresh();
      },
      { capture: true, signal: this.abort.signal },
    );
    this.element.addEventListener(
      "toggle",
      () => {
        if (!this.element.open) {
          this.waiting = undefined;
          this.refresh();
        }
      },
      { signal: this.abort.signal },
    );
    this.refresh();
  }
  private async loadProfileColour(changed: () => void): Promise<void> {
    try {
      for (
        let attempt = 0;
        attempt < 30 && !this.abort.signal.aborted;
        attempt++
      ) {
        const response = await fetch("/api/session", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: "{}",
          signal: this.abort.signal,
        });
        if (response.status === 202) {
          await new Promise((resolve) => setTimeout(resolve, 100));
          continue;
        }
        if (!response.ok) return;
        const data = await response.json();
        const colour: unknown = data?.profile?.tshirt_colour;
        if (
          !this.colourChanged &&
          typeof colour === "number" &&
          Number.isInteger(colour) &&
          colour >= 0 &&
          colour <= 0xffffff
        ) {
          this.settings.tshirtColour = `#${colour.toString(16).padStart(6, "0")}`;
          this.colourInput.value = this.settings.tshirtColour;
          changed();
        }
        return;
      }
    } catch {
      /* Offline menus retain the last local choice. */
    }
  }
  private refresh(): void {
    for (const [action, button] of this.buttons)
      button.textContent =
        this.waiting === action
          ? "Press a key…"
          : keyLabel(this.settings.bindings[action]);
  }
  cancelRemap(): void {
    this.waiting = undefined;
    this.refresh();
  }
  dispose(): void {
    this.abort.abort();
  }
}
