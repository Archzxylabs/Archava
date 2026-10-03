import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("public preflight uses GET only and fails when worker health is unavailable", async t => {
  let healthy = true;
  const methods = [];
  const server = createServer((req, res) => {
    methods.push(req.method);
    const url = new URL(req.url, "http://localhost");
    if (["/", "/build"].includes(url.pathname)) {
      res.setHeader("content-type", "text/html");
      return res.end('<title>Archava</title><script src="/assets/index-test.js"></script>');
    }
    res.setHeader("content-type", "application/json");
    const body = url.pathname === "/api/config"
      ? { livekitReady: true, avatarProvider: "spatius", spatiusAppId: "fixture-app", spatiusAvatarId: "fixture-avatar",
        contractAddress: "fixture-contract", chainId: 97, packMinutes: [60, 300], previewEnabled: true, previewSeconds: 120 }
      : url.pathname === "/api/quote"
        ? { minutes: Number(url.searchParams.get("minutes")), symbol: "mUSDT", tokenAmount: "5" }
        : url.pathname === "/api/ready"
          ? { ok: healthy, checks: { worker: healthy ? "ok" : "unavailable" } }
          : { ok: true };
    if (url.pathname === "/api/ready" && !healthy) res.statusCode = 503;
    res.end(JSON.stringify(body));
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const run = () => new Promise(resolve => {
    const child = spawn(process.execPath, [fileURLToPath(new URL("../scripts/readonly-preflight.mjs", import.meta.url)), "--origin", origin],
      { env: {}, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", chunk => { output += chunk; });
    child.stderr.on("data", chunk => { output += chunk; });
    child.once("close", code => resolve({ code, output }));
  });
  const passing = await run();
  assert.equal(passing.code, 0);
  assert.match(passing.output, /READ ONLY; NO ROOMS OR TRANSACTIONS/);
  assert.equal(methods.length, 7);
  assert.ok(methods.every(method => method === "GET"));
  healthy = false;
  const failing = await run();
  assert.equal(failing.code, 1);
  assert.match(failing.output, /FAIL public preflight/);
});
