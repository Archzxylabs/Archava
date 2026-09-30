import assert from "node:assert/strict";
import test from "node:test";
import { createPreviewController } from "./preview.mjs";

function makeController(overrides = {}) {
  return createPreviewController({
    enabled: true,
    seconds: 90,
    now: () => 1_000_000,
    openRoom: async () => ({ roomName: "room-1", token: "guest-jwt" }),
    newTicket: () => "ticket-hex",
    sessionFields: { serverUrl: "wss://demo.livekit.cloud", avatarProvider: "tavus" },
    onSession: () => {},
    ...overrides,
  });
}

test("a preview needs no cookie, visitor identity, or proxy IP header", async () => {
  const tickets = [];
  const c = makeController({ onSession: (ticket, info) => tickets.push([ticket, info]) });
  const result = await c.handle();
  assert.equal(result.status, 200);
  assert.equal(result.body.preview, true);
  assert.equal(tickets.length, 1);
});

test("available() reports only whether the preview is switched on", () => {
  assert.equal(makeController().available(), true);
  assert.equal(makeController({ enabled: false }).available(), false);
});

test("the disabled preview is the only refusal the controller can produce", () => {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    assert.equal(makeController().begin().ok, true, "attempt " + attempt + " must be allowed");
  }
  const disabled = makeController({ enabled: false }).begin();
  assert.equal(disabled.ok, false);
  assert.equal(disabled.status, 403);
  assert.equal(disabled.error, "Preview is disabled");
});

test("a disabled preview is refused before any room work", async () => {
  let opened = 0;
  const c = makeController({ enabled: false, openRoom: async () => { opened += 1; return { roomName: "r", token: "t" }; } });
  const result = await c.handle();
  assert.equal(result.status, 403);
  assert.deepEqual(result.body, { error: "Preview is disabled" });
  assert.equal(opened, 0);
});

test("the same visitor starts another preview immediately after one ends", async () => {
  let ticketNo = 0;
  const tickets = [];
  const c = makeController({
    newTicket: () => "ticket-" + (++ticketNo),
    onSession: (ticket, info) => tickets.push([ticket, info]),
  });
  // The clock never advances, so the second call lands in the window the removed
  // per-browser cooldown used to refuse.
  const first = await c.handle("visitor-a");
  const second = await c.handle("visitor-a");
  assert.equal(first.status, 200);
  assert.equal(second.status, 200);
  assert.deepEqual(
    tickets.map(([ticket]) => ticket),
    ["ticket-1", "ticket-2"],
  );
});

test("no visitor-count ceiling blocks preview after preview", async () => {
  const c = makeController();
  for (let visitor = 0; visitor < 5; visitor += 1) {
    assert.equal((await c.handle("visitor-" + visitor)).status, 200);
  }
});

test("overlapping openings are allowed while a preview is still running", async () => {
  let inFlight = 0;
  let peak = 0;
  const c = makeController({
    openRoom: async () => {
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      await new Promise((resolve) => setTimeout(resolve, 5));
      inFlight -= 1;
      return { roomName: "room-1", token: "guest-jwt" };
    },
  });
  const [first, second] = await Promise.all([c.handle("visitor-a"), c.handle("visitor-b")]);
  assert.equal(first.status, 200);
  assert.equal(second.status, 200);
  assert.equal(peak, 2, "both rooms must be opened at the same time");
});

test("a granted preview returns the existing AvatarSession shape with preview true", async () => {
  const tickets = [];
  const c = makeController({
    sessionFields: { serverUrl: "wss://demo.livekit.cloud", avatarProvider: "tavus" },
    newTicket: () => "ticket-xyz",
    onSession: (ticket, info) => tickets.push([ticket, info]),
  });
  const result = await c.handle();
  assert.equal(result.status, 200);
  assert.deepEqual(result.body, {
    serverUrl: "wss://demo.livekit.cloud",
    token: "guest-jwt",
    endsAt: 1000 + 90,
    preview: true,
    ticket: "ticket-xyz",
    avatarProvider: "tavus",
  });
  assert.deepEqual(tickets, [["ticket-xyz", { roomName: "room-1", endsAt: 1090, preview: true }]]);
});

test("spatius previews carry the public provider identifiers", async () => {
  const c = makeController({
    sessionFields: {
      serverUrl: "wss://demo.livekit.cloud",
      avatarProvider: "spatius",
      spatiusAppId: "app-id",
      spatiusAvatarId: "avatar-id",
    },
  });
  const { body } = await c.handle();
  assert.equal(body.avatarProvider, "spatius");
  assert.equal(body.spatiusAppId, "app-id");
  assert.equal(body.spatiusAvatarId, "avatar-id");
  assert.equal(body.preview, true);
});

test("the preview response never leaks LiveKit or provider secrets", async () => {
  const c = makeController({
    openRoom: async () => ({ roomName: "room-1", token: "guest-jwt", secret: "lk_secret", apiKey: "APIkey" }),
  });
  const { body } = await c.handle();
  assert.equal("secret" in body, false);
  assert.equal("apiKey" in body, false);
  const serialized = JSON.stringify(body);
  assert.equal(serialized.includes("lk_secret"), false);
  assert.equal(serialized.includes("APIkey"), false);
});

test("a failed room open leaves no state behind for the next preview", async () => {
  let fail = true;
  const tickets = [];
  const c = makeController({
    openRoom: async () => {
      if (fail) throw new Error("livekit down");
      return { roomName: "room-1", token: "guest-jwt" };
    },
    newTicket: () => "ticket-hex",
    onSession: (ticket, info) => tickets.push([ticket, info]),
  });
  await assert.rejects(() => c.handle("visitor-a"), /livekit down/);
  // A failed open registers nothing, so it cannot consume the visitor either.
  assert.deepEqual(tickets, []);
  fail = false;
  assert.equal((await c.handle("visitor-a")).status, 200);
  assert.deepEqual(tickets, [["ticket-hex", { roomName: "room-1", endsAt: 1090, preview: true }]]);
});
