import assert from "node:assert/strict";
import test from "node:test";
import { Wallet } from "ethers";
import { createChallengeStore } from "./auth.mjs";
import { parseCookieHeader, createDashboardSessionStore } from "./developer-auth.mjs";

function makeStore(overrides = {}) {
  return createDashboardSessionStore({
    ttlMs: 30 * 60_000,
    now: () => 1_000_000,
    secure: true,
    ...overrides,
  });
}

test("a developer challenge names the dashboard, not a paid session", async () => {
  const wallet = Wallet.createRandom();
  const store = createChallengeStore({
    origin: "https://archava.example",
    purpose: "Archava developer dashboard login",
    ttlMs: 60_000,
  });
  const challenge = store.issue(wallet.address);
  assert.match(challenge.message, /Archava developer dashboard login/);
  assert.doesNotMatch(challenge.message, /Archava session access/);
  assert.match(challenge.message, /https:\/\/archava\.example/);
  const signature = await wallet.signMessage(challenge.message);
  assert.equal(store.consume({ wallet: wallet.address, ...challenge, signature }), true);
});

test("the default challenge message is unchanged so the paid browser flow keeps working", () => {
  const store = createChallengeStore({ origin: "https://archava.example" });
  const challenge = store.issue("0x" + "1".repeat(40));
  assert.match(challenge.message, /Archava session access/);
});

test("a signed developer challenge proves the wallet and consumes the nonce", async () => {
  const owner = Wallet.createRandom();
  const intruder = Wallet.createRandom();
  const store = createChallengeStore({
    origin: "https://archava.example",
    purpose: "Archava developer dashboard login",
  });
  const challenge = store.issue(owner.address);

  // The intruder signs the exact same message: the signature is valid but the
  // recovered address is not the challenged wallet.
  const forged = await intruder.signMessage(challenge.message);
  assert.equal(
    store.consume({ wallet: owner.address, ...challenge, signature: forged }),
    false,
    "a signature from a different wallet must not authenticate",
  );
  // A failed attempt burns the nonce, so the owner signs a fresh challenge.
  const fresh = store.issue(owner.address);
  const real = await owner.signMessage(fresh.message);
  assert.equal(store.consume({ wallet: owner.address, ...fresh, signature: real }), true);
  // Replay of a real signature must fail too: the nonce is one-use.
  assert.equal(
    store.consume({ wallet: owner.address, ...fresh, signature: real }),
    false,
    "a consumed challenge must not authorize again",
  );
});

test("a developer login sets an HTTP-only, SameSite=Strict, Secure session cookie", () => {
  const { cookie } = makeStore().issue("0x" + "a".repeat(40));
  assert.match(cookie, /^archava_session=[0-9a-f]{16,};/);
  assert.match(cookie, /(^|; )HttpOnly(;|$)/);
  assert.match(cookie, /(^|; )SameSite=Strict(;|$)/);
  assert.match(cookie, /(^|; )Secure(;|$)/);
  assert.match(cookie, /Path=\/api\/developer/);
  // The cookie must not be readable by script, and must not leak to other paths.
  assert.doesNotMatch(cookie, /document/i);
});

test("over plain HTTP the Secure attribute is dropped so the demo is not broken", () => {
  const { cookie } = makeStore({ secure: false }).issue("0x" + "a".repeat(40));
  assert.match(cookie, /(^|; )HttpOnly(;|$)/);
  assert.doesNotMatch(cookie, /(^|; )Secure(;|$)/);
});

test("a dashboard session resolves the wallet it was issued for", () => {
  const wallet = "0x" + "b".repeat(40);
  const store = makeStore();
  const { cookie } = store.issue(wallet);
  assert.equal(store.resolve("archava_session=" + parseCookieHeader(cookie).archava_session), wallet);
  // Resolving by the raw Cookie header is the real path.
  assert.equal(store.resolve(cookie), wallet);
});

test("a forged, expired, or absent cookie resolves to nobody", () => {
  const store = makeStore();
  const { cookie } = store.issue("0x" + "c".repeat(40));
  const id = parseCookieHeader(cookie).archava_session;
  assert.equal(store.resolve("archava_session=" + "f".repeat(id.length)), null);
  assert.equal(store.resolve("archava_session=" + id.toUpperCase()), null);
  assert.equal(store.resolve("other=1"), null);
  assert.equal(store.resolve(undefined), null);
  assert.equal(store.resolve(""), null);
});

test("a dashboard session expires after its ttl and is dropped", () => {
  let now = 5_000;
  const store = makeStore({ ttlMs: 1000, now: () => now });
  const { cookie } = store.issue("0x" + "d".repeat(40));
  assert.equal(store.resolve(cookie), "0x" + "d".repeat(40));
  now = 5_999;
  assert.equal(store.resolve(cookie), "0x" + "d".repeat(40));
  now = 6_001;
  assert.equal(store.resolve(cookie), null);
  store.prune();
  assert.equal(store.size(), 0);
});

test("logout clears the cookie and destroys the session so it cannot be replayed", () => {
  const wallet = "0x" + "e".repeat(40);
  const store = makeStore();
  const { cookie } = store.issue(wallet);
  const doomed = parseCookieHeader(cookie).archava_session;
  const cleared = store.destroy(doomed);
  assert.match(cleared, /archava_session=/);
  assert.match(cleared, /Max-Age=0/);
  assert.match(cleared, /(^|; )HttpOnly(;|$)/);
  assert.match(cleared, /(^|; )SameSite=Strict(;|$)/);
  assert.equal(store.resolve(cookie), null);
  // Destroying a second time must not throw: the UI may retry a logout.
  assert.match(store.destroy(doomed), /Max-Age=0/);
});

test("parseCookieHeader reads the first value and tolerates surrounding noise", () => {
  assert.deepEqual(parseCookieHeader("a=1; b=2"), { a: "1", b: "2" });
  assert.deepEqual(parseCookieHeader(" archava_session = xyz ; a=b "), { archava_session: "xyz", a: "b" });
  // A duplicate name must not flip the value used for lookup.
  assert.equal(parseCookieHeader("a=1; a=2").a, "1");
  assert.deepEqual(parseCookieHeader(""), {});
  assert.deepEqual(parseCookieHeader(undefined), {});
  assert.deepEqual(parseCookieHeader(";;;"), {});
});

test("a session store does not float unboundedly as expired sessions accumulate", () => {
  let now = 0;
  const store = makeStore({ ttlMs: 10, now: () => now });
  for (let i = 0; i < 50; i += 1) {
    now = i * 100;
    store.issue("0x" + String(i).padStart(40, "0"));
  }
  assert.equal(store.size(), 50);
  now = 10_000_000;
  store.prune();
  assert.equal(store.size(), 0);
});
