import generated from "./generated/limits.json";

/** Generated Rust limits; consumers cannot modify the codec's budget constants. */
export const protocolLimits = Object.freeze(generated);
