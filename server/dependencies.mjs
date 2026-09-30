import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import { AccessToken, AgentDispatchClient, RoomServiceClient } from "livekit-server-sdk";
import { TrackSource } from "@livekit/protocol";
import { createChallengeStore } from "./auth.mjs";
import { createCreditReader, createPackQuoter } from "./rental.mjs";
import { createPreviewController } from "./preview.mjs";
import { createKeyStore } from "./apikeys.mjs";
import { createUsageStore } from "./usage.mjs";
import { createEntitlementGate } from "./entitlement.mjs";
import { createCustomerSessions } from "./customer-sessions.mjs";
import { createCustomerApi } from "./customer-api.mjs";
import { createDeveloperApi } from "./developer-api.mjs";
import { createDashboardSessionStore } from "./developer-auth.mjs";
import { createRoomProvider, removeAutomaticAgents } from "./livekit-rooms.mjs";
import { createApp } from "./app.mjs";

/**
 * Builds every collaborator the HTTP application needs, from the environment.
 *
 * This is the only place production values are read, and it returns rather than
 * listens, so `server/index.mjs` stays a bootstrap and the tests can build the
 * same graph with fakes substituted for the room provider and the stores.
 */

const DIST_DIR = "dist";
const STATE_DIR = "data";
const SWEEP_INTERVAL_MS = 10_000;

export function createDependencies({ env = process.env, cwd = process.cwd(), stateDir } = {}) {
  const origin = env.WEB_ORIGIN || "http://localhost:5174";
  const chainId = Number(env.CHAIN_ID || 97);
  const livekitUrl = env.LIVEKIT_URL || "";
  const livekitKey = env.LIVEKIT_API_KEY || "";
  const livekitSecret = env.LIVEKIT_API_SECRET || "";
  const avatarProvider = (env.AVATAR_PROVIDER || "tavus").trim().toLowerCase();
  if (!["tavus", "spatius"].includes(avatarProvider)) {
    throw new Error("AVATAR_PROVIDER must be tavus or spatius");
  }
  const spatiusAppId = env.SPATIUS_APP_ID || "";
  const spatiusAvatarId = env.SPATIUS_AVATAR_ID || "";
  if (avatarProvider === "spatius" && (!env.SPATIUS_API_KEY || !spatiusAppId || !spatiusAvatarId)) {
    throw new Error("Spatius requires SPATIUS_API_KEY, SPATIUS_APP_ID, and SPATIUS_AVATAR_ID");
  }
  const previewSeconds = Math.min(180, Math.max(30, Number(env.PREVIEW_SECONDS || 90)));
  const packMinutes = [60, 300];
  const agentName = env.ARCHAVA_AGENT_NAME || "archava-host";

  const livekitConfigured = Boolean(livekitUrl && livekitKey && livekitSecret);
  const livekitHttpUrl = livekitUrl.replace(/^wss:/, "https:").replace(/^ws:/, "http:");
  const roomService = livekitConfigured
    ? new RoomServiceClient(livekitHttpUrl, livekitKey, livekitSecret)
    : null;
  const dispatchService = livekitConfigured
    ? new AgentDispatchClient(livekitHttpUrl, livekitKey, livekitSecret)
    : null;
  const rooms = livekitConfigured
    ? createRoomProvider({ rooms: roomService, dispatch: dispatchService, agentName, key: livekitKey, secret: livekitSecret })
    : null;

  const rentalConfig = {
    rpcUrl: env.RPC_URL,
    contractAddress: env.PACK_CONTRACT || "",
    chainId,
  };
  const readCredits = createCreditReader(rentalConfig);
  const quotePack = createPackQuoter(rentalConfig);

  // The developer-facing product: a key proves a customer, the chain proves the
  // key's wallet purchased minutes, and the ledger checks remaining time. The key
  // store, the ledger, and the rooms all live next to each other in data/.
  const dataDir = stateDir || resolve(cwd, STATE_DIR);
  const keys = createKeyStore({ path: resolve(dataDir, "keys.json") });
  const usage = createUsageStore({ path: resolve(dataDir, "usage.json") });
  // Customer API and browser sessions share this entitlement gate and ledger.
  const checkEntitlement = createEntitlementGate({ readCredits, usage });
  const customerSessions = createCustomerSessions({
    gate: checkEntitlement,
    keys,
    usage,
    livekit: rooms || {
      // With no LiveKit configured a customer session cannot be honest about its
      // room, so it is refused rather than answered with a room that cannot open.
      async openRoom() { throw new Error("LiveKit is not configured"); },
      async deleteRoom() {},
      issueToken() { throw new Error("LiveKit is not configured"); },
    },
    // The LiveKit URL and the public avatar identifiers a customer's browser
    // needs are read from the server's own public config, never from a request.
    sessionFields: { serverUrl: livekitUrl, avatarProvider, spatiusAppId, spatiusAvatarId },
  });
  const customerApi = createCustomerApi({ gate: checkEntitlement, keys, usage, sessions: customerSessions, readCredits });

  // The dashboard proves a wallet with its own purpose-bound challenge, so a
  // signature collected for a room can never be replayed to mint API keys.
  const challenges = createChallengeStore({ origin });
  const dashboardChallenges = createChallengeStore({
    origin,
    purpose: "Archava developer dashboard access",
  });
  const sessionTickets = new Map();
  const creatingSessions = new Set();
  // A Secure cookie is silently dropped over plain HTTP, so it is set only for an
  // HTTPS origin; local development keeps the insecure cookie on /api/developer.
  const cookieOptions = { secure: false };
  try {
    const site = new URL(origin);
    if (site.protocol === "https:") {
      cookieOptions.secure = true;
    }
  } catch { /* Keep the local HTTP default when WEB_ORIGIN is not a URL. */ }
  const dashboardSessions = createDashboardSessionStore(cookieOptions);
  const developerApi = createDeveloperApi({
    dashboard: {
      resolve(cookieHeader) {
        // A dashboard login is its own short-lived session, not a room ticket: a
        // visitor holding a preview ticket has proved nothing about a wallet.
        const wallet = dashboardSessions.resolve(cookieHeader);
        return wallet ? { wallet } : null;
      },
    },
    keys,
    readCredits,
    usage,
  });

  const previewController = createPreviewController({
    enabled: env.ENABLE_PREVIEW === "true",
    seconds: previewSeconds,
    now: Date.now,
    openRoom: async ({ endsAt }) => {
      if (!rooms) throw new Error("LiveKit is not configured");
      return openPreviewRoom({ endsAt, dispatchService, roomService, rooms, agentName, livekitKey, livekitSecret });
    },
    sessionFields: { serverUrl: livekitUrl, avatarProvider, spatiusAppId, spatiusAvatarId },
    onSession: (ticket, session) => sessionTickets.set(ticket, session),
  });

  return {
    /** Closes LiveKit rooms whose metadata says their time is up. */
    closeExpiredRooms: async () => {
      if (!roomService) return [];
      const listed = await roomService.listRooms();
      const nowSeconds = Math.floor(Date.now() / 1000);
      const closed = [];
      for (const room of listed) {
        let meta;
        try { meta = JSON.parse(room.metadata || "{}"); } catch { continue; }
        if (meta.product !== "archava" || !Number.isFinite(meta.endsAt)) continue;
        if (meta.endsAt <= nowSeconds) {
          await roomService.deleteRoom(room.name).catch((error) => {
            if (error.status === 404 || error.code === "not_found") return;
            console.error("Failed to close expired room:", room.name, error);
          });
          closed.push(room.name);
        }
      }
      return closed;
    },
    origin,
    distDir: resolve(cwd, DIST_DIR),
    chainId,
    contractAddress: env.PACK_CONTRACT || "",
    livekitUrl,
    avatarProvider,
    spatiusAppId,
    spatiusAvatarId,
    previewSeconds,
    packMinutes,
    rooms,
    roomService,
    challenges,
    dashboardChallenges,
    dashboardSessions,
    developerApi,
    customerApi,
    // The sweep in `index.mjs` closes abandoned paid rooms on its tick, so the
    // instance it must reach has to be on the object `createDependencies`
    // returns — building it internally was not enough.
    customerSessions,
    usage,
    readCredits,
    quotePack,
    previewController,
    sessionTickets,
    creatingSessions,
    // A shared proxy IP would otherwise cap preview usage for everyone behind it.
    rateLimitKey: (req) => req.socket.remoteAddress || "unknown",
  };
}

