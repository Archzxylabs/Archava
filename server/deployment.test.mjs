import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createReadinessCheck } from "./readiness.mjs";
import { createApp } from "./app.mjs";
import { createDependencies } from "./dependencies.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));

test("readiness follows the worker health response and hides private details", async t => {
  let status = 503;
  const worker = createServer((_req, res) => { res.writeHead(status); res.end("internal worker detail"); });
  await new Promise(resolve => worker.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => worker.close(resolve)));
  const check = createReadinessCheck({ workerHealthUrl: `http://127.0.0.1:${worker.address().port}/` });
  assert.deepEqual(await check(), { ok: false, checks: { api: "ok", worker: "unavailable" } });
  status = 200;
  assert.deepEqual(await check(), { ok: true, checks: { api: "ok", worker: "ok" } });
});

test("readiness refuses remote/credentialed probes and handles connection failure", async () => {
  for (const workerHealthUrl of ["https://example.com/", "http://example.com/", "http://secret@localhost/"]) {
    assert.throws(() => createReadinessCheck({ workerHealthUrl }), /credential-free loopback/);
  }
  const check = createReadinessCheck({ workerHealthUrl: "http://127.0.0.1:8081/", fetcher: async () => { throw new Error("private provider detail"); } });
  assert.deepEqual(await check(), { ok: false, checks: { api: "ok", worker: "unavailable" } });
  assert.deepEqual(await createReadinessCheck()(), { ok: true, checks: { api: "ok", worker: "not_configured" } });
});

test("readiness answers 503 until the worker is healthy, independently of request quota", async t => {
  let healthy = false;
  const cwd = await mkdtemp(join(tmpdir(), "archava-ready-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const deps = createDependencies({ env: { AVATAR_PROVIDER: "tavus" }, cwd });
  deps.checkReadiness = async () => ({ ok: healthy, checks: { api: "ok", worker: healthy ? "ok" : "unavailable" } });
  const server = createServer(createApp(deps).handle);
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const origin = `http://127.0.0.1:${server.address().port}`;
  for (let i = 0; i < 35; i++) await fetch(origin + "/api/config");
  const blocked = await fetch(origin + "/api/ready");
  assert.equal(blocked.status, 503);
  assert.equal((await blocked.json()).checks.worker, "unavailable");
  healthy = true;
  assert.equal((await fetch(origin + "/api/ready")).status, 200);
  assert.equal((await fetch(origin + "/api/health")).status, 200);
});

test("an explicit state directory retains API key records across a restart", async t => {
  const cwd = await mkdtemp(join(tmpdir(), "archava-state-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const env = { AVATAR_PROVIDER: "tavus", ARCHAVA_STATE_DIR: "persistent" };
  const first = createDependencies({ env, cwd });
  const wallet = "0x1111111111111111111111111111111111111111";
  const cookie = first.dashboardSessions.issue(wallet).cookie;
  const key = (await first.developerApi.handle({ method: "POST", path: "/api/developer/keys", cookie })).body;
  const second = createDependencies({ env, cwd });
  const after = await second.developerApi.handle({ method: "GET", path: "/api/developer/keys", cookie: second.dashboardSessions.issue(wallet).cookie });
  assert.equal(after.body.keys[0].id, key.id);
  const raw = await readFile(join(cwd, "persistent/keys.json"), "utf8");
  assert.ok(!raw.includes(key.apiKey));
});

async function supervisor(t, exitCode) {
  const cwd = await mkdtemp(join(tmpdir(), "archava-supervisor-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const common = '#!/bin/bash\ntrap \'echo stopped >> "$ARCHAVA_TEST_STOPPED"; exit 0\' TERM INT\n';
  await writeFile(join(cwd, "node"), common + 'echo ready\nwhile true; do /bin/sleep 0.1; done\n', { mode: 0o755 });
  await writeFile(join(cwd, "python3"), common + (exitCode === null
    ? 'while true; do /bin/sleep 0.1; done\n'
    : `/bin/sleep 0.2\nexit ${exitCode}\n`), { mode: 0o755 });
  const stopped = join(cwd, "stopped");
  const child = spawn("/bin/bash", [join(root, "entrypoint.sh")], {
    cwd, env: { PATH: cwd + ":/usr/bin:/bin", ARCHAVA_TEST_STOPPED: stopped },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const closed = new Promise(resolve => child.once("close", (code, signal) => resolve({ code, signal })));
  const timeout = setTimeout(() => child.kill("SIGKILL"), 5000);
  t.after(() => clearTimeout(timeout));
  if (exitCode === null) {
    await new Promise(resolve => child.stdout.on("data", chunk => { if (chunk.includes("ready")) resolve(); }));
    child.kill("SIGTERM");
  }
  const result = await closed;
  assert.equal(result.signal, null, "supervisor failed to exit before timeout");
  assert.ok((await readFile(stopped, "utf8")).includes("stopped"), "surviving child was not stopped");
  return result.code;
}

test("a failing worker stops its peer and preserves the restart exit code", { timeout: 10000 }, async t => {
  assert.equal(await supervisor(t, 7), 7);
});
test("an unexpectedly clean worker exit still causes a restart", { timeout: 10000 }, async t => {
  assert.equal(await supervisor(t, 0), 1);
});
test("a planned SIGTERM stops both children without reporting a crash", { timeout: 10000 }, async t => {
  assert.equal(await supervisor(t, null), 0);
});
