import { ApiError } from "./apiError.ts";
import type { AppConfig } from "./rental.ts";

export const SPATIUS_UNSUPPORTED_MESSAGE =
  "This browser cannot display the live avatar. Open this page in a recent Chrome or Edge browser to try the demo.";

export function previewDurationLabel(seconds?: number): string {
  if (!seconds || seconds <= 0) return "Short demo";
  return seconds % 60 === 0 ? `${seconds / 60}-minute demo` : `${seconds}-second demo`;
}

const UNAVAILABLE_BY_STATUS: Record<number, string> = {
  403: "The live demo is not open right now. Please try again later.",
  404: "The live demo is not available yet. Please check back shortly.",
  503: "The live demo is warming up. Please try again in a moment.",
};

const FALLBACK_MESSAGE = "Could not open the live demo. Please try again.";

export function isPreviewAvailable(config: AppConfig | null): boolean {
  return Boolean(config?.previewEnabled && config.livekitReady);
}

export function supportsAvatarRendering(config: AppConfig | null): boolean {
  return config?.avatarProvider !== "spatius" || "RTCRtpScriptTransform" in globalThis;
}

export function previewErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) return FALLBACK_MESSAGE;
  return UNAVAILABLE_BY_STATUS[error.status] || error.message || FALLBACK_MESSAGE;
}
