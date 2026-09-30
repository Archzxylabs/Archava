import { ApiError } from "./apiError.ts";
import { injectedProvider } from "./rental.ts";

/**
 * Client for the wallet-authenticated developer dashboard.
 *
 * The dashboard cookie is HttpOnly and owned by the server, so every call here
 * rides on `credentials: "same-origin"` and nothing is written to storage. The
 * API key secret exists only in the response to the create call and is handed
 * straight to React state by the caller — this module never persists it.
 */

/** Key metadata. Deliberately has no `apiKey` field: the secret is unreturnable. */
export interface DeveloperKey {
  id: string;
  label: string;
  prefix: string;
  createdAt: string;
  lastUsedAt: string | null;
  revokedAt: string | null;
}

export type DeveloperKeyStatus = "active" | "revoked";

export interface DeveloperChallenge {
  nonce: string;
  message: string;
}

export interface DeveloperWallet {
  wallet: string;
}

/** The single moment the secret is visible. */
export interface CreatedDeveloperKey {
  id: string;
  apiKey: string;
  label: string;
  prefix: string;
  createdAt: string;
}

/** Usage as the server reports it; see `fetchDeveloperUsage` for what is readable. */
export interface DeveloperUsage {
  allocatedSeconds: number;
  sessionCount: number;
  balanceAvailable: boolean;
  purchasedMinutes?: number;
  remainingSeconds?: number;
  reservedSeconds?: number;
}

interface DeveloperKeyResponse {
  keys?: DeveloperKey[];
}

/** The usage payload exactly as it arrives: nothing is trusted until it is read. */
interface DeveloperUsageResponse {
  allocatedSeconds?: unknown;
  sessionCount?: unknown;
  balanceAvailable?: unknown;
  purchasedMinutes?: unknown;
  remainingSeconds?: unknown;
  reservedSeconds?: unknown;
}

/**
 * Usage cannot be shown as a number. Deliberately not an `ApiError`: an HTTP
 * status never describes a well-formed response whose body is unusable, so it
 * must not be reported as a transport or session problem.
 */
export class DeveloperUsageUnavailableError extends Error {
  constructor(message = "Usage could not be read for this wallet, so no session time is shown.") {
    super(message);
    this.name = "DeveloperUsageUnavailableError";
  }
}

