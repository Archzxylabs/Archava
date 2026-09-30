import assert from "node:assert/strict";
import test from "node:test";
import { Wallet } from "ethers";
import { createChallengeStore } from "./auth.mjs";

test("a signed challenge proves wallet control and can only be used once", async () => {
  const wallet = Wallet.createRandom();
  const store = createChallengeStore({ origin: "https://archava.example" });
  const challenge = store.issue(wallet.address);
  assert.match(challenge.message, /https:\/\/archava\.example/);
  const signature = await wallet.signMessage(challenge.message);
  assert.equal(store.consume({ wallet: wallet.address, ...challenge, signature }), true);
  assert.equal(store.consume({ wallet: wallet.address, ...challenge, signature }), false);
});

test("a failed signature consumes the challenge", async () => {
  const owner = Wallet.createRandom();
  const intruder = Wallet.createRandom();
  const store = createChallengeStore({ origin: "https://archava.example" });
  const challenge = store.issue(owner.address);
  const bad = await intruder.signMessage(challenge.message);
  const good = await owner.signMessage(challenge.message);
  assert.equal(store.consume({ wallet: owner.address, ...challenge, signature: bad }), false);
  assert.equal(store.consume({ wallet: owner.address, ...challenge, signature: good }), false);
});

test("expired challenges cannot authorize a session", async () => {
  let now = 1000;
  const wallet = Wallet.createRandom();
  const store = createChallengeStore({ origin: "https://archava.example", ttlMs: 10, now: () => now });
  const challenge = store.issue(wallet.address);
  const signature = await wallet.signMessage(challenge.message);
  now = 1011;
  assert.equal(store.consume({ wallet: wallet.address, ...challenge, signature }), false);
});
