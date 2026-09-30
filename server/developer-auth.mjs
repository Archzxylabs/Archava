import { randomBytes } from "node:crypto";

export const DASHBOARD_SESSION_COOKIE = "archava_session";
const DASHBOARD_SESSION_PATH = "/api/developer";

/**
 * Reads a Cookie header into a plain object. The first value for a duplicated
 * name wins, matching how a browser sends the cookie it actually stored.
 */
export function parseCookieHeader(header) {
  const jar = {};
  if (typeof header !== "string") return jar;
  for (const part of header.split(";")) {
    const separator = part.indexOf("=");
    if (separator < 1) continue;
    const name = part.slice(0, separator).trim();
    if (!name || Object.hasOwn(jar, name)) continue;
    jar[name] = part.slice(separator + 1).trim();
  }
  return jar;
}

/**
 * Dashboard logins after a signed, purpose-bound challenge. Sessions live only
 * in memory: they are short-lived, and a restart must not resurrect a login.
 */
export function createDashboardSessionStore({ ttlMs = 30 * 60_000, now = Date.now, secure = true } = {}) {
  const sessions = new Map();

  function cookieAttributes() {
    const attributes = [
      "Path=" + DASHBOARD_SESSION_PATH,
      "HttpOnly",
      "SameSite=Strict",
      "Max-Age=" + Math.floor(ttlMs / 1000),
    ];
    // A Secure cookie is silently dropped over plain HTTP, which would break
    // the local demo, so it is only requested when the request came over TLS.
    if (secure) attributes.push("Secure");
    return attributes.join("; ");
  }

  function issue(wallet) {
    const id = randomBytes(24).toString("hex");
    sessions.set(id, { wallet, expires: now() + ttlMs });
    return { id, cookie: DASHBOARD_SESSION_COOKIE + "=" + id + "; " + cookieAttributes() };
  }

  function resolve(cookieHeader) {
    const id = parseCookieHeader(cookieHeader)[DASHBOARD_SESSION_COOKIE];
    if (typeof id !== "string") return null;
    const session = sessions.get(id);
    if (!session) return null;
    if (session.expires < now()) {
      sessions.delete(id);
      return null;
    }
    return session.wallet;
  }

  function destroy(id) {
    sessions.delete(id);
    return DASHBOARD_SESSION_COOKIE + "=; "
      + cookieAttributes().replace(/Max-Age=\d+/, "Max-Age=0");
  }

  function prune() {
    for (const [id, session] of sessions) {
      if (session.expires < now()) sessions.delete(id);
    }
  }

  return { issue, resolve, destroy, prune, size: () => sessions.size };
}
