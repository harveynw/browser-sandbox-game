import process from "node:process";
import console from "node:console";
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:net";
import { createInterface } from "node:readline";
import { mkdtemp, writeFile, access } from "node:fs/promises";
import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { setTimeout as sleep } from "node:timers/promises";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const npm = process.env.npm_execpath
  ? [process.execPath, [process.env.npm_execpath]]
  : ["npm", []];

export function options(args) {
  const [mode, ...flags] = args;
  if (!["setup", "dev", "preview", "config"].includes(mode))
    throw new Error(
      "Use npm run setup, dev:all, preview:local or config:test.",
    );
  const out = {
    mode,
    temporary: false,
    build: true,
    config: "config/dev.toml",
  };
  for (let i = 0; i < flags.length; i++) {
    if (flags[i] === "--temporary") out.temporary = true;
    else if (flags[i] === "--no-build") out.build = false;
    else if (
      flags[i] === "--config" &&
      flags[i + 1] &&
      !flags[i + 1].startsWith("--")
    )
      out.config = flags[++i];
    else throw new Error(`Unknown or incomplete option: ${flags[i]}`);
  }
  if (out.temporary && flags.includes("--config"))
    throw new Error("Choose --temporary or --config PATH, not both.");
  if (["setup", "config"].includes(mode) && flags.length)
    throw new Error(`${mode} takes no options.`);
  return out;
}
export function nodeVersion(version) {
  if (Number(version.split(".")[0]) !== 24)
    throw new Error(
      `Node ${version} is unsupported. Select Node 24 (see .nvmrc), then run npm ci.`,
    );
}
function command(command, args, help) {
  try {
    return execFileSync(command, args, {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 10000,
    }).trim();
  } catch {
    throw new Error(`${command} is missing or failed. ${help}`);
  }
}
export function toolchains() {
  nodeVersion(process.versions.node);
  if (process.platform === "win32")
    throw new Error(
      "The local supervisor currently supports macOS/Linux process groups. Use the documented separate-terminal commands on Windows.",
    );
  const pin = readFileSync(join(root, "rust-toolchain.toml"), "utf8").match(
    /channel = "([^"]+)"/,
  )[1];
  const installed = command(
    "rustup",
    ["toolchain", "list"],
    "Install rustup, then follow README prerequisites.",
  );
  if (
    !installed
      .split("\n")
      .some((s) => s.startsWith(`${pin}-`) || s.startsWith(`${pin} `))
  )
    throw new Error(
      `Install the pinned toolchain: rustup toolchain install ${pin} --profile minimal --component rustfmt,clippy --target wasm32-unknown-unknown`,
    );
  const override = process.env.RUSTUP_TOOLCHAIN;
  if (override && override !== pin && !override.startsWith(`${pin}-`))
    throw new Error(
      `RUSTUP_TOOLCHAIN=${override} overrides this project. Unset it or select ${pin} before launch.`,
    );
  const rustc = command(
    "rustc",
    ["--version"],
    "Check Rustup's rustc shim is on PATH.",
  );
  const cargo = command(
    "cargo",
    ["--version"],
    "Check Rustup's Cargo shim is on PATH.",
  );
  if (!rustc.startsWith(`rustc ${pin} `) || !cargo.startsWith(`cargo ${pin} `))
    throw new Error(
      `Expected Rust/Cargo ${pin}; found ${rustc} / ${cargo}. Check PATH and the project toolchain override.`,
    );
  const targets = command(
    "rustup",
    ["target", "list", "--toolchain", pin, "--installed"],
    "Check rustup installation.",
  );
  if (!targets.split("\n").includes("wasm32-unknown-unknown"))
    throw new Error(
      `Missing WASM target. Run rustup target add --toolchain ${pin} wasm32-unknown-unknown`,
    );
  const components = command(
    "rustup",
    ["component", "list", "--toolchain", pin, "--installed"],
    "Check rustup installation.",
  );
  for (const c of ["rustfmt", "clippy"])
    if (!components.split("\n").some((s) => s.startsWith(`${c}-`)))
      throw new Error(
        `Missing ${c}. Run rustup component add --toolchain ${pin} ${c}`,
      );
  const wasm = command(
    "wasm-bindgen",
    ["--version"],
    "Run cargo install wasm-bindgen-cli --version 0.2.129 --locked",
  );
  if (wasm !== "wasm-bindgen 0.2.129")
    throw new Error(
      `Found ${wasm}; run cargo install wasm-bindgen-cli --version 0.2.129 --locked --force`,
    );
  const npmVersion = command(
    npm[0],
    [...npm[1], "--version"],
    "Install npm 10 or 11 with Node 24.",
  );
  if (![10, 11].includes(Number(npmVersion.split(".")[0])))
    throw new Error(`npm ${npmVersion} is unsupported; use npm 10 or 11.`);
  if (!existsSync(join(root, "node_modules/vite/bin/vite.js")))
    throw new Error(
      "Workspace dependencies are missing. Run npm ci, then retry.",
    );
  console.log(
    `[setup] Node ${process.versions.node}, npm ${npmVersion}, Rust ${pin}, ${wasm}, WASM target, rustfmt and Clippy available.`,
  );
}
export async function temporaryConfig(parent = tmpdir()) {
  const directory = await mkdtemp(join(parent, "port-meridian-"));
  const config = join(directory, "server.toml");
  await writeFile(
    config,
    `[persistence]\nmode = "sqlite"\npath = ${JSON.stringify(join(directory, "profiles.sqlite"))}\n`,
    { flag: "wx", mode: 0o600 },
  );
  return { config, directory };
}
export async function available(port) {
  await new Promise((resolvePort, reject) => {
    const socket = createServer();
    socket.once("error", (error) => {
      const hint =
        error.code === "EADDRINUSE"
          ? `Stop its owner first; inspect with lsof -nP -iTCP:${port} -sTCP:LISTEN on macOS/Linux. This launcher never kills unrelated processes.`
          : "Run the launcher in a terminal/environment that permits binding loopback listeners; check the reported OS error.";
      reject(
        new Error(
          `Port ${port} on 127.0.0.1 is unavailable (${error.code}). ${hint}`,
        ),
      );
    });
    socket.listen(port, "127.0.0.1", () => socket.close(resolvePort));
  });
}
function signal(entry, name) {
  if (!entry.child.pid) return;
  try {
    process.kill(-entry.child.pid, name);
  } catch (error) {
    if (error.code !== "ESRCH")
      console.error(`[shutdown] ${entry.label}: ${error.message}`);
  }
}
export class Processes {
  entries = new Set();
  stopping = false;
  interrupted = false;
  failure = new Promise((resolveFailure) => {
    this.fail = resolveFailure;
  });
  spawn(label, command, args, persistent = false) {
    const child = spawn(command, args, {
      cwd: root,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, NO_COLOR: "1" },
    });
    console.log(`[${label}] starting (pid ${child.pid ?? "unavailable"})`);
    const entry = { child, label };
    this.entries.add(entry);
    for (const pipe of [child.stdout, child.stderr])
      createInterface({ input: pipe }).on("line", (line) =>
        console.log(`[${label}] ${line}`),
      );
    entry.exited = new Promise((resolveExit) => {
      child.once("error", (error) => resolveExit({ error }));
      child.once("exit", (code, exitSignal) =>
        resolveExit({ code, signal: exitSignal }),
      );
    }).then((result) => {
      if (!persistent && !result.error && result.code === 0)
        this.entries.delete(entry);
      else if (persistent && !this.stopping)
        this.fail(
          new Error(
            `${label} exited unexpectedly (${result.error?.message ?? result.signal ?? result.code}).`,
          ),
        );
      return result;
    });
    return entry;
  }
  guard(work) {
    return Promise.race([
      work,
      this.failure.then((error) => {
        throw error;
      }),
    ]);
  }
  async run(label, command, args) {
    const result = await this.guard(this.spawn(label, command, args).exited);
    if (result.error || result.code !== 0)
      throw new Error(
        `${label} failed (${result.error?.message ?? result.signal ?? result.code}); see labelled output above.`,
      );
  }
  async stop(grace = 8000) {
    this.stopping = true;
    const entries = [...this.entries];
    const alive = (entry) => {
      if (!entry.child.pid) return false;
      try {
        process.kill(-entry.child.pid, 0);
        return true;
      } catch (error) {
        return error.code !== "ESRCH";
      }
    };
    for (const [name, wait] of [
      ["SIGINT", grace],
      ["SIGTERM", 2000],
      ["SIGKILL", 1000],
    ]) {
      for (const entry of entries) signal(entry, name);
      const deadline = Date.now() + wait;
      while (entries.some(alive) && Date.now() < deadline) await sleep(25);
      if (!entries.some(alive)) break;
    }
    if (entries.some(alive)) {
      console.error(
        "[shutdown] A child process group did not terminate; inspect the labelled PID.",
      );
      process.exitCode = 1;
    }
    await Promise.all(entries.map((e) => e.exited));
  }
}
async function healthy(url, deadline) {
  let last = "not responding";
  while (Date.now() < deadline) {
    try {
      const response = await globalThis.fetch(url, {
        signal: globalThis.AbortSignal.timeout(1000),
      });
      await response.body?.cancel();
      if (response.ok) return;
      last = `HTTP ${response.status}`;
    } catch (e) {
      last = e.message;
    }
    await sleep(100);
  }
  throw new Error(
    `${url} failed startup within 20 seconds (${last}). See server/frontend logs; no sibling will be left running.`,
  );
}
async function prepare(supervisor) {
  await supervisor.run("build", npm[0], [...npm[1], "run", "build"]);
  await supervisor.run("native", "cargo", [
    "build",
    "-p",
    "server",
    "--release",
    "--locked",
    "--bin",
    "server",
    "--example",
    "local_probe",
  ]);
}
export async function main(args = process.argv.slice(2)) {
  const opts = options(args);
  if (opts.mode === "config") {
    const result = await temporaryConfig();
    console.log(
      `Disposable SQLite test config: ${result.config}\nTest data is retained at ${result.directory}; normal saves are untouched.`,
    );
    return;
  }
  toolchains();
  const supervisor = new Processes();
  const interrupt = () => {
    supervisor.interrupted = true;
    supervisor.fail(new Error("Interrupted"));
  };
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  try {
    if (opts.mode === "setup") {
      await prepare(supervisor);
      console.log(
        "[setup] Ready. Run npm run dev:all or npm run preview:local.",
      );
      return;
    }
    const port = opts.mode === "dev" ? 5173 : 4173;
    for (const p of [3000, 3001, port]) await available(p);
    let config = resolve(root, opts.config);
    if (opts.temporary) {
      const test = await temporaryConfig();
      config = test.config;
      console.log(`[config] Disposable database retained at ${test.directory}`);
    }
    await access(config);
    if (opts.build) await prepare(supervisor);
    for (const artifact of [
      "target/release/server",
      "target/release/examples/local_probe",
      "packages/sim-wasm/sim_wasm_bg.wasm",
      ...(opts.mode === "preview" ? ["apps/client/dist/index.html"] : []),
    ]) {
      if (!existsSync(join(root, artifact)))
        throw new Error(
          `Missing ${artifact}. Run npm run setup, or omit --no-build.`,
        );
    }
    supervisor.spawn(
      "server",
      join(root, "target/release/server"),
      ["--config", config],
      true,
    );
    // Full RTC readiness requires the frontend proxy; never wait for a browser-dependent gate before serving the page.
    const deadline = Date.now() + 20000;
    await supervisor.guard(
      healthy("http://127.0.0.1:3001/health/live", deadline),
    );
    supervisor.spawn(
      "frontend",
      process.execPath,
      [
        join(root, "node_modules/vite/bin/vite.js"),
        ...(opts.mode === "preview" ? ["preview"] : []),
        join(root, "apps/client"),
        "--host",
        "127.0.0.1",
        "--port",
        String(port),
        "--strictPort",
      ],
      true,
    );
    await supervisor.guard(
      healthy(`http://127.0.0.1:${port}/health/live`, deadline),
    );
    const page = await globalThis.fetch(`http://127.0.0.1:${port}/`, {
      signal: globalThis.AbortSignal.timeout(2000),
    });
    if (!page.ok || !(await page.text()).includes("Port Meridian"))
      throw new Error("Frontend page failed the startup check.");
    const key = createHash("sha256").update(config).digest("hex").slice(0, 16);
    const cookie = join(root, "var", "probes", `${key}.cookie`);
    const remaining = Math.max(1, deadline - Date.now());
    await supervisor.guard(
      Promise.race([
        supervisor.run(
          "probe",
          join(root, "target/release/examples/local_probe"),
          [String(port), cookie],
        ),
        sleep(remaining, undefined, { ref: false }).then(() => {
          throw new Error(
            "Real WS/RTC startup probe exceeded the 20-second startup deadline.",
          );
        }),
      ]),
    );
    console.log(
      `[ready] http://localhost:${port}/ — HTTP proxy, admission, WS and RTC verified. Ctrl+C stops both processes and flushes saves.`,
    );
    console.log(
      "[ready] Probe guest has left. /health/ready remains a short-lived transport receipt; idle expiry is normal. Use separate browser profiles/private contexts for two players.",
    );
    throw await supervisor.failure;
  } catch (error) {
    if (!supervisor.interrupted) throw error;
  } finally {
    await supervisor.stop();
    process.removeListener("SIGINT", interrupt);
    process.removeListener("SIGTERM", interrupt);
    console.log(
      "[shutdown] Owned process groups stopped; SQLite saves preserved.",
    );
  }
}
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main().catch((error) => {
    console.error(`[local] ${error.message}`);
    process.exitCode = 1;
  });
}
