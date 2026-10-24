export const defaults = {
  forward: "KeyW",
  left: "KeyA",
  backward: "KeyS",
  right: "KeyD",
  sprint: "ShiftLeft",
  jump: "Space",
  interact: "KeyE",
  job: "KeyF",
  aim: "KeyQ",
  reload: "KeyR",
  map: "KeyM",
  fire: "KeyX",
} as const;
export type Control = keyof typeof defaults;
export type Bindings = Record<Control, string>;
export interface Settings {
  version: 1;
  sensitivity: number;
  invert: boolean;
  reduced: boolean;
  quality: "medium" | "low";
  master: number;
  effects: number;
  muted: boolean;
  name: string;
  bindings: Bindings;
}
const key = "port-meridian.settings.v1";
export function validKey(code: string): boolean {
  return /^(Key[A-Z]|Digit[0-9]|Arrow(Up|Down|Left|Right)|Space|Shift(Left|Right))$/.test(
    code,
  );
}
export function rebind(
  bindings: Bindings,
  action: Control,
  code: string,
): Bindings {
  if (!validKey(code)) return bindings;
  const result = { ...bindings };
  const other = (Object.keys(result) as Control[]).find(
    (a) => result[a] === code,
  );
  if (other) result[other] = result[action];
  result[action] = code;
  return result;
}
export function keyLabel(code: string): string {
  return code
    .replace(/^Key|^Digit/, "")
    .replace("ShiftLeft", "Shift")
    .replace("ShiftRight", "Right Shift")
    .replace("Arrow", "");
}
export function controlText(text: string, bindings: Bindings): string {
  const labels: Record<string, string> = {
    WASD: [bindings.forward, bindings.left, bindings.backward, bindings.right]
      .map(keyLabel)
      .join(" / "),
    Shift: keyLabel(bindings.sprint),
    Space: keyLabel(bindings.jump),
    E: keyLabel(bindings.interact),
    F: keyLabel(bindings.job),
    Q: keyLabel(bindings.aim),
    R: keyLabel(bindings.reload),
    M: keyLabel(bindings.map),
  };
  return text.replace(
    /\b(WASD|Shift|Space|E|F|Q|R|M)\b/g,
    (m) => labels[m] ?? m,
  );
}
export function readSettings(
  storage?: Pick<Storage, "getItem">,
  reduced = false,
): Settings {
  const base: Settings = {
    version: 1,
    sensitivity: 0.002,
    invert: false,
    reduced,
    quality: "medium",
    master: 0.6,
    effects: 0.8,
    muted: false,
    name: "Local Player",
    bindings: { ...defaults },
  };
  try {
    const value: unknown = JSON.parse(storage?.getItem(key) ?? "null");
    if (
      !value ||
      typeof value !== "object" ||
      !("version" in value) ||
      value.version !== 1
    )
      return base;
    const v = value as Partial<Settings>;
    for (const field of ["sensitivity", "master", "effects"] as const) {
      const n = v[field];
      if (typeof n === "number" && Number.isFinite(n))
        base[field] = Math.max(
          field === "sensitivity" ? 0.0005 : 0,
          Math.min(field === "sensitivity" ? 0.006 : 1, n),
        );
    }
    for (const field of ["invert", "reduced", "muted"] as const)
      if (typeof v[field] === "boolean") base[field] = v[field];
    if (v.quality === "low") base.quality = "low";
    if (typeof v.name === "string" && /^[A-Za-z0-9 _-]{1,24}$/.test(v.name))
      base.name = v.name;
    if (
      v.bindings &&
      Object.keys(v.bindings).length === Object.keys(defaults).length &&
      Object.keys(defaults).every(
        (a) =>
          typeof v.bindings?.[a as Control] === "string" &&
          validKey(v.bindings[a as Control]),
      ) &&
      new Set(Object.values(v.bindings)).size === Object.keys(defaults).length
    )
      base.bindings = { ...v.bindings };
  } catch {
    /* Storage may be disabled or contain an older/corrupt value. */
  }
  return base;
}
export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(key, JSON.stringify(settings));
  } catch {
    /* Session settings still work. */
  }
}
