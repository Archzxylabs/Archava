import assert from "node:assert/strict";
import test from "node:test";
import { createDeveloperApi } from "./developer-api.mjs";
import { publicUsage } from "./usage.mjs";

const ALICE = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
const BOB = "0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB";
const DASHBOARD_COOKIE = "archava_session=session-1";

// Shaped exactly like the real store in apikeys.mjs: records carry only a
// digest, `issue` is positional and answers with `apiKey`, and `revoke` already
// refuses a wallet that does not own the key.
function fakeKeys() {
  const records = [
    { id: "key_alice", wallet: ALICE, prefix: "abcdef01", secretHash: "sha256:alice", label: "Archava customer key", createdAt: "2026-09-01T00:00:00.000Z", revokedAt: null, lastUsedAt: null },
  ];
  return {
    records,
    async issue(wallet, { label } = {}) {
      const record = {
        id: "key_new",
        wallet,
        prefix: "12345678",
        secretHash: "sha256:new",
        label: label || "Archava customer key",
        createdAt: "2026-09-29T00:00:00.000Z",
        revokedAt: null,
        lastUsedAt: null,
      };
      records.push(record);
      return { id: record.id, apiKey: "archava_sk_live_" + "f".repeat(64), wallet, label: record.label, prefix: record.prefix, createdAt: record.createdAt };
    },
    async list(wallet) {
      return records
        .filter((record) => record.wallet === wallet)
        .map(({ id, wallet: owner, prefix, label, createdAt, lastUsedAt, revokedAt }) =>
          ({ id, wallet: owner, prefix, label, createdAt, lastUsedAt, revokedAt }));
    },
    async revoke(id, wallet) {
      const record = records.find((entry) => entry.id === id);
      if (!record || record.wallet !== wallet || record.revokedAt) return null;
      record.revokedAt = "2026-09-29T00:00:00.000Z";
      return record;
    },
    async touch() {},
  };
}

function harness({ keys = fakeKeys(), credits = async () => ({ purchasedMinutes: 60 }) } = {}) {
  const dashboard = {
    resolve(cookieHeader) {
      return cookieHeader === DASHBOARD_COOKIE ? { wallet: ALICE } : null;
    },
  };
  const api = createDeveloperApi({
    dashboard,
    keys,
    readCredits: credits,
    usage: {
      // The real publisher, fed the same labelled records the store answers with.
      async summary(wallet) {
        return publicUsage(wallet === ALICE ? [{ id: "use_1", keyId: "key_alice", wallet: ALICE, roomName: "archava-1", allocatedSeconds: 42 }] : []);
      },
      async balance(_wallet, purchasedMinutes) {
        return { purchasedMinutes, usedSeconds: 42, reservedSeconds: 0, remainingSeconds: purchasedMinutes * 60 - 42, active: true };
      },
    },
  });
  return { api, keys };
}

test("GET /api/developer/me reports the logged-in wallet, keys and minute balance", async () => {
  const h = harness();
  const res = await h.api.handle({ method: "GET", path: "/api/developer/me", cookie: DASHBOARD_COOKIE });
  assert.equal(res.status, 200);
  assert.equal(res.body.wallet, ALICE);
  assert.equal(res.body.keys.length, 1);
  // The secret, and its digest, are never part of the dashboard answer.
  assert.equal(res.body.keys[0].secret, undefined);
  assert.equal(res.body.keys[0].secretHash, undefined);
  assert.equal(res.body.usage.remainingSeconds, 3558);
  // Allocated room seconds and a session count, named as the customer contract
  // names them: `src/lib/developer.ts` refuses to render anything else.
  assert.equal(res.body.usage.allocatedSeconds, 42);
  assert.equal(res.body.usage.sessionCount, 1);
});

test("every developer route needs the dashboard session cookie", async () => {
  const h = harness();
  for (const [method, path] of [
    ["GET", "/api/developer/me"],
    ["GET", "/api/developer/keys"],
    ["POST", "/api/developer/keys"],
    ["POST", "/api/developer/keys/key_alice/revoke"],
  ]) {
    const res = await h.api.handle({ method, path, cookie: "archava_session=nope", body: {} });
    assert.equal(res.status, 401, method + " " + path);
  }
});