/** A whole, finite number or nothing. `NaN`, strings, and `null` are nothing. */
function readCount(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

async function developerApi<T>(path: string, init: { method?: string; body?: unknown } = {}): Promise<T> {
  const hasBody = init.body !== undefined;
  const response = await fetch(path, {
    method: init.method ?? "GET",
    credentials: "same-origin",
    headers: hasBody ? { "Content-Type": "application/json" } : undefined,
    body: hasBody ? JSON.stringify(init.body) : undefined,
  });
  const data = (await response.json().catch(() => ({}))) as { error?: string };
  if (!response.ok) throw new ApiError(response.status, data.error || "Developer request failed");
  return data as T;
}

export async function requestDeveloperChallenge(wallet: string): Promise<DeveloperChallenge> {
  return developerApi<DeveloperChallenge>("/api/developer/challenge", { method: "POST", body: { wallet } });
}

/** Signs the purpose-bound challenge with the already-connected wallet. */
export async function signChallengeMessage(wallet: string, message: string): Promise<string> {
  const provider = injectedProvider();
  const signer = await provider.getSigner();
  const address = await signer.getAddress();
  if (address.toLowerCase() !== wallet.toLowerCase()) {
    throw new Error("Connected wallet changed. Connect it again to continue.");
  }
  return signer.signMessage(message);
}

export async function loginDeveloper(wallet: string, nonce: string, signature: string): Promise<DeveloperWallet> {
  return developerApi<DeveloperWallet>("/api/developer/login", {
    method: "POST",
    body: { wallet, nonce, signature },
  });
}

/** Challenge → signature → dashboard cookie. Called once per dashboard session. */
export async function authenticateDeveloper(wallet: string): Promise<DeveloperWallet> {
  const challenge = await requestDeveloperChallenge(wallet);
  const signature = await signChallengeMessage(wallet, challenge.message);
  return loginDeveloper(wallet, challenge.nonce, signature);
}

export async function fetchDeveloperWallet(): Promise<DeveloperWallet> {
  return developerApi<DeveloperWallet>("/api/developer/me");
}

export async function logoutDeveloper(): Promise<void> {
  await developerApi<{ ok: true }>("/api/developer/logout", { method: "POST" });
}

/**
 * Metadata-only listing. The response is rebuilt field by field, so an extra
 * `apiKey` the server may one day leak into this payload is dropped here rather
 * than rendered.
 */
export async function listDeveloperKeys(): Promise<DeveloperKey[]> {
  const data = await developerApi<DeveloperKeyResponse>("/api/developer/keys");
  return (data.keys ?? []).map((key) => ({
    id: key.id,
    label: key.label,
    prefix: key.prefix,
    createdAt: key.createdAt,
    lastUsedAt: key.lastUsedAt ?? null,
    revokedAt: key.revokedAt ?? null,
  }));
}

export async function createDeveloperKey(label: string): Promise<CreatedDeveloperKey> {
  const result = await developerApi<CreatedDeveloperKey>("/api/developer/keys", { method: "POST", body: { label } });
  if (typeof result.apiKey !== "string" || result.apiKey.length === 0) {
    // Fail closed: a created key whose secret never arrived must not leave the
    // dashboard looking like it is usable.
    throw new Error("The server did not return the new key. Nothing was saved — try creating it again.");
  }
  return result;
}

export async function revokeDeveloperKey(id: string): Promise<void> {
  await developerApi<{ ok: true }>(`/api/developer/keys/${encodeURIComponent(id)}/revoke`, { method: "POST" });
}

/**
 * Usage is reported by the server; the dashboard displays it, never estimates it.
 *
 * `allocatedSeconds` is allocated room time: it runs from the moment a room is
 * created until that room ends or expires, so it includes the wait before any
 * participant joins. A payload that is missing a number, or carries a value
 * JavaScript cannot count with (`NaN`, `Infinity`, a string), is refused rather
 * than coerced — `0` and `NaN` both render as real answers on screen, and a
 * wrong number here would understate what the key has actually spent.
 */
export async function fetchDeveloperUsage(): Promise<DeveloperUsage> {
  const data = await developerApi<DeveloperUsageResponse>("/api/developer/usage");
  const allocatedSeconds = readCount(data?.allocatedSeconds);
  const sessionCount = readCount(data?.sessionCount);
  if (allocatedSeconds === null || sessionCount === null) {
    throw new DeveloperUsageUnavailableError();
  }
  if (data.balanceAvailable === true) {
    const purchasedMinutes = readCount(data.purchasedMinutes);
    const remainingSeconds = readCount(data.remainingSeconds);
    const reservedSeconds = readCount(data.reservedSeconds);
    if (purchasedMinutes === null || remainingSeconds === null || reservedSeconds === null) {
      throw new DeveloperUsageUnavailableError();
    }
    return { allocatedSeconds, sessionCount, balanceAvailable: true, purchasedMinutes, remainingSeconds, reservedSeconds };
  }
  return { allocatedSeconds, sessionCount, balanceAvailable: false };
}

export function developerKeyStatus(key: DeveloperKey): DeveloperKeyStatus {
  return key.revokedAt ? "revoked" : "active";
}

/**
 * Renders allocated session seconds as `12 min 05 s`. A non-finite input renders
 * as an em dash, because a read that could not be measured must never look like
 * a measurement of zero.
 */
export function formatAllocatedSessionTime(seconds: number): string {
  if (!Number.isFinite(seconds)) return "—";
  const safe = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(safe / 60);
  const rest = safe % 60;
  return `${minutes} min ${String(rest).padStart(2, "0")} s`;
}

const DEVELOPER_MESSAGES: Record<number, string> = {
  400: "That key request was rejected. Check the label and try again.",
  401: "The dashboard session expired. Sign the developer challenge again.",
  404: "The developer API is not available yet. Please check back shortly.",
  409: "Key limit reached. Revoke an unused key first.",
  429: "Too many requests. Wait a moment before trying again.",
  503: "The developer API is warming up. Please try again shortly.",
};

const DEVELOPER_FALLBACK = "Developer dashboard is temporarily unavailable. Please try again.";

export function developerErrorMessage(error: unknown): string {
  if (error instanceof DeveloperUsageUnavailableError) return error.message;
  if (!(error instanceof ApiError)) return DEVELOPER_FALLBACK;
  return DEVELOPER_MESSAGES[error.status] || error.message || DEVELOPER_FALLBACK;
}

/** True when the dashboard cookie is missing or stale and a new signature is needed. */
export function needsDeveloperSignIn(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}
