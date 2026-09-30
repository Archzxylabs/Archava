import assert from "node:assert/strict";
import test from "node:test";
import { ApiError } from "./apiError.ts";
import {
  isPreviewAvailable,
  previewErrorMessage,
  supportsAvatarRendering,
  SPATIUS_UNSUPPORTED_MESSAGE,
} from "./preview.ts";
import type { AppConfig } from "./rental.ts";

function config(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    chainId: 97,
    contractAddress: "",
    livekitReady: true,
    avatarProvider: "tavus",
    previewEnabled: true,
    previewSeconds: 120,
    packMinutes: [60, 300],
    ...overrides,
  };
}

test("preview is available when enabled and LiveKit is ready", () => {
  assert.equal(isPreviewAvailable(config()), true);
});

test("preview is hidden when the server disables it", () => {
  assert.equal(isPreviewAvailable(config({ previewEnabled: false })), false);
});

test("preview is hidden when LiveKit keys are missing", () => {
  assert.equal(isPreviewAvailable(config({ livekitReady: false })), false);
  assert.equal(isPreviewAvailable(null), false);
});

test("avatar rendering is allowed outside Spatius", () => {
  assert.equal(supportsAvatarRendering(config({ avatarProvider: "tavus" })), true);
  assert.equal(supportsAvatarRendering(null), true);
});

test("Spatius rendering requires the RTP script transform", () => {
  assert.equal(supportsAvatarRendering(config({ avatarProvider: "spatius" })), false);
  assert.equal(SPATIUS_UNSUPPORTED_MESSAGE.length > 0, true);
});

test("a missing preview route reads as not available yet", () => {
  const message = previewErrorMessage(new ApiError(404, "Not found"));
  assert.equal(message, "The live demo is not available yet. Please check back shortly.");
});

test("busy and limit messages come from the server", () => {
  const busy = new ApiError(409, "Demo is busy. Please try again shortly.");
  const limit = new ApiError(429, "Demo capacity reached. Please try later.");
  assert.equal(previewErrorMessage(busy), "Demo is busy. Please try again shortly.");
  assert.equal(previewErrorMessage(limit), "Demo capacity reached. Please try later.");
});

test("disabled and warming-up demos get friendly copy", () => {
  assert.equal(previewErrorMessage(new ApiError(403, "Preview disabled")), "The live demo is not open right now. Please try again later.");
  assert.equal(previewErrorMessage(new ApiError(503, "Unavailable")), "The live demo is warming up. Please try again in a moment.");
  assert.equal(previewErrorMessage(new ApiError(401, "Unauthorized")), "Unauthorized");
  assert.equal(previewErrorMessage(new Error("offline")), "Could not open the live demo. Please try again.");
});
