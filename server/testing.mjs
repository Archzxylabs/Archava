import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { createCustomerSessions } from "./customer-sessions.mjs";
import { createCustomerApi } from "./customer-api.mjs";
import { createEntitlementGate } from "./entitlement.mjs";
import { createKeyStore } from "./apikeys.mjs";
import { createUsageStore } from "./usage.mjs";

/**
 * Test-only harness for the paid product's server half.
 *
 * It wires the *real* key store, usage ledger, entitlement gate, session module
 * and customer API together, and fakes only the two things that may not be
 * called in a test: the chain read and the LiveKit service. Everything in
 * between — digests, file integrity, cap arithmetic, ownership, metering,
 * HTTP routing — is production code under test.
 */

export const ALICE = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
export const BOB = "0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB";

/** What the public session response advertises to a customer's browser. */
export const SESSION_FIELDS = {
  serverUrl: "wss://archava-test.livekit.cloud",
  avatarProvider: "spatius",
  spatiusAppId: "test-app-id",
  spatiusAvatarId: "test-avatar-id",
};

/** A LiveKit stand-in. `issueToken` is async on purpose: the real one is. */
function fakeLiveKit({ token = "jwt-for-room" } = {}) {
  const opened = [];
  const deleted = [];
  return {
    opened,
    deleted,
    async openRoom({ roomName, wallet, endsAt }) {
      opened.push({ roomName, wallet, endsAt });
      return { roomName };
    },
    // Resolved, not returned: a caller that forgets to await this gets a Promise.
    async issueToken({ roomName }) { return token + ":" + roomName; },
    async deleteRoom(roomName) { deleted.push(roomName); },
  };
}

/**
 * @param {object} options
 * @param {function} options.readCredits the fake chain reader
 * @param {function} [options.now] the shared fake clock in ms
 */
export async function createCustomerHarness({
  readCredits = async () => ({ purchasedMinutes: 60 }),
  now = Date.now,
} = {}) {
  const dir = await mkdtemp(join(process.cwd(), "archava-customer-"));
  const keys = createKeyStore({ path: join(dir, "keys.json"), now });
  const usage = createUsageStore({ path: join(dir, "usage.json"), now });
  const livekit = fakeLiveKit();
  const gate = createEntitlementGate({ readCredits, usage });
  const sessions = createCustomerSessions({
    gate,
    keys,
    usage,
    livekit,
    now,
    sessionFields: SESSION_FIELDS,
  });
  const api = createCustomerApi({ gate, keys, usage, sessions, readCredits });
  return {
    api,
    sessions,
    keys,
    usage,
    livekit,
    async dispose() {
      await rm(dir, { recursive: true, force: true });
    },
  };
}

/** Issues a real key for `wallet` and answers with it plus its key record. */
export async function issueKey(harness, wallet) {
  const issued = await harness.keys.issue(wallet, { label: "test key" });
  const record = await harness.keys.findBySecret(issued.apiKey);
  return { ...issued, record };
}

/** POST /v1/sessions with only a Bearer credential. */
export function startRequest(secret) {
  return { method: "POST", path: "/v1/sessions", authorization: "Bearer " + secret, body: {} };
}

/** POST /v1/sessions/:id/end — the path-based end route. */
export function endRequest(secret, sessionId) {
  return {
    method: "POST",
    path: "/v1/sessions/" + encodeURIComponent(sessionId) + "/end",
    authorization: "Bearer " + secret,
    body: {},
  };
}
