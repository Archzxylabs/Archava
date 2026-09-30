import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

/**
 * The real process bootstrap, started as a child and watched across a sweep.
 *
 * `server/index.mjs` listens on import, so it can never be imported here; it is
 * executed instead. That is the point of this file: every other test imports
 * `app.mjs`, and the two Cycle 6 bootstrap defects (an `existsSync` import from
 * the wrong module, and a sweep calling a collaborator `createDependencies`
 * never returned) were both invisible to a suite that never ran the real entry
 * point.
 *
 * Nothing here touches the repository `.env` or any provider. The child runs in
 * a throwaway cwd with a hand-written `.env` of its own, receives no inherited
 * provider credentials, and has LiveKit unconfigured — so `rooms` and
 * `roomService` are both null and the sweep's room close short-circuits without
 * a network call. A provider room is therefore not merely not-opened; it cannot
 * be opened.
 */

const REPO_ROOT = resolve(fileURLToPath(import.meta.url), "..", "..");
const ENTRY = join(REPO_ROOT, "server", "index.mjs");
const SWEEP_INTERVAL_MS = 10_000;

/** The value the repository `.env` actually uses, so a leak is detectable. */
const REPO_PREVIEW_SECONDS = 120;
/** A sentinel only the throwaway cwd's `.env` can supply. */
const CHILD_PREVIEW_SECONDS = 150;

async function freePort() {
  const server = createServer();
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address();
  await new Promise((done) => server.close(done));
  return port;
}

/**
 * Runs the real entry point and collects what it printed.
 *
 * The environment is built from scratch rather than inherited: no LiveKit, no
 * Spatius, no contract, no `GEMINI_*`/`TAVUS_*` from the surrounding shell.
 * `PATH` is kept so `node` stays usable, `HOME` is redirected so nothing in the
 * real home directory can satisfy the run, and every provider variable is
 * present as an empty string so a value inherited by the test runner cannot
 * change the outcome.
 */
function runBootstrap(cwd, port, extraEnv = {}) {
  const child = spawn(process.execPath, [ENTRY], {
    cwd,
    env: {
      PATH: process.env.PATH || "/usr/bin:/bin",
      HOME: cwd,
      PORT: String(port),
      WEB_ORIGIN: "http://127.0.0.1:5174",
      CHAIN_ID: "97",
      AVATAR_PROVIDER: "tavus",
      LIVEKIT_URL: "",
      LIVEKIT_API_KEY: "",
      LIVEKIT_API_SECRET: "",
      RENTAL_CONTRACT: "",
      SPATIUS_API_KEY: "",
      SPATIUS_APP_ID: "",
      SPATIUS_AVATAR_ID: "",
      ENABLE_PREVIEW: "",
      ...extraEnv,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  return { child, readStdout: () => stdout, readStderr: () => stderr };
}

/** Waits until the bootstrap says it is listening, or fails with what it printed. */
async function waitForListen(server, port) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (server.child.exitCode !== null) {
      throw new Error("bootstrap exited with code " + server.child.exitCode
        + "\nstdout: " + server.readStdout() + "\nstderr: " + server.readStderr());
    }
    if (server.readStdout().includes("listening on :" + port)) return;
    await new Promise((done) => setTimeout(done, 50));
  }
  throw new Error("bootstrap never announced a listening port.\nstdout: " + server.readStdout()
    + "\nstderr: " + server.readStderr());
}

async function stop(server) {
  const { child } = server;
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  await new Promise((done) => {
    if (child.exitCode !== null) return done();
    child.on("exit", done);
    setTimeout(() => { child.kill("SIGKILL"); }, 5_000).unref();
  });
}

test("the real bootstrap boots with a blank provider config and lives through a sweep", { timeout: 90_000 }, async (t) => {
  const cwd = await mkdtemp(join(process.cwd(), "archava-bootstrap-"));
  const port = await freePort();
  // A `.env` belonging to the throwaway cwd, not to the repository. Its only job
  // is to be observable: if the process ever read the repository's `.env`
  // instead, the published preview budget would come back as that file's value
  // and the assertion below fails.
  await writeFile(join(cwd, ".env"), "PREVIEW_SECONDS=" + CHILD_PREVIEW_SECONDS + "\n", "utf8");
  // The bootstrap does not serve the SPA without a dist directory, and creating
  // one keeps the static fallthrough from logging on every request.
  await mkdir(join(cwd, "dist"), { recursive: true });

  const server = runBootstrap(cwd, port);
  t.after(async () => {
    await stop(server);
    await rm(cwd, { recursive: true, force: true });
  });
  await waitForListen(server, port);

  const config = await (await fetch("http://127.0.0.1:" + port + "/api/config")).json();
  assert.equal(config.previewSeconds, CHILD_PREVIEW_SECONDS,
    "the child must read only its own cwd's .env (the repository .env says " + REPO_PREVIEW_SECONDS + ")");
  assert.equal(config.avatarProvider, "tavus");
  assert.equal(config.livekitReady, false, "LiveKit is unconfigured, so no provider client exists");
  assert.equal(config.previewEnabled, false, "no preview can open without a provider");

  // `SWEEP_INTERVAL_MS` is 10s, so waiting past it runs the tick that used to
  // throw on `dependencies.customerSessions.sweep()`. The bootstrap logs a sweep
  // failure to stderr, so an empty stderr is what proves the tick completed
  // against the collaborators `createDependencies` actually returns.
  await new Promise((done) => setTimeout(done, SWEEP_INTERVAL_MS + 2_000));

  assert.equal(server.readStderr(), "",
    "the sweep tick must not report a failed collaborator:\n" + server.readStderr());
  assert.equal(server.child.exitCode, null, "the process must still be running after the sweep");

  const afterSweep = await fetch("http://127.0.0.1:" + port + "/api/config");
  assert.equal(afterSweep.status, 200, "the handler must still answer requests after a sweep");
  assert.equal((await afterSweep.json()).previewSeconds, CHILD_PREVIEW_SECONDS);
});

test("the real bootstrap refuses an uncredentialed provider instead of starting", async () => {
  const cwd = await mkdtemp(join(process.cwd(), "archava-bootstrap-bad-"));
  const port = await freePort();
  const server = runBootstrap(cwd, port, { AVATAR_PROVIDER: "spatius" });
  const code = await new Promise((done) => {
    server.child.on("exit", done);
    setTimeout(() => { if (server.child.exitCode === null) server.child.kill("SIGKILL"); }, 20_000).unref();
  });
  assert.notEqual(code, 0, "a provider with no credentials must not boot");
  assert.match(server.readStderr(), /Spatius requires SPATIUS_API_KEY/);
  await rm(cwd, { recursive: true, force: true });
});
