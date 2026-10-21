import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";

// Match the crate exactly: wasm-bindgen's generated ABI is version specific.
const version = execFileSync("wasm-bindgen", ["--version"], {
  encoding: "utf8",
}).trim();
if (version !== "wasm-bindgen 0.2.129") {
  throw new Error(
    "Install wasm-bindgen-cli 0.2.129: cargo install wasm-bindgen-cli --version 0.2.129 --locked",
  );
}
execFileSync(
  "cargo",
  [
    "build",
    "-p",
    "sim-wasm",
    "--target",
    "wasm32-unknown-unknown",
    "--release",
    "--locked",
  ],
  { stdio: "inherit" },
);
mkdirSync("packages/sim-wasm", { recursive: true });
execFileSync(
  "wasm-bindgen",
  [
    "target/wasm32-unknown-unknown/release/sim_wasm.wasm",
    "--target",
    "web",
    "--out-dir",
    "packages/sim-wasm",
    "--out-name",
    "sim_wasm",
  ],
  { stdio: "inherit" },
);
