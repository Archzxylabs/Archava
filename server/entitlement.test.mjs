import assert from "node:assert/strict";
import test from "node:test";
import { createEntitlementGate } from "./entitlement.mjs";

const WALLET = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
const usage = {
  async balance(_wallet, purchasedMinutes) {
    return { purchasedMinutes, usedSeconds: 75, reservedSeconds: 0,
      remainingSeconds: Math.max(0, purchasedMinutes * 60 - 75), active: purchasedMinutes * 60 > 75 };
  },
};

test("gate grants only remaining purchased seconds", async () => {
  const gate = createEntitlementGate({ readCredits: async () => ({ purchasedMinutes: 60 }), usage });
  assert.deepEqual(await gate(WALLET), {
    ok: true, wallet: WALLET, purchasedMinutes: 60, usedSeconds: 75,
    reservedSeconds: 0, remainingSeconds: 3525, active: true,
  });
});

test("zero or exhausted packs cannot open a room", async () => {
  const gate = createEntitlementGate({ readCredits: async () => ({ purchasedMinutes: 0 }), usage });
  assert.equal((await gate(WALLET)).status, 403);
});

test("an already reserved wallet reports a room conflict", async () => {
  const gate = createEntitlementGate({ readCredits: async () => ({ purchasedMinutes: 1 }),
    usage: { balance: async () => ({ purchasedMinutes: 1, usedSeconds: 0,
      reservedSeconds: 60, remainingSeconds: 0, active: false }) } });
  assert.equal((await gate(WALLET)).status, 409);
});

test("a missing contract, unreadable chain, or unavailable ledger fails closed", async () => {
  assert.equal((await createEntitlementGate({ usage })(WALLET)).status, 503);
  const brokenChain = createEntitlementGate({ readCredits: async () => { throw new Error("RPC down"); }, usage });
  assert.equal((await brokenChain(WALLET)).status, 502);
  const brokenLedger = createEntitlementGate({ readCredits: async () => ({ purchasedMinutes: 60 }),
    usage: { balance: async () => { throw new Error("disk down"); } } });
  assert.equal((await brokenLedger(WALLET)).status, 503);
});

test("malformed chain totals and wallets are rejected", async () => {
  const invalid = createEntitlementGate({ readCredits: async () => ({ purchasedMinutes: Number.NaN }), usage });
  assert.equal((await invalid(WALLET)).status, 502);
  assert.equal((await invalid("not a wallet")).status, 400);
});
