import { randomBytes } from "node:crypto";
import { AccessToken } from "livekit-server-sdk";
import { TrackSource } from "@livekit/protocol";

const AGENT_EVICT_GRACE_POLLS = 10;
const AGENT_EVICT_LATE_POLLS = 40;
const AGENT_EVICT_PAUSE_MS = 500;

function isMissingRoom(error) {
  return Boolean(error) && (error.status === 404 || error.code === "not_found");
}

/**
 * Hands a freshly opened room to the named host agent.
 *
 * An agent the operator set to join every room is not ours to manage, so the
 * automatic ones are deleted rather than removed afterwards; anything that is
 * already in the room as one of them is then evicted. A room that cannot get
 * its host is not usable, so the caller rolls it back.
 */
export async function attachHostAgent({ roomName, rooms, dispatch, agentName }) {
  if (typeof agentName !== "string" || !agentName.trim()) {
    throw new Error("ARCHAVA_AGENT_NAME must name the host agent");
  }
  const automaticDispatches = (await dispatch.listDispatch(roomName))
    .filter((dispatchRecord) => !dispatchRecord.agentName);
  for (const automatic of automaticDispatches) {
    await dispatch.deleteDispatch(automatic.id, roomName);
  }
  await dispatch.createDispatch(roomName, agentName);

  await removeAutomaticAgents(roomName, rooms, automaticDispatches);
}

/** A deleted dispatch can still have a job that joins later. Remove that exact
 * agent when it appears, without refusing a room when the cancelled job never
 * joins at all. The extra room slot keeps the guest able to connect meanwhile. */
export async function removeAutomaticAgents(roomName, rooms, dispatches, options = {}) {
  const identities = new Set(dispatches.flatMap((record) =>
    (record.state?.jobs || []).map((job) => `agent-${job.id}`)));
  if (!identities.size) return;

  const pauseMs = options.pauseMs ?? AGENT_EVICT_PAUSE_MS;
  const gracePolls = options.gracePolls ?? AGENT_EVICT_GRACE_POLLS;
  const latePolls = options.latePolls ?? AGENT_EVICT_LATE_POLLS;
  const pause = () => new Promise((resolve) => setTimeout(resolve, pauseMs));
  const removeJoined = async () => {
    const participants = await rooms.listParticipants(roomName);
    for (const participant of participants) {
      if (!identities.has(participant.identity)) continue;
      await rooms.removeParticipant(roomName, participant.identity);
      identities.delete(participant.identity);
    }
  };

  for (let attempt = 0; attempt < gracePolls && identities.size; attempt += 1) {
    await removeJoined();
    if (identities.size) await pause();
  }
  if (!identities.size) return;

  void (async () => {
    for (let attempt = 0; attempt < latePolls && identities.size; attempt += 1) {
      await removeJoined();
      if (identities.size) await pause();
    }
  })().catch((error) => {
    if (!isMissingRoom(error)) console.error("Late automatic-agent cleanup failed:", error);
  });
}

/**
 * The room-facing half of the customer session product.
 *
 * It holds the LiveKit grant, the host agent, and the room metadata the expiry
 * sweep reads. It knows nothing about keys, wallets, or entitlement — the
 * session layer decides who may have a room, and this only mints it.
 */
export function createRoomProvider({ rooms, dispatch, key, secret, agentName }) {
  async function openRoom({ roomName, wallet, endsAt }) {
    await rooms.createRoom({
      name: roomName,
      emptyTimeout: 60,
      departureTimeout: 30,
      maxParticipants: 4,
      metadata: JSON.stringify({ product: "archava", wallet, endsAt, preview: false }),
    });
    try {
      await attachHostAgent({ roomName, rooms, dispatch, agentName });
    } catch (error) {
      // A room without its host is a paid session that cannot work, and an
      // orphaned room would still be swept against the wallet's entitlement.
      await rooms.deleteRoom(roomName).catch(() => {});
      throw error;
    }
    return { roomName };
  }

  async function issueToken({ roomName, wallet, endsAt }) {
    const nowSeconds = Math.floor(Date.now() / 1000);
    // The token lives exactly as long as the room: the room itself is deleted
    // at its end by the sweep, so a room cannot exceed its reserved seconds.
    // A room that has already ended is refused, not quietly extended.
    const ttl = endsAt - nowSeconds;
    if (ttl <= 0) throw new Error("Cannot mint a token for an ended room");
    const token = new AccessToken(key, secret, {
      // The wallet on the key is the identity: a customer room can be told
      // apart from a free preview room, whose identity is an anonymous guest.
      identity: wallet,
      name: "Archava customer",
      ttl,
    });
    token.addGrant({
      roomJoin: true,
      room: roomName,
      canPublish: true,
      canPublishSources: [TrackSource.MICROPHONE],
      canPublishData: true,
      canSubscribe: true,
    });
    return await token.toJwt();
  }

  async function deleteRoom(roomName) {
    await rooms.deleteRoom(roomName).catch((error) => {
      // Metering must not be lost because the room was already gone.
      if (isMissingRoom(error)) return;
      throw error;
    });
  }

  return { openRoom, issueToken, deleteRoom };
}
