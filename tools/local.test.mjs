import test from "node:test";
import assert from "node:assert/strict";
import process from "node:process";
import { execFile } from "node:child_process";
import { createServer } from "node:net";
import { readFile, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { once } from "node:events";
import { setTimeout as sleep } from "node:timers/promises";
import {
  options,
  nodeVersion,
  temporaryConfig,
  available,
  Processes,
} from "./local.mjs";

test("CLI rejects incomplete/ambiguous options and unsupported Node before launch", () => {
  assert.throws(() => options(["preview", "--port", "9000"]), /Unknown/);
  assert.throws(() => options(["dev", "--config"]), /incomplete/);
  assert.throws(() => options(["dev", "--config", "--no-build"]), /incomplete/);
  assert.throws(
    () => options(["dev", "--temporary", "--config", "config/dev.toml"]),
    /not both/,
  );
  assert.throws(
    () => options(["dev", "--temporary", "--config", "other.toml"]),
    /not both/,
  );
  assert.throws(() => nodeVersion("23.11.0"), /Node 24/);
  assert.equal(options(["dev", "--temporary", "--no-build"]).temporary, true);
});
test("temporary configurations have independent databases and preserve existing files", async () => {
  const parent = await mkdtemp(join(tmpdir(), "pm-config-test-"));
  try {
    const a = await temporaryConfig(parent),
      b = await temporaryConfig(parent);
    assert.notEqual(a.config, b.config);
    const source = await readFile(a.config, "utf8");
    assert.ok(
      source.includes(JSON.stringify(join(a.directory, "profiles.sqlite"))),
    );
    assert.equal(await readFile(a.config, "utf8"), source);
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});
test("occupied port check does not terminate its owner", async () => {
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  try {
    await assert.rejects(
      available(port),
      new RegExp(`Port ${port}.*unavailable`),
    );
    assert.equal(server.listening, true);
  } finally {
    await new Promise((r) => server.close(r));
  }
  await available(port);
});
test("missing toolchain gives an install hint and fails without installing anything", async () => {
  const outcome = await new Promise((r) =>
    execFile(
      process.execPath,
      ["tools/local.mjs", "setup"],
      { env: { ...process.env, PATH: "/no-tools-here" }, timeout: 5000 },
      (error, stdout, stderr) => r({ error, stdout, stderr }),
    ),
  );
  assert.ok(outcome.error);
  assert.match(outcome.stderr, /rustup is missing.*Install rustup/);
});
async function ready(entry) {
  let output = "";
  entry.child.stdout.on("data", (b) => {
    output += b;
  });
  for (let i = 0; i < 100; i++) {
    if (/READY (\d+)/.test(output))
      return Number(output.match(/READY (\d+)/)[1]);
    await sleep(20);
  }
  throw new Error(`fixture did not listen: ${output}`);
}
async function listener(supervisor, grandchild = false) {
  const code =
    'const net=require("node:net");const s=net.createServer();s.listen(0,"127.0.0.1",()=>console.log("READY "+s.address().port));process.on("SIGINT",()=>s.close(()=>process.exit(0)));';
  const parent = grandchild
    ? `require('node:child_process').spawn(process.execPath,['-e',${JSON.stringify(code)}],{stdio:'inherit'});setInterval(()=>{},1000);`
    : code;
  const entry = supervisor.spawn(
    "fixture",
    process.execPath,
    ["-e", parent],
    true,
  );
  return { entry, port: await ready(entry) };
}
test(
  "shutdown terminates process groups including a listening grandchild",
  { timeout: 10000 },
  async () => {
    const supervisor = new Processes();
    try {
      const { port } = await listener(supervisor, true);
      await supervisor.stop(1000);
      await available(port);
    } finally {
      await supervisor.stop(100);
    }
  },
);
test(
  "unexpected child failure stops a live sibling and releases its listener",
  { timeout: 10000 },
  async () => {
    const supervisor = new Processes();
    try {
      const { port } = await listener(supervisor);
      supervisor.spawn(
        "crash",
        process.execPath,
        ["-e", "process.exit(7)"],
        true,
      );
      const error = await supervisor.failure;
      assert.match(error.message, /crash.*7/);
      await supervisor.stop(1000);
      await available(port);
    } finally {
      await supervisor.stop(100);
    }
  },
);

test(
  "failed build wrapper retains ownership of its surviving descendants",
  { timeout: 10000 },
  async () => {
    const supervisor = new Processes();
    const childCode =
      'const s=require("node:net").createServer();s.listen(0,"127.0.0.1",()=>console.log("READY "+s.address().port));process.on("SIGINT",()=>s.close(()=>process.exit(0)));';
    const parentCode = `require('node:child_process').spawn(process.execPath,['-e',${JSON.stringify(childCode)}],{stdio:'inherit'});setTimeout(()=>process.exit(7),200);`;
    const entry = supervisor.spawn("failed-build", process.execPath, [
      "-e",
      parentCode,
    ]);
    try {
      const port = await ready(entry);
      assert.equal((await entry.exited).code, 7);
      await supervisor.stop(1000);
      await available(port);
    } finally {
      await supervisor.stop(100);
      try {
        process.kill(-entry.child.pid, "SIGKILL");
      } catch (error) {
        assert.equal(error.code, "ESRCH");
      }
    }
  },
);
