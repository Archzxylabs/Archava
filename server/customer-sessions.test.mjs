import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { createUsageStore } from "./usage.mjs";
import { createEntitlementGate } from "./entitlement.mjs";
import { createCustomerSessions } from "./customer-sessions.mjs";

const WALLET = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
const KEY = { id: "key_1", wallet: WALLET };

async function harness(run) {
  const dir = await mkdtemp(join(process.cwd(), ".session-test-"));
  const clock = { ms: 1_800_000_000_000 };
  let failOpen = false;
  let failToken = false;
  let failDelete = false;
  const opened = new Set();
  const livekit = {
    async openRoom({ roomName }) { if (failOpen) throw new Error("open failed"); opened.add(roomName); },
    async issueToken() { if (failToken) throw new Error("token failed"); return "jwt"; },
    async deleteRoom(roomName) { if (failDelete) throw new Error("delete failed"); opened.delete(roomName); },
  };
  const usage = createUsageStore({ path: join(dir, "usage.json"), now: () => clock.ms });
  const gate = createEntitlementGate({ readCredits: async () => ({ purchasedMinutes: 60 }), usage });
  const sessions = createCustomerSessions({ gate, usage, livekit, now: () => clock.ms,
    keys: { findBySecret: async (secret) => secret === "valid" ? KEY : null } });
  try {
    return await run({ sessions, usage, clock, opened,
      setFailOpen(value) { failOpen = value; },
      setFailToken(value) { failToken = value; },
      setFailDelete(value) { failDelete = value; } });
  } finally { await rm(dir, { recursive: true, force: true }); }
}

test("two simultaneous starts cannot reserve the same wallet twice", async () => {
  await harness(async (h) => {
    const [one, two] = await Promise.all([
      h.sessions.startWallet({ wallet: WALLET }), h.sessions.start({ apiKey: "valid" }),
    ]);
    assert.deepEqual([one.status || 200, two.status || 200].sort(), [200, 409]);
    assert.equal(h.opened.size, 1);
    assert.equal((await h.usage.listHolds()).length, 1);
  });
});

test("failed provider setup releases the reserved minutes", async () => {
  await harness(async (h) => {
    h.setFailOpen(true);
    assert.equal((await h.sessions.startWallet({ wallet: WALLET })).status, 502);
    assert.equal((await h.usage.balance(WALLET, 60)).remainingSeconds, 3600);
    h.setFailOpen(false);
    h.setFailToken(true);
    assert.equal((await h.sessions.startWallet({ wallet: WALLET })).status, 502);
    assert.equal(h.opened.size, 0);
    assert.equal((await h.usage.balance(WALLET, 60)).remainingSeconds, 3600);
  });
});

test("deletion failure retains the reservation until a later successful end", async () => {
  await harness(async (h) => {
    const started = await h.sessions.start({ apiKey: "valid" });
    h.clock.ms += 9_000;
    h.setFailDelete(true);
    await assert.rejects(h.sessions.end({ sessionId: started.sessionId, key: KEY }), /delete failed/);
    assert.equal((await h.usage.balance(WALLET, 60)).reservedSeconds, 1800);
    h.setFailDelete(false);
    assert.equal((await h.sessions.end({ sessionId: started.sessionId, key: KEY })).seconds, 9);
    assert.equal((await h.usage.summary(WALLET)).sessionCount, 1);
  });
});

test("a different key cannot close the current key's session", async () => {
  await harness(async (h) => {
    const started = await h.sessions.start({ apiKey: "valid" });
    const other = { id: "key_2", wallet: WALLET };
    assert.equal((await h.sessions.end({ sessionId: started.sessionId, key: other })).status, 404);
    assert.equal(h.opened.size, 1);
  });
});
