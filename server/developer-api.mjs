/**
 * The dashboard's own API: issue, list, and revoke the logged-in wallet's keys,
 * and report purchased minutes and server-metered usage behind them.
 *
 * Every route is answered against the wallet in the session cookie. A wallet
 * named in a request body is only ever validated against that session wallet,
 * never trusted, so a logged-in developer cannot mint, read, or revoke a key
 * that belongs to another address.
 */
// These three establish and end a login, so answering them here would refuse
// the very request that creates a session. index.mjs owns them.
const LOGIN_PATHS = new Set([
  "/api/developer/challenge",
  "/api/developer/login",
  "/api/developer/logout",
]);

export function createDeveloperApi({ dashboard, keys, readCredits, usage }) {
  // The store keeps only a digest, so a digest must never travel back out.
  async function usageSummary(wallet) {
    const history = await usage.summary(wallet);
    if (!readCredits) return { ...history, balanceAvailable: false };
    try {
      const { purchasedMinutes } = await readCredits(wallet);
      return { ...history, ...await usage.balance(wallet, purchasedMinutes), balanceAvailable: true };
    } catch {
      return { ...history, balanceAvailable: false };
    }
  }

  async function handle(request) {
    const { method, path, cookie, body = {} } = request;

    if (!path.startsWith("/api/developer")) return null;
    if (LOGIN_PATHS.has(path)) return null;

    const session = dashboard.resolve(cookie);
    if (!session) return { status: 401, body: { error: "Dashboard login required" } };
    const wallet = session.wallet;

    if (method === "GET" && path === "/api/developer/me") {
      const usageStatus = await usageSummary(wallet);
      return {
        status: 200,
        body: {
          wallet,
          keys: await keys.list(wallet),
          usage: usageStatus,
        },
      };
    }

    if (method === "GET" && path === "/api/developer/keys") {
      return { status: 200, body: { wallet, keys: await keys.list(wallet) } };
    }

    if (method === "POST" && path === "/api/developer/keys") {
      if (body.wallet && body.wallet !== wallet) {
        return { status: 400, body: { error: "A key can only be issued for the logged-in wallet" } };
      }
      // The wallet is positional because the store normalises and binds it, and
      // the one plaintext secret it answers with is named `apiKey`.
      const issued = await keys.issue(wallet, { label: body.label });
      // The secret is returned exactly once, here; what is stored is only a
      // digest of it, so it cannot be read back later. The dashboard shows it
      // once and never persists it, so this payload is the only copy.
      return { status: 201, body: {
        id: issued.id,
        apiKey: issued.apiKey,
        label: issued.label,
        prefix: issued.prefix,
        createdAt: issued.createdAt,
      } };
    }

    if (method === "GET" && path === "/api/developer/usage") {
      // The sessions travelled inside the same published pair, so this route
      // answers one shape rather than duplicating the totals under new names.
      return { status: 200, body: {
        wallet,
        ...await usageSummary(wallet),
      } };
    }

    if (method === "POST" && path.startsWith("/api/developer/keys/") && path.endsWith("/revoke")) {
      const id = path.slice("/api/developer/keys/".length, -"/revoke".length);
      // The store refuses a key that belongs to another wallet, and answers
      // null for a key that never existed, so the two cases are indistinguishable.
      const revoked = await keys.revoke(id, wallet);
      if (!revoked) return { status: 404, body: { error: "Unknown API key" } };
      // The acknowledgement the dashboard's `revokeDeveloperKey` expects. The
      // revoked record is not echoed back: a listing already carries it, with no
      // secret and no digest.
      return { status: 200, body: { ok: true } };
    }

    return null;
  }

  return { handle };
}
