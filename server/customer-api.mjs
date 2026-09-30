import { bearerKey } from "./http-auth.mjs";

/**
 * The developer-facing HTTP surface for the paid session product.
 *
 * It is deliberately thin: every request resolves a Bearer key, then hands the
 * work to the session module, which owns the ordering that matters (key first,
 * chain and balance second, room last). This layer never invents entitlement of its own,
 * and never accepts a wallet from the body — a room always belongs to the
 * wallet on the key that paid for it.
 */
export function createCustomerApi({ gate, keys, usage, sessions, readCredits }) {
  async function handle(request) {
    const { method, path, authorization, body = {} } = request;

    if (method === "GET" && path === "/v1/sessions") {
      const key = await keys.findBySecret(bearerKey(authorization));
      if (!key) return { status: 401, body: { error: "Invalid or revoked API key" } };
      return { status: 200, body: { wallet: key.wallet, sessions: await sessions.list({ key }) } };
    }

    if (method === "GET" && path === "/v1/usage") {
      const key = await keys.findBySecret(bearerKey(authorization));
      if (!key) return { status: 401, body: { error: "Invalid or revoked API key" } };
      if (!readCredits) return { status: 503, body: { error: "Minute-pack contract not configured" } };
      let purchasedMinutes;
      try { purchasedMinutes = (await readCredits(key.wallet)).purchasedMinutes; }
      catch { return { status: 502, body: { error: "Could not read purchased minutes" } }; }
      return { status: 200, body: {
        wallet: key.wallet,
        ...await usage.summary(key.wallet),
        ...await usage.balance(key.wallet, purchasedMinutes),
      } };
    }

    if (method !== "POST" || !path.startsWith("/v1/sessions")) return null;

    // The Bearer credential is resolved before anything else happens, so a
    // request with no key is answered before a chain read or a room is spent.
    const apiKey = bearerKey(authorization);
    const key = await keys.findBySecret(apiKey);
    if (!key) return { status: 401, body: { error: "Invalid or revoked API key" } };

    if (path === "/v1/sessions" || path === "/v1/sessions/") {
      // Ava is named by request, never assumed. An avatar the product does not
      // serve is refused here rather than mapped onto Ava, so a customer is
      // never silently put in a room they did not ask for. Only an absent field
      // counts as "not named"; any other value but "ava" is another avatar.
      const avatar = (body || {}).avatar;
      if (avatar !== undefined && avatar !== "ava") {
        return { status: 400, body: { error: "Unknown avatar. Only \"ava\" is available." } };
      }
      // The credential travels as presented: the store keeps only a digest of
      // it, so there is no `secret` field on the record to hand over instead.
      const result = await sessions.start({ apiKey });
      if (!result.ok) return { status: result.status, body: { error: result.error } };
      return { status: result.status ?? 201, body: {
        sessionId: result.sessionId,
        serverUrl: result.serverUrl,
        participantToken: result.participantToken,
        endsAt: result.endsAt,
        avatar: result.avatar,
      } };
    }

    // The end route is path-based: `POST /v1/sessions/:id/end`. The wallet is
    // never read from the body — the key record already names its owner, so a
    // second valid key cannot close the first key's session.
    const ended = /^\/v1\/sessions\/([^/]+)\/end\/?$/.exec(path);
    if (ended) {
      const result = await sessions.end({ sessionId: decodeURIComponent(ended[1]), key });
      if (!result.ok) return { status: result.status, body: { error: result.error } };
      return { status: 200, body: { session: result.session, seconds: result.seconds } };
    }

    return null;
  }

  return { handle };
}
