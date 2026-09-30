import assert from "node:assert/strict";
import test from "node:test";
import { createCustomerHarness, issueKey, startRequest, endRequest, ALICE, BOB } from "./testing.mjs";

test("the session API returns only the documented browser join fields", async () => {
  const h = await createCustomerHarness();
  try {
    const { apiKey } = await issueKey(h, ALICE);
    const response = await h.api.handle(startRequest(apiKey));
    assert.equal(response.status, 201);
    assert.deepEqual(Object.keys(response.body).sort(),
      ["avatar", "endsAt", "participantToken", "serverUrl", "sessionId"]);
    assert.deepEqual(response.body.avatar, {
      provider: "spatius", appId: "test-app-id", avatarId: "test-avatar-id",
    });
    assert.equal(typeof response.body.participantToken, "string");
    assert.equal(response.body.participantToken.includes("[object Promise]"), false);
  } finally { await h.dispose(); }
});

test("invalid keys and unknown avatars fail before a chain read or room open", async () => {
  let reads = 0;
  const h = await createCustomerHarness({ readCredits: async () => { reads += 1; return { purchasedMinutes: 60 }; } });
  try {
    const { apiKey } = await issueKey(h, ALICE);
    assert.equal((await h.api.handle(startRequest("bad-key"))).status, 401);
    assert.equal((await h.api.handle({ ...startRequest(apiKey), body: { avatar: "someone-else" } })).status, 400);
    assert.equal(reads, 0);
    assert.equal(h.livekit.opened.length, 0);
  } finally { await h.dispose(); }
});

test("the key wallet owns the room, the balance and the right to end it", async () => {
  const h = await createCustomerHarness();
  try {
    const alice = await issueKey(h, ALICE);
    const bob = await issueKey(h, BOB);
    const started = await h.api.handle({ ...startRequest(alice.apiKey), body: { wallet: BOB, avatar: "ava" } });
    assert.equal(started.status, 201);
    assert.equal(h.livekit.opened[0].wallet, ALICE);
    assert.equal((await h.api.handle(endRequest(bob.apiKey, started.body.sessionId))).status, 404);
    assert.equal((await h.api.handle(endRequest(alice.apiKey, started.body.sessionId))).status, 200);
    const usage = await h.api.handle({ method: "GET", path: "/v1/usage", authorization: "Bearer " + alice.apiKey });
    assert.equal(usage.status, 200);
    assert.equal(usage.body.wallet, ALICE);
    assert.equal(usage.body.purchasedMinutes, 60);
    assert.equal(usage.body.sessionCount, 1);
    assert.ok(usage.body.remainingSeconds < 3600);
  } finally { await h.dispose(); }
});
