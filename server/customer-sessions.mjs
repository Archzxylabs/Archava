import { randomBytes } from "node:crypto";
import { CreditError } from "./usage.mjs";

const MAX_SESSION_SECONDS = 30 * 60;

function publicView(hold) {
  return {
    id: hold.id, roomName: hold.roomName, wallet: hold.wallet,
    keyId: hold.keyId, startedAt: new Date(hold.startedAtMs).toISOString(),
    endsAt: hold.endsAt,
  };
}

/** One session manager for wallet calls and API-key calls. */
export function createCustomerSessions({ gate, keys, usage, livekit, sessionFields = {}, now = Date.now } = {}) {
  const closing = new Map();

  function publicAvatar() {
    const { avatarProvider = "", spatiusAppId = "", spatiusAvatarId = "" } = sessionFields;
    return { provider: avatarProvider, appId: spatiusAppId, avatarId: spatiusAvatarId };
  }

  async function close(hold, endedAtMs = now()) {
    if (closing.has(hold.id)) return closing.get(hold.id);
    const operation = (async () => {
      // Keep the reservation while a provider deletion is failing. Otherwise a
      // second room could open against refunded seconds as the old room runs.
      await livekit.deleteRoom(hold.roomName);
      return usage.settle(hold.id, endedAtMs);
    })();
    closing.set(hold.id, operation);
    try { return await operation; }
    finally { if (closing.get(hold.id) === operation) closing.delete(hold.id); }
  }

  async function startFor({ wallet, keyId }) {
    const entitlement = await gate(wallet);
    if (!entitlement.ok) return entitlement;
    const roomName = "archava-" + randomBytes(12).toString("hex");
    let hold;
    try {
      hold = await usage.reserve({
        wallet: entitlement.wallet, keyId, roomName,
        purchasedMinutes: entitlement.purchasedMinutes,
        maxSeconds: MAX_SESSION_SECONDS,
      });
    } catch (error) {
      if (error instanceof CreditError) return { ok: false, status: error.status, error: error.message };
      throw error;
    }
    let token;
    try {
      await livekit.openRoom({ roomName, wallet: hold.wallet, endsAt: hold.endsAt });
      token = await livekit.issueToken({ roomName, wallet: hold.wallet, endsAt: hold.endsAt });
    } catch (error) {
      try {
        await livekit.deleteRoom(roomName);
        await usage.release(hold.id);
      } catch (cleanupError) {
        console.error("Failed to clean up paid room; reservation retained:", roomName, cleanupError);
      }
      console.error("Could not start paid room:", error);
      return { ok: false, status: 502, error: "Service temporarily unavailable" };
    }
    return {
      ok: true, sessionId: hold.id, ticket: hold.ticket,
      serverUrl: sessionFields.serverUrl || "", participantToken: token,
      endsAt: hold.endsAt, avatar: publicAvatar(), session: publicView(hold),
    };
  }

  async function start({ apiKey }) {
    const key = await keys.findBySecret(apiKey);
    if (!key || key.revokedAt) return { ok: false, status: 401, error: "Invalid or revoked API key" };
    const result = await startFor({ wallet: key.wallet, keyId: key.id });
    if (result.ok) await Promise.resolve(keys.touch?.(key.id)).catch(() => {});
    return result;
  }

  async function startWallet({ wallet }) {
    return startFor({ wallet, keyId: "wallet" });
  }

  async function end({ sessionId, key }) {
    const hold = await usage.findHold(sessionId);
    if (!hold || hold.wallet !== key.wallet || hold.keyId !== key.id) {
      return { ok: false, status: 404, error: "Unknown session" };
    }
    const record = await close(hold);
    return { ok: true, session: publicView(hold), seconds: record?.seconds || 0 };
  }

  async function endTicket(ticket) {
    const hold = await usage.findByTicket(ticket);
    if (!hold || hold.keyId !== "wallet") return { ok: true, seconds: 0 };
    const record = await close(hold);
    return { ok: true, seconds: record?.seconds || 0 };
  }

  async function list({ key }) {
    return (await usage.listHolds()).filter((hold) =>
      hold.wallet === key.wallet && hold.keyId === key.id).map(publicView);
  }

  async function sweep() {
    const nowSeconds = Math.floor(now() / 1000);
    const expired = (await usage.listHolds()).filter((hold) => hold.endsAt <= nowSeconds);
    const closed = [];
    for (const hold of expired) {
      const record = await close(hold, hold.endsAt * 1000);
      closed.push({ id: hold.id, wallet: hold.wallet, seconds: record?.seconds || 0 });
    }
    return closed;
  }

  return { start, startWallet, end, endTicket, list, sweep };
}
