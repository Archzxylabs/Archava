import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { extname, resolve, sep } from "node:path";
import { normalizeWallet } from "./rental.mjs";
import { parseCookieHeader, DASHBOARD_SESSION_COOKIE } from "./developer-auth.mjs";

/**
 * The HTTP application for Archava.
 *
 * Nothing here is bound at import time: every collaborator — the chain reader,
 * the room provider, the stores, the challenge and session stores — arrives
 * through `dependencies`, and `origin`/`distDir` come from the same object. That
 * is what lets the tests run this exact handler over a real socket with fakes
 * in those slots, instead of asserting on its source text. `server/index.mjs` is
 * the only place that wires the real ones, and importing this module never
 * opens a port or reads a `.env`.
 */

const RATE_LIMIT = 30;
const RATE_WINDOW_MS = 60_000;
const MAX_BODY_BYTES = 16_384;
const BODY_METHODS = new Set(["POST", "PUT", "PATCH", "DELETE"]);

async function readJsonBody(req) {
  let body = "";
  for await (const part of req) {
    body += part;
    if (body.length > MAX_BODY_BYTES) throw new Error("Request too large");
  }
  return JSON.parse(body || "{}");
}

export function createApp(dependencies) {
  const {
    now = Date.now,
    origin,
    distDir,
    chainId,
    contractAddress,
    livekitUrl,
    avatarProvider,
    spatiusAppId,
    spatiusAvatarId,
    previewSeconds,
    packMinutes = [60, 300],
    readCredits = null,
    quotePack = null,
    usage,
    customerSessions,
    rooms = null,
    roomService = null,
    agentName,
    previewController,
    challenges,
    dashboardChallenges,
    dashboardSessions,
    developerApi,
    customerApi,
    sessionTickets,
    rateLimitKey = () => "shared",
  } = dependencies;

  const rateBuckets = new Map();

  function respond(res, status, data, requestOrigin = "", extraHeaders = {}) {
    const headers = {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
      ...extraHeaders,
    };
    if (requestOrigin === origin) {
      headers["access-control-allow-origin"] = origin;
      headers.vary = "Origin";
    }
    res.writeHead(status, headers);
    res.end(JSON.stringify(data));
  }

  function publicConfig() {
    return {
      chainId,
      contractAddress,
      livekitReady: Boolean(rooms),
      avatarProvider,
      ...(avatarProvider === "spatius" ? { spatiusAppId, spatiusAvatarId } : {}),
      previewEnabled: previewController.available(),
      previewSeconds,
      packMinutes,
    };
  }

  function allowRate(bucketKey) {
    const current = now();
    const recent = (rateBuckets.get(bucketKey) || []).filter((time) => current - time < RATE_WINDOW_MS);
    if (recent.length >= RATE_LIMIT) return false;
    recent.push(current);
    rateBuckets.set(bucketKey, recent);
    return true;
  }

  function pruneRateBuckets() {
    const current = now();
    for (const [bucketKey, times] of rateBuckets) {
      const recent = times.filter((time) => current - time < RATE_WINDOW_MS);
      if (recent.length) rateBuckets.set(bucketKey, recent);
      else rateBuckets.delete(bucketKey);
    }
  }

  async function serveStatic(pathname, res) {
    if (!existsSync(distDir)) return false;
    const path = resolve(distDir, "." + decodeURIComponent(pathname));
    if (path !== distDir && !path.startsWith(distDir + sep)) return false;
    const file = pathname === "/" || !extname(path) ? resolve(distDir, "index.html") : path;
    const types = {
      ".html": "text/html; charset=utf-8",
      ".js": "text/javascript; charset=utf-8",
      ".css": "text/css; charset=utf-8",
      ".svg": "image/svg+xml",
      ".png": "image/png",
      ".webp": "image/webp",
      ".jpg": "image/jpeg",
      ".jpeg": "image/jpeg",
      ".ico": "image/x-icon",
      ".woff2": "font/woff2",
      ".wasm": "application/wasm",
    };
    try {
      const body = await readFile(file);
      res.writeHead(200, {
        "content-type": types[extname(file)] || "application/octet-stream",
        "x-content-type-options": "nosniff",
        "cache-control": file.endsWith("index.html") ? "no-cache" : "public, max-age=31536000, immutable",
      });
      res.end(body);
      return true;
    } catch (error) {
      if (error.code === "ENOENT" || error.code === "EISDIR") return false;
      throw error;
    }
  }

  async function dispatchApi(url, req) {
    const method = req.method || "GET";
    const request = {
      method,
      path: url.pathname,
      cookie: req.headers.cookie || "",
      authorization: req.headers.authorization || "",
      body: BODY_METHODS.has(method) ? await readJsonBody(req) : {},
    };
    const api = url.pathname.startsWith("/api/developer") ? developerApi : customerApi;
    const result = await api.handle(request);
    return result || { status: 404, body: { error: "Not found" } };
  }

  /** The wallet-signed browser session. Separate from the customer API, always. */
  async function paidSession(req, res, requestOrigin) {
    const { wallet: input, nonce, signature } = await readJsonBody(req);
    if (!challenges.consume({ wallet: input, nonce, signature })) {
      return respond(res, 401, { error: "Invalid or expired wallet signature" }, requestOrigin);
    }
    const wallet = normalizeWallet(input);
    const result = await customerSessions.startWallet({ wallet });
    if (!result.ok) return respond(res, result.status, { error: result.error }, requestOrigin);
    return respond(res, 200, {
        serverUrl: livekitUrl,
        token: result.participantToken,
        endsAt: result.endsAt,
        preview: false,
        ticket: result.ticket,
        avatarProvider,
        ...(avatarProvider === "spatius" ? { spatiusAppId, spatiusAvatarId } : {}),
    }, requestOrigin);
  }

  async function handle(req, res) {
    const requestOrigin = req.headers.origin || "";
    const method = req.method || "GET";

    if (method === "OPTIONS") {
      res.writeHead(204, {
        "access-control-allow-origin": origin,
        "access-control-allow-methods": "GET, POST, OPTIONS",
        "access-control-allow-headers": "content-type, authorization",
        vary: "Origin",
      });
      return res.end();
    }
    if (requestOrigin && requestOrigin !== origin) {
      return respond(res, 403, { error: "Origin not allowed" }, requestOrigin);
    }

    const url = new URL(req.url || "/", "http://" + (req.headers.host || "localhost"));
    const isApi = url.pathname.startsWith("/api/") || url.pathname.startsWith("/v1/");
    // The anonymous preview is deliberately outside the generic limiter: a bucket
    // keyed on a shared proxy IP would cap preview usage, which is no longer
    // wanted. Everything else — including the metered /v1 surface — stays on the
    // 30/60s bucket.
    if (isApi && url.pathname !== "/api/preview" && !allowRate(rateLimitKey(req))) {
      return respond(res, 429, { error: "Too many requests" }, requestOrigin);
    }

    try {
      if (method === "GET" && url.pathname === "/api/health") {
        return respond(res, 200, { ok: true }, requestOrigin);
      }
      if (method === "GET" && url.pathname === "/api/config") {
        return respond(res, 200, publicConfig(), requestOrigin);
      }
      if (method === "GET" && url.pathname === "/api/quote") {
        if (!quotePack) return respond(res, 503, { error: "Minute-pack contract not configured" }, requestOrigin);
        const minutes = Number(url.searchParams.get("minutes") || packMinutes[0]);
        if (!packMinutes.includes(minutes)) return respond(res, 400, { error: "Unsupported minute pack" }, requestOrigin);
        return respond(res, 200, await quotePack(minutes), requestOrigin);
      }
      if (method === "GET" && url.pathname === "/api/access") {
        if (!readCredits) return respond(res, 503, { error: "Minute-pack contract not configured" }, requestOrigin);
        const wallet = normalizeWallet(url.searchParams.get("wallet"));
        const chain = await readCredits(wallet);
        return respond(res, 200, await usage.balance(wallet, chain.purchasedMinutes), requestOrigin);
      }
      if (method === "POST" && url.pathname === "/api/challenge") {
        const { wallet } = await readJsonBody(req);
        return respond(res, 200, challenges.issue(wallet), requestOrigin);
      }
      if (method === "POST" && url.pathname === "/api/preview") {
        if (!rooms) return respond(res, 503, { error: "LiveKit is not configured" }, requestOrigin);
        try {
          const result = await previewController.handle();
          return respond(res, result.status, result.body, requestOrigin);
        } catch (error) {
          console.error("Preview session failed:", error);
          return respond(res, 503, { error: "Service temporarily unavailable" }, requestOrigin);
        }
      }
      if (method === "POST" && url.pathname === "/api/session") {
        if (!rooms) return respond(res, 503, { error: "LiveKit is not configured" }, requestOrigin);
        return await paidSession(req, res, requestOrigin);
      }
      if (method === "POST" && url.pathname === "/api/session/end") {
        // The paid browser session is torn down by the one-shot ticket handed to
        // that browser, so a walk-away on a rented wallet stops its room.
        const { ticket } = await readJsonBody(req);
        if (typeof ticket !== "string") return respond(res, 400, { error: "Invalid session ticket" }, requestOrigin);
        const session = sessionTickets.get(ticket);
        if (session) {
          if (rooms) {
            await rooms.deleteRoom(session.roomName).catch((error) => {
              if (error?.status === 404 || error?.code === "not_found") return;
              throw error;
            });
          }
          sessionTickets.delete(ticket);
          return respond(res, 200, { ok: true }, requestOrigin);
        }
        return respond(res, 200, await customerSessions.endTicket(ticket), requestOrigin);
      }

      // Dashboard login is a signature over its own purpose-bound challenge, and
      // answers with the store's HttpOnly cookie. It sits ahead of the API so an
      // unauthenticated developer still has a way in.
      if (method === "POST" && url.pathname === "/api/developer/challenge") {
        const { wallet } = await readJsonBody(req);
        return respond(res, 200, dashboardChallenges.issue(wallet), requestOrigin);
      }
      if (method === "POST" && url.pathname === "/api/developer/login") {
        const { wallet, nonce, signature } = await readJsonBody(req);
        if (!dashboardChallenges.consume({ wallet, nonce, signature })) {
          return respond(res, 401, { error: "Invalid or expired wallet signature" }, requestOrigin);
        }
        const address = normalizeWallet(wallet);
        return respond(res, 200, { wallet: address }, requestOrigin, {
          "set-cookie": dashboardSessions.issue(address).cookie,
        });
      }
      if (method === "POST" && url.pathname === "/api/developer/logout") {
        const id = parseCookieHeader(req.headers.cookie || "")[DASHBOARD_SESSION_COOKIE];
        return respond(res, 200, { ok: true }, requestOrigin, {
          "set-cookie": dashboardSessions.destroy(id),
        });
      }

      // The customer and developer APIs answer before the static fallthrough. A
      // prefix is claimed in full: once it matches, an unrecognised path inside it
      // is a JSON 404, never the SPA shell a browser fetch would choke on.
      if (url.pathname.startsWith("/v1/") || url.pathname.startsWith("/api/developer")) {
        const result = await dispatchApi(url, req);
        return respond(res, result.status, result.body, requestOrigin);
      }

      if (method === "GET" && await serveStatic(url.pathname, res)) return;
      return respond(res, 404, { error: "Not found" }, requestOrigin);
    } catch (error) {
      if (error instanceof SyntaxError) return respond(res, 400, { error: "Invalid JSON" }, requestOrigin);
      if (error?.message === "Invalid wallet address" || error?.message === "Request too large") {
        return respond(res, 400, { error: error.message }, requestOrigin);
      }
      console.error("API error:", error);
      return respond(res, 503, { error: "Service temporarily unavailable" }, requestOrigin);
    }
  }

  return { handle, pruneRateBuckets };
}