/**
 * The anonymous preview room: a fresh guest identity, a short token, and the
 * host agent dispatched only once the automatic dispatches are gone.
 */
async function openPreviewRoom({ endsAt, dispatchService, roomService, rooms, agentName, livekitKey, livekitSecret }) {
  const roomName = "archava-" + randomBytes(12).toString("hex");
  const nowSec = Math.floor(Date.now() / 1000);
  const token = new AccessToken(livekitKey, livekitSecret, {
    identity: "guest-" + randomBytes(8).toString("hex"),
    name: "Archava guest",
    ttl: Math.max(30, Math.min(300, endsAt - nowSec)),
  });
  token.addGrant({
    roomJoin: true,
    room: roomName,
    canPublish: true,
    canPublishSources: [TrackSource.MICROPHONE],
    canPublishData: true,
    canSubscribe: true,
  });
  const jwt = await token.toJwt();
  await roomService.createRoom({
    name: roomName,
    emptyTimeout: 60,
    departureTimeout: 30,
    maxParticipants: 4,
    metadata: JSON.stringify({ product: "archava", preview: true, endsAt }),
  });
  try {
    const automaticDispatches = (await dispatchService.listDispatch(roomName)).filter((dispatch) => !dispatch.agentName);
    for (const dispatch of automaticDispatches) {
      await dispatchService.deleteDispatch(dispatch.id, roomName);
    }
    await dispatchService.createDispatch(roomName, agentName);
    await removeAutomaticAgents(roomName, roomService, automaticDispatches);
  } catch (error) {
    await roomService.deleteRoom(roomName).catch(() => {});
    throw error;
  }
  return { roomName, token: jwt };
}

export { SWEEP_INTERVAL_MS, createApp };
