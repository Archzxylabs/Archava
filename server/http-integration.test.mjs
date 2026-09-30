import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { Wallet } from "ethers";
import { createApp } from "./app.mjs";
import { createChallengeStore } from "./auth.mjs";
import { createKeyStore } from "./apikeys.mjs";
import { createUsageStore } from "./usage.mjs";
import { createEntitlementGate } from "./entitlement.mjs";
import { createCustomerSessions } from "./customer-sessions.mjs";
import { createCustomerApi } from "./customer-api.mjs";
import { createDeveloperApi } from "./developer-api.mjs";
import { createDashboardSessionStore } from "./developer-auth.mjs";

const ORIGIN = "http://localhost:5174";

async function harness(run) {
  const dir = await mkdtemp(join(process.cwd(), ".minute-http-"));
  const wallet = Wallet.createRandom();
  const clock = { ms: 1_800_000_000_000 };
  const now = () => clock.ms;
  let purchasedMinutes = 0;
  const opened = new Set();
  const rooms = {
    async openRoom({ roomName }) { opened.add(roomName); },
    async issueToken({ roomName }) { return `jwt:${roomName}`; },
    async deleteRoom(roomName) { opened.delete(roomName); },
  };
  const keys = createKeyStore({ path: join(dir, "keys.json"), now });
  const usage = createUsageStore({ path: join(dir, "usage.json"), now });
  const readCredits = async () => ({ purchasedMinutes });
  const gate = createEntitlementGate({ readCredits, usage });
  const sessions = createCustomerSessions({ gate, keys, usage, livekit: rooms, now,
    sessionFields: { serverUrl: "wss://test.livekit.cloud", avatarProvider: "spatius", spatiusAppId: "app", spatiusAvatarId: "avatar" } });
  const challenges = createChallengeStore({ origin: ORIGIN, now });
  const dashboardChallenges = createChallengeStore({ origin: ORIGIN, purpose: "Archava developer dashboard access", now });
  const dashboardSessions = createDashboardSessionStore({ now, secure: false });
  const dashboard = { resolve(cookie) { const address = dashboardSessions.resolve(cookie); return address ? { wallet: address } : null; } };
  const app = createApp({
    now, origin: ORIGIN, distDir: join(dir, "dist"), chainId: 97,
    contractAddress: wallet.address, livekitUrl: "wss://test.livekit.cloud",
    avatarProvider: "spatius", spatiusAppId: "app", spatiusAvatarId: "avatar",
    previewSeconds: 120, packMinutes: [60, 300], readCredits,
    quotePack: async (minutes) => ({ minutes, wei: String(minutes * 1000), tokenAmount: String(minutes / 60_000), symbol: "mUSDT" }),
    usage, customerSessions: sessions, rooms, challenges, dashboardChallenges,
    dashboardSessions, sessionTickets: new Map(),
    previewController: { available: () => false, handle: async () => ({ status: 403, body: {} }) },
    developerApi: createDeveloperApi({ dashboard, keys, readCredits, usage }),
    customerApi: createCustomerApi({ gate, keys, usage, sessions, readCredits }),
    rateLimitKey: () => "test",
  });
  const server = createServer(app.handle);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  async function call(method, path, body, headers = {}) {
    const response = await fetch(`http://127.0.0.1:${port}${path}`, {
      method, headers: { origin: ORIGIN, ...(body === undefined ? {} : { "content-type": "application/json" }), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    return { status: response.status, body: await response.json(), headers: response.headers };
  }
  async function browserStart() {
    const challenge = await call("POST", "/api/challenge", { wallet: wallet.address });
    const signature = await wallet.signMessage(challenge.body.message);
    return call("POST", "/api/session", { wallet: wallet.address, nonce: challenge.body.nonce, signature });
  }
  async function issueKey() {
    const challenge = await call("POST", "/api/developer/challenge", { wallet: wallet.address });
    const signature = await wallet.signMessage(challenge.body.message);
    const loggedIn = await call("POST", "/api/developer/login", { wallet: wallet.address, nonce: challenge.body.nonce, signature });
    const cookie = loggedIn.headers.get("set-cookie").split(";")[0];
    const issued = await call("POST", "/api/developer/keys", { label: "integration" }, { cookie });
    assert.equal(issued.status, 201);
    return { ...issued.body, cookie };
  }
  try {
    return await run({ wallet, clock, call, browserStart, issueKey, opened,
      buy(minutes) { purchasedMinutes += minutes; } });
  } finally {
    await new Promise((resolve) => server.close(resolve));
    await rm(dir, { recursive: true, force: true });
  }
}

test("quotes accept only 60 and 300 minutes and balance starts at zero", async () => {
  await harness(async (h) => {
    assert.deepEqual((await h.call("GET", "/api/config")).body.packMinutes, [60, 300]);
    assert.equal((await h.call("GET", "/api/quote?minutes=60")).body.minutes, 60);
    assert.equal((await h.call("GET", "/api/quote?minutes=300")).body.minutes, 300);
    assert.equal((await h.call("GET", "/api/quote?minutes=7")).status, 400);
    assert.equal((await h.call("GET", `/api/access?wallet=${h.wallet.address}`)).body.remainingSeconds, 0);
    assert.equal((await h.browserStart()).status, 403);
    assert.equal(h.opened.size, 0);
  });
});

test("wallet signature, API key, and one shared minute balance work over HTTP", async () => {
  await harness(async (h) => {
    h.buy(60);
    const first = await h.browserStart();
    assert.equal(first.status, 200);
    assert.equal(first.body.preview, false);
    assert.equal((await h.call("GET", `/api/access?wallet=${h.wallet.address}`)).body.remainingSeconds, 1800);
    const key = await h.issueKey();
    const bearer = { authorization: `Bearer ${key.apiKey}` };
    assert.equal((await h.call("POST", "/v1/sessions", { avatar: "ava" }, bearer)).status, 409);
    h.clock.ms += 75_000;
    assert.equal((await h.call("POST", "/api/session/end", { ticket: first.body.ticket })).body.seconds, 75);
    const second = await h.call("POST", "/v1/sessions", { avatar: "ava" }, bearer);
    assert.equal(second.status, 201);
    assert.equal(second.body.avatar.provider, "spatius");
    h.clock.ms += 10_000;
    const ended = await h.call("POST", `/v1/sessions/${second.body.sessionId}/end`, {}, bearer);
    assert.equal(ended.body.seconds, 10);
    const usage = await h.call("GET", "/v1/usage", undefined, bearer);
    assert.equal(usage.body.usedSeconds, 85);
    assert.equal(usage.body.remainingSeconds, 3515);
    assert.equal(usage.body.sessionCount, 2);
    h.buy(300);
    assert.equal((await h.call("GET", `/api/access?wallet=${h.wallet.address}`)).body.remainingSeconds, 21_515);
  });
});

test("an invalid key cannot spend minutes or choose another avatar", async () => {
  await harness(async (h) => {
    h.buy(60);
    assert.equal((await h.call("POST", "/v1/sessions", {}, { authorization: "Bearer bad" })).status, 401);
    const key = await h.issueKey();
    const bearer = { authorization: `Bearer ${key.apiKey}` };
    assert.equal((await h.call("POST", "/v1/sessions", { avatar: "other" }, bearer)).status, 400);
    assert.equal(h.opened.size, 0);
    assert.equal((await h.call("GET", `/api/access?wallet=${h.wallet.address}`)).body.remainingSeconds, 3600);
  });
});
