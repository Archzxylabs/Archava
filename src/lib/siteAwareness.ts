export const siteSections = [
  { id: "top", label: "Home" },
  { id: "catalog-section", label: "The avatar" },
  { id: "protocol-section", label: "How it works" },
  { id: "business-section", label: "For business" },
] as const;

export type SiteSection = (typeof siteSections)[number]["id"];

/** Browser → worker: which allowlisted section the visitor is reading. */
export const PAGE_TOPIC = "archava.page";
/** Worker → browser: the section/revision pair the worker accepted. */
export const PAGE_ACK_TOPIC = "archava.page.ack";
/** Worker → browser: scroll the visitor to an allowlisted section. */
export const NAVIGATION_TOPIC = "archava.navigation";

/** The section the browser is waiting on an acknowledgement for. */
export interface PageSync {
  section: SiteSection;
  revision: number;
}

/** The browser's page-context state: what is on screen, in flight, and acknowledged. */
export interface PageSyncState {
  section: SiteSection;
  pending: PageSync | null;
  confirmed: PageSync | null;
}

const MAX_PAGE_PACKET_BYTES = 128;
const RETRY_DELAY_MS = 250;
const MAX_RETRY_DELAY_MS = 4000;

export function currentSiteSection(): SiteSection {
  const readingLine = window.innerHeight * 0.4;
  let current: SiteSection = "top";
  for (const section of siteSections) {
    const element = document.getElementById(section.id);
    if (element && element.getBoundingClientRect().top <= readingLine) current = section.id;
  }
  return current;
}

export function isSiteSection(value: unknown): value is SiteSection {
  return siteSections.some((section) => section.id === value);
}

/**
 * The worker rejects anything at or below its highest revision, so the next
 * revision must strictly beat both the previous one and the wall clock. Reading
 * Date.now() alone lets a clock adjustment or a same-millisecond resend land on
 * a revision the worker has already stored.
 */
export function nextRevision(previous: number, now: number = Date.now()): number {
  return Math.max(now, previous + 1);
}

/** The confirm label is claimed only for the exact pair still awaiting an ack. */
export function isFreshAck(payload: { section?: unknown; revision?: unknown }, pending: PageSync | null): boolean {
  if (!pending) return false;
  if (!isSiteSection(payload?.section)) return false;
  if (typeof payload?.revision !== "number" || !Number.isInteger(payload.revision)) return false;
  return payload.section === pending.section && payload.revision === pending.revision;
}

/**
 * An acknowledgement answers with the accepted pair only. A packet carrying any
 * other key is dropped rather than partially trusted, so page text or injection
 * attempts riding alongside a valid-looking section never reach the label.
 */
export function parsePageAck(data: Uint8Array): PageSync | null {
  if (data.byteLength > MAX_PAGE_PACKET_BYTES) return null;
  let payload: unknown;
  try {
    payload = JSON.parse(new TextDecoder().decode(data));
  } catch {
    return null;
  }
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return null;
  if (Object.keys(payload as Record<string, unknown>).length !== 2) return null;
  const fields = payload as Record<string, unknown>;
  if (!("section" in fields) || !("revision" in fields)) return null;
  if (!isSiteSection(fields.section)) return null;
  const revision = fields.revision;
  if (typeof revision !== "number" || !Number.isInteger(revision) || revision < 0) return null;
  return { section: fields.section, revision };
}

/** Bounded, jitter-free backoff for a publish the room refused or dropped. */
export function publishRetryDelay(attempt: number): number {
  return Math.min(MAX_RETRY_DELAY_MS, RETRY_DELAY_MS * 2 ** attempt);
}

/**
 * Publish the reading section. A repeat of the section already on screen is a
 * no-op, so scroll jitter inside one section costs one packet. A forced send
 * (agent rejoin, navigation settle, publish retry) always re-asks, which also
 * withdraws the previous confirmation — the worker must answer again before the
 * label may claim anything.
 */
export function beginSend(current: PageSyncState, section: SiteSection, revision: number, force = false): PageSyncState {
  if (!force && section === current.section) return current;
  return { section, pending: { section, revision }, confirmed: null };
}

/**
 * Record an acknowledgement. It only counts when it answers the pair still in
 * flight, so a late packet from an earlier send cannot confirm a newer section.
 */
export function receiveAck(current: PageSyncState, data: Uint8Array): PageSyncState {
  const ack = parsePageAck(data);
  if (!current.pending || !ack || !isFreshAck(ack, current.pending)) return current;
  return { ...current, confirmed: current.pending, pending: null };
}

/** The label may only claim the section once the ack for the current send lands. */
export function isAcknowledged(current: PageSyncState): boolean {
  return current.confirmed !== null && current.confirmed.section === current.section;
}

/**
 * The publish was dropped, so nothing is in flight and nothing may be claimed.
 * The stored ack is discarded with the pending send — a packet the browser
 * believes never reached the worker proves nothing about it.
 */
export function dropAckExpectation(current: PageSyncState): PageSyncState {
  return { section: current.section, pending: null, confirmed: null };
}
