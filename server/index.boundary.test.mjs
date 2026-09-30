import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { createApp } from "./app.mjs";
import { createPreviewController } from "./preview.mjs";

/**
 * The route boundaries of the HTTP application, over a real socket.
 *
 * `server/index.mjs` is a listen-on-import bootstrap, so it is not importable;
 * `server/app.mjs` holds the same handler with everything injectable, and is
 * what runs here. Nothing below reads source text: each assertion drives the
 * handler over a socket with fakes in the collaborator slots, and checks what a
 * client actually receives.
 */

const ORIGIN = "http://localhost:5174";
const ALICE = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";

/** A LiveKit stand-in that also answers listRooms, which the paid route reads. */
function fakeRooms({ token = "paid-jwt" } = {}) {
  const opened = [];
  const deleted = [];
  const rooms = [];
  return {
    opened,
    deleted,
    rooms,
    async listRooms() { return rooms; },
    async openRoom(room) { opened.push(room); rooms.push({ metadata: JSON.stringify({ product: "archava", wallet: room.wallet, endsAt: room.endsAt }) }); },
    async issueToken({ roomName }) { return token + ":" + roomName; },
    async deleteRoom(roomName) { deleted.push(roomName); },
  };
}

/** A challenge store that accepts one wallet signature and nothing else. */
function fakeChallenges() {
  const issued = new Map();
  return {
    issued,
    issue(wallet) {
      const nonce = "n-" + Math.random();
      issued.set(nonce, wallet);
      return { wallet, nonce };
    },
    consume({ nonce }) { return issued.delete(nonce); },
  };
}

function fakeSessionTickets() {
  const tickets = new Map();
  return {
    tickets,
    get: (ticket) => tickets.get(ticket),
    set: (ticket, value) => tickets.set(ticket, value),
    delete: (ticket) => tickets.delete(ticket),
  };
}

function appOverSocket(handler) {
  return new Promise((resolve) => {
    const server = createServer(handler);
    server.listen(0, "127.0.0.1", () => resolve({ server, port: server.address().port }));
  });
}

/**
 * One request over a real socket. Returns `{ status, body, headers }` with the
 * body already parsed when it is JSON.
 */
async function call(port, method, path, { body, headers = {} } = {}) {
  const res = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers: {
      origin: ORIGIN,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
      ...headers,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    redirect: "manual",
  });
  const text = await res.text();
  let parsed;
  try { parsed = JSON.parse(text); } catch { parsed = undefined; }
  return { status: res.status, body: parsed ?? text, headers: res.headers };
}

async function withApp(deps, run) {
  const app = createApp({
    origin: ORIGIN,
    distDir: "/nonexistent-dist",
    chainId: 97,
    contractAddress: "0xrental",
    livekitUrl: "wss://archava-test.livekit.cloud",
    avatarProvider: "spatius",
    spatiusAppId: "test-app-id",
    spatiusAvatarId: "test-avatar-id",
    previewSeconds: 90,
    packMinutes: [60, 300],
    agentName: "archava-host",
    previewController: { available: () => true, handle: async () => ({ status: 200, body: { preview: true } }) },
    challenges: fakeChallenges(),
    dashboardChallenges: fakeChallenges(),
    dashboardSessions: { issue: () => ({ cookie: "archava_session=abc; Path=/api/developer; HttpOnly" }), destroy: () => "archava_session=; Path=/api/developer; Max-Age=0", resolve: () => null },
    developerApi: { handle: async () => ({ status: 200, body: { developer: true } }) },
    customerApi: { handle: async () => ({ status: 200, body: { customer: true } }) },
    customerSessions: { endTicket: async () => ({ ok: true, seconds: 0 }) },
    sessionTickets: fakeSessionTickets(),
    rateLimitKey: () => "test",
    ...deps,
  });
  const { server, port } = await appOverSocket(app.handle);
  try {
    return await run(call.bind(null, port));
  } finally {
    await new Promise((done) => server.close(done));
  }
}

test("POST /api/preview needs no wallet, signature, or rental", async () => {
  await withApp({ rooms: fakeRooms() }, async (call) => {
    const res = await call("POST", "/api/preview", { body: {} });
    assert.equal(res.status, 200);
    assert.deepEqual(res.body, { preview: true });
  });
});

test("a preview ticket closes the room opened for that preview", async () => {
  const rooms = fakeRooms();
  const tickets = fakeSessionTickets();
  const previewController = createPreviewController({
    enabled: true,
    now: () => 1_800_000_000_000,
    openRoom: async () => ({ roomName: "archava-preview", token: "preview-jwt" }),
    newTicket: () => "preview-ticket",
    sessionFields: { serverUrl: "wss://archava-test.livekit.cloud", avatarProvider: "spatius" },
    onSession: (ticket, session) => tickets.set(ticket, session),
  });
  await withApp({ rooms, sessionTickets: tickets, previewController }, async (call) => {
    const opened = await call("POST", "/api/preview", { body: {} });
    assert.equal(opened.status, 200);
    assert.equal(opened.body.ticket, "preview-ticket");
    const closed = await call("POST", "/api/session/end", { body: { ticket: opened.body.ticket } });
    assert.deepEqual(closed.body, { ok: true });
    assert.deepEqual(rooms.deleted, ["archava-preview"]);
    assert.equal(tickets.get("preview-ticket"), undefined);
  });
});