test("POST /api/developer/keys issues a key for the session wallet and shows the secret once", async () => {
  const h = harness();
  const res = await h.api.handle({ method: "POST", path: "/api/developer/keys", cookie: DASHBOARD_COOKIE, body: { label: "CI" } });
  assert.equal(res.status, 201);
  // Flat, and only once: the plaintext secret exists in this answer and in no
  // later one, because the store keeps a digest of it.
  assert.equal(res.body.apiKey, "archava_sk_live_" + "f".repeat(64));
  assert.equal(res.body.id, "key_new");
  assert.equal(res.body.label, "CI");
  assert.equal(res.body.prefix, "12345678");
  assert.equal(res.body.createdAt, "2026-09-29T00:00:00.000Z");
  assert.deepEqual(Object.keys(res.body).sort(),
    ["apiKey", "createdAt", "id", "label", "prefix"]);
  assert.equal(res.body.secret, undefined);
  assert.equal(res.body.key, undefined, "the nested shape is gone");
  assert.equal(res.body.secretHash, undefined);
  assert.equal(h.keys.records.length, 2);

  // Listing afterwards never returns it again.
  const list = await h.api.handle({ method: "GET", path: "/api/developer/keys", cookie: DASHBOARD_COOKIE });
  assert.equal(JSON.stringify(list.body).includes(res.body.apiKey), false);
});

test("a key cannot be issued for a wallet the session does not own", async () => {
  const h = harness();
  const res = await h.api.handle({
    method: "POST",
    path: "/api/developer/keys",
    cookie: DASHBOARD_COOKIE,
    body: { wallet: BOB },
  });
  assert.equal(res.status, 400);
  assert.equal(res.body.key, undefined);
  assert.equal(h.keys.records.length, 1);
});

test("revoking another wallet's key is refused, even by a logged-in developer", async () => {
  const h = harness();
  h.keys.records.push({ id: "key_bob", wallet: BOB, prefix: "99999999", label: "b", createdAt: "2026-09-01T00:00:00.000Z", revokedAt: null, lastUsedAt: null });
  const res = await h.api.handle({ method: "POST", path: "/api/developer/keys/key_bob/revoke", cookie: DASHBOARD_COOKIE, body: {} });
  assert.equal(res.status, 404);
  assert.equal(h.keys.records[1].revokedAt, null);
});

test("revoking one's own key answers the contract's flat acknowledgement", async () => {
  const h = harness();
  const res = await h.api.handle({ method: "POST", path: "/api/developer/keys/key_alice/revoke", cookie: DASHBOARD_COOKIE, body: {} });
  assert.equal(res.status, 200);
  assert.equal(h.keys.records[0].revokedAt, "2026-09-29T00:00:00.000Z");
  // `revokeDeveloperKey` in src/lib/developer.ts types this as `{ok: true}`; a
  // revoked key record here would leak the digest-free record as a second shape.
  assert.deepEqual(res.body, { ok: true });
});

test("GET /api/developer/usage answers allocated time and a minute balance", async () => {
  const h = harness();
  const res = await h.api.handle({ method: "GET", path: "/api/developer/usage", cookie: DASHBOARD_COOKIE });
  assert.equal(res.status, 200);
  assert.equal(res.body.wallet, ALICE);
  // Allocated room seconds are subtracted from purchased seconds.
  assert.equal(res.body.allocatedSeconds, 42);
  assert.equal(res.body.sessionCount, 1);
  assert.equal(res.body.sessions.length, 1);
  assert.equal(res.body.purchasedMinutes, 60);
  assert.equal(res.body.remainingSeconds, 3558);
  assert.equal(res.body.balanceAvailable, true);
  assert.equal(res.body.totalSeconds, undefined, "the old field name is gone");
  // The session carries the published unit under the published name, so it
  // reconciles against the total without a second vocabulary.
  assert.equal(res.body.sessions[0].allocatedSeconds, 42);
  assert.equal(res.body.sessions[0].seconds, undefined, "the raw field name is no longer published");
});

test("the login and logout routes are never claimed by the dashboard API", async () => {
  // If the API claimed them it would answer "login required" for the very
  // routes that establish a login, and no developer could ever get in.
  const h = harness();
  for (const [method, path] of [
    ["POST", "/api/developer/challenge"],
    ["POST", "/api/developer/login"],
    ["POST", "/api/developer/logout"],
  ]) {
    const res = await h.api.handle({ method, path, cookie: "archava_session=nope", body: {} });
    assert.equal(res, null, method + " " + path + " must fall through to the login routes");
  }
  // A login attempt with a valid-looking wallet still reaches the login routes,
  // not an auth refusal.
  assert.equal(await h.api.handle({
    method: "POST",
    path: "/api/developer/login",
    cookie: DASHBOARD_COOKIE,
    body: { wallet: BOB },
  }), null);
});

test("an unknown path under /api/developer is not claimed", async () => {
  const h = harness();
  assert.equal(await h.api.handle({ method: "GET", path: "/api/developer/unknown", cookie: DASHBOARD_COOKIE }), null);
  assert.equal(await h.api.handle({ method: "GET", path: "/api/me", cookie: DASHBOARD_COOKIE }), null);
});
