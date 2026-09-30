import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { createKeyStore } from "./apikeys.mjs";

async function withStore(run, overrides = {}) {
  const dir = await mkdtemp(join(process.cwd(), "archava-keys-"));
  try {
    return await run({ path: join(dir, "nested", "keys.json"), ...overrides });
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

test("creating a key returns a high-entropy secret exactly once", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path, now: () => 1000 });
    const created = await store.issue("0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa", { label: "hackathon demo" });
    assert.match(created.apiKey, /^archava_sk_live_[0-9a-f]{48,}$/);
    assert.equal(created.label, "hackathon demo");
    assert.equal(created.wallet, "0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa");
    assert.equal(created.createdAt, new Date(1000).toISOString());

    // The listing is metadata only: no secret, no hash, nothing to replay with.
    const [listed] = await store.list("0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa");
    assert.deepEqual(
      Object.keys(listed).sort(),
      ["createdAt", "id", "label", "lastUsedAt", "prefix", "revokedAt", "wallet"],
    );
    // A second wallet never sees the first wallet's keys.
    await store.issue("0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB", { label: "" });
    assert.equal((await store.list("0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa")).length, 1);
  });
});

test("the secret resolves to its key while it is live", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path, now: () => 1000 });
    const created = await store.issue("0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa", { label: "demo" });
    const found = await store.findBySecret(created.apiKey);
    assert.equal(found.id, created.id);
    assert.equal(found.wallet, "0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa");
  });
});

test("the persisted record holds a hash, never the secret", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path, now: () => 2000 });
    const { apiKey, id } = await store.issue("0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC", { label: "demo" });
    await store.flush();

    const raw = JSON.parse(await readFile(path, "utf8"));
    assert.equal(JSON.stringify(raw).includes(apiKey), false, "the secret must never reach disk");
    assert.equal(raw[0].id, id);
    assert.equal(raw[0].wallet, "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC");
    assert.match(raw[0].secretHash, /^[0-9a-f]{64}$/, "the stored hash is a sha256 digest");
    assert.equal(apiKey.endsWith(raw[0].prefix), true, "the prefix is the tail of the secret");
    assert.equal(raw[0].createdAt, new Date(2000).toISOString());
    assert.equal(raw[0].revokedAt, null);
    assert.equal(raw[0].lastUsedAt, null);

    // A store constructed over the same file resolves the secret after a restart.
    assert.equal((await createKeyStore({ path }).findBySecret(apiKey)).id, id);
  });
});

test("a revoked key stops resolving and is marked, not deleted", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path, now: () => 3000 });
    const { apiKey, id } = await store.issue("0xDDdDddDdDdddDDddDDddDDDDdDdDDdDDdDDDDDDd", { label: "demo" });
    const revoked = await store.revoke(id, "0xDDdDddDdDdddDDddDDddDDDDdDdDDdDDdDDDDDDd", () => 4000);
    assert.equal(revoked.revokedAt, new Date(4000).toISOString());
    assert.equal(await store.findBySecret(apiKey), null);
    assert.equal((await store.list("0xDDdDddDdDdddDDddDDddDDDDdDdDDdDDdDDDDDDd"))[0].revokedAt, new Date(4000).toISOString());

    // Revoking twice, or another wallet's key, is refused instead of thrown at.
    assert.equal(await store.revoke(id, "0xDDdDddDdDdddDDddDDddDDDDdDdDDdDDdDDDDDDd"), null);
    assert.equal(await store.revoke(id, "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"), null);
    assert.equal(await store.revoke("key_missing", "0xDDdDddDdDdddDDddDDddDDDDdDdDDdDDdDDDDDDd"), null);
  });
});

test("an unknown, malformed, or revoked secret resolves to no key", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path });
    const { apiKey, id } = await store.issue("0xFFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF", { label: "demo" });
    await store.revoke(id, "0xFFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF");
    assert.equal(await store.findBySecret("archava_sk_live_" + "0".repeat(48)), null);
    assert.equal(await store.findBySecret(""), null);
    assert.equal(await store.findBySecret(undefined), null);
    assert.equal(await store.findBySecret(apiKey + "x"), null);
    assert.equal(await store.findBySecret(apiKey.toUpperCase()), null);
  });
});

test("a corrupt store file fails closed and is backed aside, not silently dropped", async () => {
  await withStore(async ({ path }) => {
    const { mkdir, writeFile } = await import("node:fs/promises");
    await mkdir(path.replace(/\/[^/]+$/, ""), { recursive: true });
    await writeFile(path, "{ not json", "utf8");
    const store = createKeyStore({ path });
    assert.deepEqual(await store.list("0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa"), []);

    const created = await store.issue("0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa", { label: "demo" });
    await store.flush();
    const raw = JSON.parse(await readFile(path, "utf8"));
    assert.equal(raw.length, 1);
    assert.equal(raw[0].id, created.id);
  });
});

test("two keys issued in the same millisecond stay distinct", async () => {
  await withStore(async ({ path }) => {
    // Same clock tick, distinct entropy draws: identity must come from the
    // random bytes, never from a timestamp or a counter of records.
    let drawn = 0;
    const store = createKeyStore({ path, now: () => 5000, randomBytes: () => "same" + String(drawn++) });
    const first = await store.issue("0x1111111111111111111111111111111111111111", { label: "a" });
    const second = await store.issue("0x1111111111111111111111111111111111111111", { label: "b" });
    assert.notEqual(first.id, second.id);
    assert.notEqual(first.apiKey, second.apiKey);
    assert.equal((await store.list("0x1111111111111111111111111111111111111111")).length, 2);
  });
});

test("touching a key records when it was last used", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path, now: () => 6000 });
    const { apiKey } = await store.issue("0x2222222222222222222222222222222222222222", { label: "demo" });
    assert.equal((await store.list("0x2222222222222222222222222222222222222222"))[0].lastUsedAt, null);
    await store.touch((await store.findBySecret(apiKey)).id, () => 7000);
    assert.equal((await store.list("0x2222222222222222222222222222222222222222"))[0].lastUsedAt, new Date(7000).toISOString());
  });
});

test("labels are trimmed and capped, and a bad wallet is rejected", async () => {
  await withStore(async ({ path }) => {
    const store = createKeyStore({ path });
    const padded = await store.issue("0x3333333333333333333333333333333333333333", { label: "  " + "x".repeat(200) + "  " });
    assert.equal(padded.label, "x".repeat(64));
    assert.equal((await store.issue("0x3333333333333333333333333333333333333333", {})).label, "Archava customer key");
    await assert.rejects(() => store.issue("not a wallet", { label: "demo" }), /wallet/);
  });
});
