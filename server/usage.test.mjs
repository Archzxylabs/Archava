import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { createUsageStore } from "./usage.mjs";

const ALICE = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
const BOB = "0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB";

async function withStore(run) {
  const dir = await mkdtemp(join(process.cwd(), "archava-usage-"));
  try {
    return await run({ path: join(dir, "usage.json") });
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

test("a finished session is recorded as seconds against its key and wallet", async () => {
  await withStore(async ({ path }) => {
    const store = createUsageStore({ path, now: () => 5_000 });
    const entry = await store.record({
      keyId: "key_abc",
      wallet: ALICE,
      roomName: "archava-1",
      seconds: 95,
    });
    assert.equal(entry.keyId, "key_abc");
    assert.equal(entry.wallet, ALICE);
    assert.equal(entry.allocatedSeconds, 95);
    assert.equal(entry.endedAt, new Date(5_000).toISOString());
    assert.equal((await store.summary(ALICE)).allocatedSeconds, 95);

    await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "archava-2", seconds: 5 });
    assert.equal((await store.summary(ALICE)).allocatedSeconds, 100);
    assert.equal((await store.summary(BOB)).allocatedSeconds, 0);
  });
});

test("a second session for another wallet is invisible to the first", async () => {
  await withStore(async ({ path }) => {
    const store = createUsageStore({ path });
    await store.record({ keyId: "key_b", wallet: BOB, roomName: "archava-9", seconds: 42 });
    const [alice] = await store.list(ALICE);
    // No entry at all, not merely an empty list from a different key.
    assert.equal(alice, undefined);
    assert.equal((await store.summary(ALICE)).allocatedSeconds, 0);
  });
});

test("the record holds no secret, and the ledger survives a restart", async () => {
  await withStore(async ({ path }) => {
    const store = createUsageStore({ path, now: () => 7_000 });
    await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "archava-3", seconds: 61 });

    const raw = JSON.parse(await readFile(path, "utf8"));
    assert.equal(JSON.stringify(raw).includes("archava_sk_live_"), false);
    assert.equal(raw.version, 2);
    assert.deepEqual(Object.keys(raw.records[0]).sort(),
      ["endedAt", "id", "keyId", "roomName", "seconds", "wallet"]);
    // What is published is relabelled; what is stored keeps the ledger's own
    // field, so a file written by an earlier version still loads.
    assert.deepEqual(Object.keys((await store.list(ALICE))[0]).sort(),
      ["allocatedSeconds", "endedAt", "id", "keyId", "roomName", "wallet"]);

    // A fresh store over the same file reads back what was recorded.
    const restarted = createUsageStore({ path });
    assert.equal((await restarted.summary(ALICE)).allocatedSeconds, 61);
    assert.equal((await restarted.list(ALICE))[0].id, raw.records[0].id);
  });
});

test("an anonymous session is never counted, so a free preview cannot bump a wallet", async () => {
  await withStore(async ({ path }) => {
    const store = createUsageStore({ path });
    assert.equal(await store.record({ keyId: "", wallet: BOB, roomName: "preview-1", seconds: 90 }), null);
    assert.equal(await store.record({ keyId: null, wallet: BOB, roomName: "preview-2", seconds: 90 }), null);
    assert.equal((await store.summary(BOB)).allocatedSeconds, 0);
    assert.deepEqual(await store.list(BOB), []);
  });
});

test("a session whose clock reads wrong is refused rather than recorded as time served", async () => {
  await withStore(async ({ path }) => {
    const store = createUsageStore({ path });
    assert.equal(await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "r", seconds: -5 }), null);
    assert.equal(await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "r", seconds: Number.NaN }), null);
    assert.equal(await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "r", seconds: 1.5 }), null);
    assert.equal((await store.summary(ALICE)).allocatedSeconds, 0);
  });
});

test("the summary reports allocated seconds, a session count, and records labelled with the same unit", async () => {
  await withStore(async ({ path }) => {
    // A movable clock: the ledger must order records newest-first without
    // depending on wall-clock, so a test can assert the order deterministically.
    let nowMs = 1_000;
    const store = createUsageStore({ path, now: () => nowMs });
    await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "archava-1", seconds: 60 });
    nowMs = 2_000;
    await store.record({ keyId: "key_abc", wallet: ALICE, roomName: "archava-2", seconds: 5 });

    const summary = await store.summary(ALICE);
    assert.equal(summary.allocatedSeconds, 65);
    assert.equal(summary.sessionCount, 2);
    // The two numbers the customer and dashboard contracts publish, under one
    // unit: allocated room seconds, not a balance.
    assert.deepEqual(Object.keys(summary).sort(), ["allocatedSeconds", "sessionCount", "sessions"]);

    // Every record carries the same unit under the same name, so a caller
    // never has to know whether it is reading a list or a total.
    const [newest, oldest] = summary.sessions;
    assert.equal(newest.roomName, "archava-2");
    assert.equal(oldest.roomName, "archava-1");
    assert.equal(newest.allocatedSeconds, 5);
    assert.equal(oldest.allocatedSeconds, 60);
    assert.equal(newest.seconds, undefined, "the raw field name is no longer published");
    assert.deepEqual(Object.keys(newest).sort(),
      ["allocatedSeconds", "endedAt", "id", "keyId", "roomName", "wallet"]);
  });
});
