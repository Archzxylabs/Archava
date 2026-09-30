import assert from "node:assert/strict";
import test from "node:test";
import { bearerKey } from "./http-auth.mjs";

test("a Bearer key is read off the Authorization header", () => {
  assert.equal(bearerKey("Bearer archava_sk_live_abc"), "archava_sk_live_abc");
  assert.equal(bearerKey("bearer archava_sk_live_abc"), "archava_sk_live_abc");
  assert.equal(bearerKey("Bearer   archava_sk_live_abc  "), "archava_sk_live_abc");
});

test("anything that is not one Bearer credential is refused, not guessed at", () => {
  for (const bad of [undefined, null, "", "Basic abc", "Bearer", "Bearer ", "Bearer a b", "archava_sk_live_abc"]) {
    assert.equal(bearerKey(bad), null, JSON.stringify(bad));
  }
});
