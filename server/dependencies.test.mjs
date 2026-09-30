import assert from "node:assert/strict";
import test from "node:test";
import { createDependencies } from "./dependencies.mjs";

test("production dashboard cookie works on local HTTP and is Secure on HTTPS", () => {
  const local = createDependencies({ env: { WEB_ORIGIN: "http://localhost:5002", AVATAR_PROVIDER: "tavus" } });
  const publicHost = createDependencies({ env: { WEB_ORIGIN: "https://archava.example", AVATAR_PROVIDER: "tavus" } });
  const localCookie = local.dashboardSessions.issue("0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC").cookie;
  const publicCookie = publicHost.dashboardSessions.issue("0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC").cookie;
  assert.doesNotMatch(localCookie, /; Secure(?:;|$)/);
  assert.match(publicCookie, /; Secure(?:;|$)/);
  assert.match(localCookie, /HttpOnly/);
  assert.match(publicCookie, /SameSite=Strict/);
});
