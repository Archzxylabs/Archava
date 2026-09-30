import assert from "node:assert/strict";
import test from "node:test";

import {
  createDeveloperKey,
  developerErrorMessage,
  developerKeyStatus,
  DeveloperUsageUnavailableError,
  fetchDeveloperUsage,
  fetchDeveloperWallet,
  formatAllocatedSessionTime,
  listDeveloperKeys,
  logoutDeveloper,
  needsDeveloperSignIn,
  revokeDeveloperKey,
  signChallengeMessage,
} from "./developer.ts";
import { ApiError } from "./apiError.ts";

// Behavioural test for the developer dashboard client. Every network call is a
// mocked fetch, so this exercises the real module — including the field-by-field
// rebuild that keeps a leaked secret out of the UI — without a server, a wallet
// extension, or a provider session.

const WALLET = "0x1111111111111111111111111111111111111111";
const OTHER_WALLET = "0x2222222222222222222222222222222222222222";

/** EIP-1193 stub: just enough for ethers' BrowserProvider + signMessage. */
function stubWallet(address: string) {
  const request = async ({ method }: { method: string }) => {
    if (method === "eth_accounts" || method === "eth_requestAccounts") return [address];
    if (method === "eth_chainId") return "0x61";
    if (method === "personal_sign") return "0xdeadbeef";
    throw new Error(`unexpected wallet method: ${method}`);
  };
  (globalThis as { window?: unknown }).window = {
    ethereum: { request, on: () => {}, removeListener: () => {} },
  };
}

interface Recorded {
  path: string;
  method: string;
  body: unknown;
  credentials: string | undefined;
  contentType: string | undefined;
}

interface MockOptions {
  status?: number;
  body?: unknown;
}

function mockFetch(handlers: Record<string, MockOptions>) {
  const calls: Recorded[] = [];
  (globalThis as { fetch?: unknown }).fetch = async (input: unknown, init: Record<string, unknown> = {}) => {
    const path = String(input);
    const method = String(init.method ?? "GET");
    const key = handlers[`${method} ${path}`] ?? handlers[path];
    assert.ok(key, `unmocked developer call: ${method} ${path}`);
    calls.push({
      path,
      method,
      body: typeof init.body === "string" ? JSON.parse(init.body) : undefined,
      credentials: init.credentials as string | undefined,
      contentType: (init.headers as Record<string, string> | undefined)?.["Content-Type"],
    });
    const status = key.status ?? 200;
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => key.body ?? {},
    } as Response;
  };
  return calls;
}

const ok = (body: unknown): MockOptions => ({ body });
const fail = (status: number, error: string): MockOptions => ({ status, body: { error } });

test("authenticateDeveloper exchanges a signed challenge for a dashboard cookie", async () => {
  stubWallet(WALLET);
  const { authenticateDeveloper } = await import("./developer.ts");
  const calls = mockFetch({
    "POST /api/developer/challenge": ok({ nonce: "n-1", message: "Sign in to Archava developer dashboard" }),
    "POST /api/developer/login": ok({ wallet: WALLET }),
  });

  const wallet = await authenticateDeveloper(WALLET);

  assert.equal(wallet.wallet, WALLET);
  assert.equal(calls.length, 2, "challenge and login are the only two calls");

  const [challenge, login] = calls;
  assert.equal(challenge.method, "POST");
  assert.deepEqual(challenge.body, { wallet: WALLET });
  assert.equal(challenge.credentials, "same-origin", "the dashboard cookie must ride on every call");

  assert.equal(login.path, "/api/developer/login");
  assert.equal(login.contentType, "application/json");
  assert.deepEqual(login.body, {
    wallet: WALLET,
    nonce: "n-1",
    signature: "0xdeadbeef",
  });
});

test("signChallengeMessage refuses a wallet that changed since connect", async () => {
  stubWallet(OTHER_WALLET);

  await assert.rejects(
    () => signChallengeMessage(WALLET, "Sign in"),
    /Connected wallet changed/,
    "a signature from the wrong account must never be forwarded",
  );
});

