import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { createUsageStore } from "./usage.mjs";
import { createEntitlementGate } from "./entitlement.mjs";
import { createCustomerSessions } from "./customer-sessions.mjs";

const WALLET = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
const KEY = { id: "key_one", wallet: WALLET };

async function harness() {
  const dir = await mkdtemp(join(process.cwd(), ".minute-test-"));
  const file = join(dir, "usage.json");
  const clock = { ms: 1_800_000_000_000 };
  const now = () => clock.ms;
  let purchasedMinutes = 0;
  let providerFails = false;
  const rooms = new Set();
  const livekit = {
    async openRoom({ roomName }) { rooms.add(roomName); },
    async issueToken({ roomName }) { return `jwt:${roomName}`; },
    async deleteRoom(roomName) {
      if (providerFails) throw new Error("provider down");
      rooms.delete(roomName);
    },
  };
  const usage = createUsageStore({ path: file, now });
  const readCredits = async () => ({ purchasedMinutes });
  const gate = createEntitlementGate({ readCredits, usage });
  const sessions = createCustomerSessions({
    gate, usage, livekit, now,
    keys: { findBySecret: async (secret) => secret === "valid-key" ? KEY : null },
    sessionFields: { serverUrl: "wss://test.livekit.cloud", avatarProvider: "spatius" },
  });
  return {
    usage, sessions, clock, rooms, file,
    buy(minutes) { purchasedMinutes += minutes; },
    providerFails(value) { providerFails = value; },
    async dispose() { await rm(dir, { recursive: true, force: true }); },
  };
}

test("website and API key share purchased minutes, with unused reservations returned", async () => {
  const h = await harness();
  try {
    assert.equal((await h.sessions.startWallet({ wallet: WALLET })).status, 403);
    h.buy(60);
    const browser = await h.sessions.startWallet({ wallet: WALLET });
    assert.equal(browser.ok, true);
    assert.equal(browser.endsAt, Math.floor(h.clock.ms / 1000) + 1800);
    assert.equal((await h.usage.balance(WALLET, 60)).remainingSeconds, 1800);
    assert.equal((await h.sessions.start({ apiKey: "valid-key" })).status, 409);
    h.clock.ms += 75_000;
    assert.equal((await h.sessions.endTicket(browser.ticket)).seconds, 75);
    assert.equal((await h.usage.balance(WALLET, 60)).remainingSeconds, 3525);
    const api = await h.sessions.start({ apiKey: "valid-key" });
    assert.equal(api.ok, true);
    h.clock.ms += 10_000;
    assert.equal((await h.sessions.end({ sessionId: api.sessionId, key: KEY })).seconds, 10);
    assert.equal((await h.usage.balance(WALLET, 60)).remainingSeconds, 3515);
    h.buy(300);
    assert.equal((await h.usage.balance(WALLET, 360)).remainingSeconds, 21_515);
    assert.equal(h.rooms.size, 0);
  } finally { await h.dispose(); }
});

test("an active reservation survives restart, prevents double spending, and sweeps at the cap", async () => {
  const h = await harness();
  try {
    h.buy(1);
    const started = await h.sessions.startWallet({ wallet: WALLET });
    assert.equal(started.ok, true);
    assert.equal((await h.usage.balance(WALLET, 1)).remainingSeconds, 0);
    const restarted = createUsageStore({ path: h.file, now: () => h.clock.ms });
    assert.equal((await restarted.balance(WALLET, 1)).remainingSeconds, 0);
    h.clock.ms += 60_000;
    const closed = await h.sessions.sweep();
    assert.equal(closed[0].seconds, 60);
    assert.equal((await restarted.balance(WALLET, 1)).remainingSeconds, 0);
    assert.equal((await h.sessions.startWallet({ wallet: WALLET })).status, 403);
  } finally { await h.dispose(); }
});

test("provider deletion failure holds the balance and retries without duplicate charge", async () => {
  const h = await harness();
  try {
    h.buy(1);
    const started = await h.sessions.startWallet({ wallet: WALLET });
    h.clock.ms += 5_000;
    h.providerFails(true);
    await assert.rejects(h.sessions.endTicket(started.ticket), /provider down/);
    assert.equal((await h.usage.balance(WALLET, 1)).reservedSeconds, 60);
    assert.equal((await h.sessions.startWallet({ wallet: WALLET })).status, 409);
    h.providerFails(false);
    assert.equal((await h.sessions.endTicket(started.ticket)).seconds, 5);
    assert.equal((await h.usage.summary(WALLET)).sessionCount, 1);
    assert.equal((await h.usage.balance(WALLET, 1)).remainingSeconds, 55);
  } finally { await h.dispose(); }
});