test("/api/config advertises the preview budget and the provider identifiers", async () => {
  await withApp({}, async (call) => {
    const res = await call("GET", "/api/config");
    assert.equal(res.status, 200);
    assert.equal(res.body.previewEnabled, true);
    assert.equal(res.body.previewSeconds, 90);
    assert.equal(res.body.avatarProvider, "spatius");
    assert.equal(res.body.spatiusAppId, "test-app-id");
    assert.equal(res.body.spatiusAvatarId, "test-avatar-id");
  });
});

test("the anonymous preview is exempt from the generic API rate limiter", async () => {
  await withApp({ rooms: fakeRooms(), previewController: { available: () => true, handle: async () => ({ status: 200, body: { n: 1 } }) } }, async (call) => {
    // Exhaust the 30/60s bucket on a metered route first.
    let refused = null;
    for (let i = 0; i < 40; i += 1) {
      const res = await call("GET", "/api/config");
      if (res.status === 429 && refused === null) refused = i;
    }
    assert.notEqual(refused, null, "a metered route must be capped at all");
    // The preview must still answer with the bucket already full.
    for (let i = 0; i < 5; i += 1) {
      const res = await call("POST", "/api/preview", { body: {} });
      assert.equal(res.status, 200, "preview refused after " + i + " preview calls");
    }
  });
});

test("a path under a metered prefix is answered with JSON, never the SPA shell", async () => {
  await withApp({
    developerApi: { handle: async () => null },
    customerApi: { handle: async () => null },
  }, async (call) => {
    const dev = await call("GET", "/api/developer/nope");
    assert.equal(dev.status, 404);
    assert.deepEqual(dev.body, { error: "Not found" });
    assert.equal(dev.headers.get("content-type"), "application/json; charset=utf-8");

    const v1 = await call("GET", "/v1/nope");
    assert.equal(v1.status, 404);
    assert.deepEqual(v1.body, { error: "Not found" });
  });
});

test("the developer login routes own the session cookie, and the API defers to them", async () => {
  await withApp({
    // The dashboard API is the real thing, minus the login routes.
    dashboardSessions: {
      issue: () => ({ cookie: "archava_session=abc; Path=/api/developer; HttpOnly; SameSite=Lax" }),
      destroy: () => "archava_session=; Path=/api/developer; Max-Age=0",
      resolve: () => ALICE,
    },
    dashboardChallenges: fakeChallenges(),
  }, async (call) => {
    const issued = await call("POST", "/api/developer/challenge", { body: { wallet: ALICE } });
    assert.equal(issued.status, 200);

    const login = await call("POST", "/api/developer/login", { body: { wallet: ALICE, nonce: issued.body.nonce } });
    assert.equal(login.status, 200);
    assert.equal(login.body.wallet, ALICE);
    // The cookie is HttpOnly and scoped to the API, so script cannot read it.
    const cookie = login.headers.get("set-cookie");
    assert.match(cookie, /HttpOnly/);
    assert.match(cookie, /Path=\/api\/developer/);

    const logout = await call("POST", "/api/developer/logout", { body: {} });
    assert.equal(logout.status, 200);
    assert.match(logout.headers.get("set-cookie"), /Max-Age=0/);
  });
});

test("a cross-origin API request is refused", async () => {
  const app = createApp({
    origin: ORIGIN,
    distDir: "/nonexistent-dist",
    chainId: 97,
    contractAddress: "0xrental",
    livekitUrl: "",
    avatarProvider: "tavus",
    previewSeconds: 90,
    packMinutes: [60, 300],
    previewController: { available: () => false, handle: async () => ({ status: 503, body: {} }) },
    challenges: fakeChallenges(),
    dashboardChallenges: fakeChallenges(),
    dashboardSessions: { issue: () => ({ cookie: "" }), destroy: () => "", resolve: () => null },
    developerApi: { handle: async () => ({ status: 200, body: {} }) },
    customerApi: { handle: async () => ({ status: 200, body: {} }) },
    customerSessions: { endTicket: async () => ({ ok: true, seconds: 0 }) },
    sessionTickets: fakeSessionTickets(),
  });
  const { server, port } = await appOverSocket(app.handle);
  try {
    const res = await fetch(`http://127.0.0.1:${port}/api/config`, { headers: { origin: "http://evil.example" } });
    assert.equal(res.status, 403);
    assert.deepEqual(await res.json(), { error: "Origin not allowed" });
  } finally {
    await new Promise((done) => server.close(done));
  }
});
