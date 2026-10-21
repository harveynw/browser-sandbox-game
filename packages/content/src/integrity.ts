import type { WorldManifest } from "./generated/types";
/** Content canonical JSON v1. This is a restricted decimal format, not RFC 8785. */
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value === "boolean" || typeof value === "string")
    return JSON.stringify(value);
  if (typeof value === "number") {
    if (
      !Number.isFinite(value) ||
      Math.abs(value) > 1_000_000 ||
      Math.abs(value * 1_000_000 - Math.round(value * 1_000_000)) > 0.0001
    )
      throw new Error("Content number outside canonical six-decimal grid");
    if (value === 0) return "0";
    return value.toFixed(6).replace(/0+$/, "").replace(/\.$/, "");
  }
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) {
      if (!Object.hasOwn(value, i)) throw new Error("Sparse content array");
    }
    return `[${value.map(canonicalJson).join(",")}]`;
  }
  if (
    typeof value === "object" &&
    Object.getPrototypeOf(value) === Object.prototype
  ) {
    const record = value as Record<string, unknown>;
    const keys = Object.keys(record).sort();
    if (keys.some((key) => !/^[\x20-\x7e]+$/.test(key)))
      throw new Error("Content object keys must be printable ASCII");
    return `{${keys.map((key) => `${JSON.stringify(key)}:${canonicalJson(record[key])}`).join(",")}}`;
  }
  throw new Error("Not a JSON content value");
}
export async function sha256(bytes: Uint8Array): Promise<string> {
  const hash = await crypto.subtle.digest(
    "SHA-256",
    new Uint8Array(bytes).buffer,
  );
  return Array.from(new Uint8Array(hash), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}
export async function manifestHash(manifest: WorldManifest): Promise<string> {
  const value: Partial<WorldManifest> = { ...manifest };
  delete value.content_hash;
  return sha256(new TextEncoder().encode(canonicalJson(value)));
}
