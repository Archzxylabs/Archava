import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// Contract test for the call-motion layer. It guards the coupling between the
// states LiveKit hands to LiveRoom and the CSS selectors that respond to them,
// which is exactly where the first pass broke: a class emitted from TSX with no
// CSS rule behind it is invisible, and a CSS rule for a phase that LiveRoom
// never emits is dead weight.

const read = (relative: string) => readFileSync(new URL(`../${relative}`, import.meta.url), "utf8");

const liveRoomSource = read("components/LiveRoom.tsx");
const stylesSource = read("styles.css");

/** Agent states LiveKit can report, per @livekit/components-react useVoiceAssistant. */
const AGENT_STATES = [
  "speaking",
  "thinking",
  "listening",
  "idle",
  "disconnected",
  "pre-connect-buffering",
  "initializing",
  "connecting",
  "failed",
] as const;

function objectLiteralKeys(source: string, declaration: string): string[] {
  const start = source.indexOf(declaration);
  assert.notEqual(start, -1, `${declaration} must be declared in LiveRoom`);
  const body = source.slice(start, source.indexOf("};", start));
  return [...body.matchAll(/^\s{2}"?([a-z-]+)"?:\s*"[^"]*",?$/gm)].map((match) => match[1]);
}

/** Values of an object literal, e.g. which phase ids CALL_PHASES can emit. */
function objectLiteralValues(source: string, declaration: string): string[] {
  const start = source.indexOf(declaration);
  assert.notEqual(start, -1, `${declaration} must be declared in LiveRoom`);
  const body = source.slice(start, source.indexOf("};", start));
  return [...new Set(
    [...body.matchAll(/^\s{2}"?[a-z-]+"?:\s*"([^"]*)",?$/gm)].map((match) => match[1]),
  )];
}

const phasesWithCss = () =>
  new Set(
    [...stylesSource.matchAll(/\.live-video-viewport\.call-phase-([a-z]+)/g)].map((match) => match[1]),
  );

test("every LiveKit state maps to a call phase", () => {
  const mapped = objectLiteralKeys(liveRoomSource, "const CALL_PHASES");
  assert.deepEqual([...mapped].sort(), [...AGENT_STATES].sort());
});

test("every LiveKit state maps to a spoken label", () => {
  const labelled = objectLiteralKeys(liveRoomSource, "const STATE_LABELS");
  assert.deepEqual([...labelled].sort(), [...AGENT_STATES].sort());
});

test("every emitted call phase has CSS behind it", () => {
  for (const phase of objectLiteralValues(liveRoomSource, "const CALL_PHASES")) {
    assert.equal(phasesWithCss().has(phase), true, `call-phase-${phase} has no CSS rule`);
  }
});

test("no CSS rule targets a phase LiveRoom never emits", () => {
  const emitted = new Set(objectLiteralValues(liveRoomSource, "const CALL_PHASES"));
  for (const phase of phasesWithCss()) {
    assert.equal(emitted.has(phase), true, `call-phase-${phase} is styled but never emitted`);
  }
});

test("the agent state pill no longer points at a per-state class", () => {
  assert.match(liveRoomSource, /className="agent-indicator-pill"/);
  assert.equal(liveRoomSource.includes("state-${state}"), false);
});

test("connecting and failed phases still the motion down", () => {
  for (const phase of ["standby", "failed"]) {
    assert.match(
      stylesSource,
      new RegExp(`call-phase-${phase}[^{]*call-(scan-beam|orbit)`),
      `call-phase-${phase} must suppress the ambient motion`,
    );
  }
});

test("hud corner brackets render with real geometry", () => {
  for (const bracket of ["top-left", "top-right", "bottom-left", "bottom-right"]) {
    assert.match(
      stylesSource,
      new RegExp(`\\.hud-corner-bracket\\.${bracket}\\s*\\{`),
      `.hud-corner-bracket.${bracket} has no CSS rule`,
    );
  }
});

test("the state indicator is announced rather than only moving", () => {
  assert.match(liveRoomSource, /<div className="floating-agent-state" role="status" aria-live="polite">/);
});

test("the scan beam spans the viewport instead of a fixed pixel distance", () => {
  const scan = stylesSource.slice(stylesSource.indexOf("@keyframes callScan"));
  assert.match(scan, /translateY\(calc\(/);
  assert.equal(/\btranslateY:\s*560px/.test(scan), false);
});

test("motion is still disabled wholesale under prefers-reduced-motion", () => {
  assert.match(stylesSource, /@media \(prefers-reduced-motion: reduce\)[\s\S]*animation: none !important/);
});

test("mobile keeps the state pill and controls clear of each other", () => {
  const mobile = stylesSource.slice(stylesSource.indexOf("@media (max-width: 520px)"));
  assert.match(mobile, /\.floating-agent-state/);
  assert.match(mobile, /\.mini-voice-viz/);
});