test("createDeveloperKey reveals the secret once and sends only a label", async () => {
  mockFetch({
    "POST /api/developer/keys": ok({
      id: "key_1",
      apiKey: "arch_live_SECRETVALUE",
      label: "hackathon demo",
      prefix: "arch_live_SECRET",
      createdAt: "2026-09-30T09:14:22.000Z",
    }),
  });

  const created = await createDeveloperKey("hackathon demo");

  assert.equal(created.apiKey, "arch_live_SECRETVALUE", "the create response is the only place the secret exists");
  assert.equal(created.prefix, "arch_live_SECRET");
});

test("createDeveloperKey fails closed when the server omits the secret", async () => {
  // A 200 whose body lost `apiKey` must never be rendered as a usable key, and
  // must never be reported to the visitor as a successful creation.
  mockFetch({
    "POST /api/developer/keys": ok({
      id: "key_1",
      label: "hackathon demo",
      prefix: "arch_live_SECRET",
      createdAt: "2026-09-30T09:14:22.000Z",
    }),
  });

  await assert.rejects(
    () => createDeveloperKey("hackathon demo"),
    /did not return the new key/,
    "a missing secret is an error, not a key the dashboard can show",
  );
});

test("createDeveloperKey fails closed on an empty-string secret", async () => {
  mockFetch({
    "POST /api/developer/keys": ok({ id: "key_1", apiKey: "", label: "x", prefix: "arch", createdAt: "2026-09-30T09:14:22.000Z" }),
  });

  await assert.rejects(() => createDeveloperKey("x"), /did not return the new key/);
});

test("listDeveloperKeys drops a leaked secret field", async () => {
  mockFetch({
    "GET /api/developer/keys": ok({
      keys: [
        {
          id: "key_1",
          label: "hackathon demo",
          prefix: "arch_live_SECRET",
          createdAt: "2026-09-30T09:14:22.000Z",
          lastUsedAt: "2026-09-30T10:00:00.000Z",
          revokedAt: null,
          apiKey: "arch_live_SECRETVALUE",
          wallet: WALLET,
        },
      ],
    }),
  });

  const [key] = await listDeveloperKeys();

  assert.notEqual(
    (key as unknown as { apiKey?: string }).apiKey,
    "arch_live_SECRETVALUE",
    "a secret in the listing payload must be dropped, not rendered",
  );
  assert.deepEqual(Object.keys(key).sort(), ["createdAt", "id", "label", "lastUsedAt", "prefix", "revokedAt"]);
  assert.equal(key.lastUsedAt, "2026-09-30T10:00:00.000Z");
});

test("revokeDeveloperKey encodes the id and posts to the documented path", async () => {
  const calls = mockFetch({
    "POST /api/developer/keys/key%2F1/revoke": ok({ ok: true }),
  });

  await revokeDeveloperKey("key/1");

  assert.equal(calls[0].path, "/api/developer/keys/key%2F1/revoke");
  assert.equal(calls[0].method, "POST");
});

test("usage, wallet and logout all ride on the dashboard cookie", async () => {
  const calls = mockFetch({
    "GET /api/developer/usage": ok({ allocatedSeconds: 3723, sessionCount: 4 }),
    "GET /api/developer/me": ok({ wallet: WALLET }),
    "POST /api/developer/logout": ok({ ok: true }),
  });

  const usage = await fetchDeveloperUsage();
  const me = await fetchDeveloperWallet();
  await logoutDeveloper();

  assert.deepEqual(usage, { allocatedSeconds: 3723, sessionCount: 4, balanceAvailable: false });
  assert.equal(me.wallet, WALLET);
  for (const call of calls) {
    assert.equal(call.credentials, "same-origin");
  }
});

test("usage is read as allocated session seconds and session count", async () => {
  mockFetch({
    "GET /api/developer/usage": ok({ allocatedSeconds: 3723, sessionCount: 4 }),
  });

  const usage = await fetchDeveloperUsage();

  assert.equal(usage.allocatedSeconds, 3723);
  assert.equal(usage.sessionCount, 4);
  assert.equal(
    (usage as unknown as { connectedSeconds?: number }).connectedSeconds,
    undefined,
    "the old field name must not survive anywhere in the parsed usage",
  );
});

