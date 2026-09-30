import { randomBytes } from "node:crypto";

/**
 * Own the anonymous-preview decision (enable flag, session length) and the
 * response shape. It never sees LiveKit or provider secrets: the LiveKit work is
 * injected through `openRoom` and only public fields travel in `sessionFields`,
 * so the minted `AvatarSession` cannot leak them.
 *
 * There is deliberately no usage state here. Anonymous previews carry no per
 * browser cooldown, no global budget, and no concurrency ceiling, so a visitor
 * can start another preview the moment one ends and two openings can overlap.
 * Availability is one question only — is the preview switched on — which is what
 * `/api/config` reports.
 *
 * @param {object} options
 * @param {boolean} [options.enabled] mirror of ENABLE_PREVIEW
 * @param {number}  [options.seconds] preview session length
 * @param {() => number} [options.now] injected clock (ms) for deterministic tests
 * @param {({ endsAt: number, preview: true }) => Promise<{ roomName: string, token: string }>} [options.openRoom]
 * @param {() => string} [options.newTicket]
 * @param {{ serverUrl: string, avatarProvider: string, spatiusAppId?: string, spatiusAvatarId?: string }} [options.sessionFields]
 * @param {(ticket: string, session: { roomName: string, endsAt: number, preview: true }) => void} [options.onSession]
 */
export function createPreviewController(options = {}) {
  const {
    enabled = true,
    seconds = 90,
    now = () => Date.now(),
    openRoom = async () => {
      throw new Error("openRoom is not configured");
    },
    newTicket = () => randomBytes(32).toString("hex"),
    sessionFields = {},
    onSession = () => {},
  } = options;

  function available() {
    return enabled;
  }

  function buildSession(endsAt, token, ticket) {
    return {
      serverUrl: sessionFields.serverUrl,
      token,
      endsAt,
      preview: true,
      ticket,
      avatarProvider: sessionFields.avatarProvider,
      ...(sessionFields.avatarProvider === "spatius"
        ? { spatiusAppId: sessionFields.spatiusAppId, spatiusAvatarId: sessionFields.spatiusAvatarId }
        : {}),
    };
  }

  /**
   * Synchronously decide whether a preview may open. The disabled preview is the
   * only refusal left, so no refusal needs a visitor identity to explain itself.
   * A room that fails to open is not recorded anywhere: the error propagates to
   * the caller and the next attempt is unaffected.
   */
  function begin() {
    if (!enabled) return { ok: false, status: 403, error: "Preview is disabled" };
    return { ok: true, endsAt: Math.floor(now() / 1000) + seconds };
  }

  async function handle() {
    const decision = begin();
    if (!decision.ok) return { status: decision.status, body: { error: decision.error } };
    const { roomName, token } = await openRoom({ endsAt: decision.endsAt, preview: true });
    const ticket = newTicket();
    onSession(ticket, { roomName, endsAt: decision.endsAt, preview: true });
    return { status: 200, body: buildSession(decision.endsAt, token, ticket) };
  }

  return { available, begin, handle };
}