test("usage refuses an unreadable number instead of rendering NaN", async () => {
  // Every payload below would otherwise reach JSX as `NaN`, and a wrong number
  // here understates what the key has actually spent — so each is an error.
  const unusable: Record<string, unknown>[] = [
    {},
    { allocatedSeconds: 3723 },
    { sessionCount: 4 },
    { allocatedSeconds: null, sessionCount: 4 },
    { allocatedSeconds: NaN, sessionCount: 4 },
    { allocatedSeconds: Infinity, sessionCount: 4 },
    { allocatedSeconds: "3723", sessionCount: 4 },
    { allocatedSeconds: 3723, sessionCount: "4" },
  ];

  for (const body of unusable) {
    mockFetch({ "GET /api/developer/usage": ok(body) });
    const cause = await fetchDeveloperUsage().then(
      () => null,
      (error: unknown) => error,
    );

    assert.ok(
      cause instanceof DeveloperUsageUnavailableError,
      `${JSON.stringify(body)} must not be reported as usable usage`,
    );
    assert.match(developerErrorMessage(cause), /no session time is shown/);
  }
});

test("usage unavailability is its own failure, not a transport or session error", async () => {
  // A 200 whose body is unusable has no HTTP status to report, so it must not be
  // dressed up as a transport problem — and it must not sign the visitor out.
  mockFetch({ "GET /api/developer/usage": ok({ sessionCount: 4 }) });

  const cause = await fetchDeveloperUsage().then(
    () => null,
    (error: unknown) => error,
  );

  assert.ok(cause instanceof Error);
  assert.ok(!(cause instanceof ApiError), "a broken body is not an HTTP failure");
  assert.equal(needsDeveloperSignIn(cause), false);
  assert.match(developerErrorMessage(cause), /Usage could not be read/);
});

test("allocated session time renders as minutes and seconds, never NaN", () => {
  assert.equal(formatAllocatedSessionTime(0), "0 min 00 s");
  assert.equal(formatAllocatedSessionTime(3723), "62 min 03 s");
  assert.equal(formatAllocatedSessionTime(59.9), "0 min 59 s");
  // A clock is measured from when the room is created, so a sub-minute reading
  // is a real reading, not a rounding artefact.
  assert.equal(formatAllocatedSessionTime(61), "1 min 01 s");
  // A read that could not be measured must never look like a measurement of zero.
  assert.equal(formatAllocatedSessionTime(NaN), "—");
  assert.equal(formatAllocatedSessionTime(Infinity), "—");
  assert.equal(formatAllocatedSessionTime(-1), "0 min 00 s", "a negative is clamped, not shown");
});

test("a 401 is recognised as needing a fresh signature", () => {
  assert.equal(needsDeveloperSignIn(new ApiError(401, "nope")), true);
  assert.equal(needsDeveloperSignIn(new ApiError(500, "boom")), false);
  assert.equal(needsDeveloperSignIn(new Error("boom")), false);

  assert.match(developerErrorMessage(new ApiError(401, "")), /challenge/i);
  assert.match(developerErrorMessage(new ApiError(409, "")), /unused key/);
  assert.match(developerErrorMessage(new ApiError(429, "")), /Too many requests/);
  assert.match(developerErrorMessage(new Error("offline")), /temporarily unavailable/);
});

test("key status comes from the server, never from the client", () => {
  assert.equal(
    developerKeyStatus({
      id: "a",
      label: "l",
      prefix: "p",
      createdAt: "2026-09-30T09:14:22.000Z",
      lastUsedAt: null,
      revokedAt: null,
    }),
    "active",
  );
  assert.equal(
    developerKeyStatus({
      id: "a",
      label: "l",
      prefix: "p",
      createdAt: "2026-09-30T09:14:22.000Z",
      lastUsedAt: null,
      revokedAt: "2026-09-30T11:00:00.000Z",
    }),
    "revoked",
  );
});

test("an HTTP error becomes a readable message instead of the raw body", async () => {
  mockFetch({ "GET /api/developer/usage": fail(409, "key limit reached") });

  const cause = await fetchDeveloperUsage().then(
    () => null,
    (error: unknown) => error,
  );

  assert.ok(cause instanceof ApiError, "the HTTP status must survive to the caller");
  assert.equal(cause.status, 409);
  // The mapped, user-facing copy wins over the raw server string when one exists.
  assert.equal(developerErrorMessage(cause), "Key limit reached. Revoke an unused key first.");
  assert.notEqual(developerErrorMessage(new ApiError(400, "internal detail X")), "internal detail X");
});
