import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import test from "node:test";

// Contract test for the page-awareness label. It guards the coupling between the
// state machine in src/lib/siteAwareness.ts and the two strings the visitor
// reads, which is exactly where this would break: a component that derives the
// label from the section it happens to be rendering, rather than from the ack it
// received, claims Ava knows the page the moment anything scrolls.

const component = readFileSync(new URL("./SiteAwareness.tsx", import.meta.url), "utf8");
const lib = readFileSync(new URL("../lib/siteAwareness.ts", import.meta.url), "utf8");

test("the confirm label is gated on an acknowledged send, not on the section being displayed", () => {
  assert.match(component, /const confirmed = confirmedSection === section && isAcknowledged\(page\)/);
  // The label may never read the raw DOM section.
  assert.doesNotMatch(component, /confirmedSection === currentSiteSection\(\)/);
  assert.doesNotMatch(component, /=== currentSiteSection\(\)/, "the label must not be derived from the live DOM section");
});

test("the dot and the label agree on what counts as confirmed", () => {
  const dot = component.match(/className=\{"live-site-awareness-dot" \+ \((confirmed)\s*\?/);
  assert.ok(dot, "the animated dot must be driven by the same signal as the text");
  assert.match(component, /\{confirmed \? "AVA HAS THIS SECTION" : "SYNCING PAGE CONTEXT"\}/);
  assert.match(component, /const confirmed = confirmedSection === section && isAcknowledged\(page\);/);
});

test("the confirmation is withdrawn whenever the reading section changes", () => {
  // beginSend clears `confirmed` in the shared state machine, and the component
  // must also clear its own label state, otherwise the old section's label
  // survives the scroll into the new one.
  assert.match(lib, /return \{ section, pending: \{ section, revision \}, confirmed: null \};/);
  assert.match(component, /setConfirmedSection\(null\)/);
});

test("a dropped publish withdraws the claim and is retried", () => {
  assert.match(component, /setPage\(\(current\) => dropAckExpectation\(current\)\)/);
  assert.match(component, /publishRetryDelay\(attempts\.current\)/);
  assert.match(component, /MAX_PUBLISH_ATTEMPTS = 4/);
});

test("a reconnecting agent forces a resend so confirmation is re-earned", () => {
  const rejoin = component.match(/onParticipantConnected[\s\S]{0,160}attempts\.current = 0;\s*\n\s*sendPage\(true\)/);
  assert.ok(rejoin, "the agent-joined handler must reset retries and force a resend");
});

test("the ack is matched against the send it answered, not the section now on screen", () => {
  const handler = component.match(/onDataReceived[\s\S]*?window\.addEventListener\("scroll"/);
  assert.ok(handler, "the data handler must be present");
  const body = handler[0];
  // The ack branch routes the packet through the state machine, which compares
  // it to the pair still in flight. Anything derived from the live DOM — the
  // section on screen when the packet arrives — is a different question.
  const ackBranch = body.slice(body.indexOf("topic === PAGE_ACK_TOPIC"), body.indexOf("topic !== NAVIGATION_TOPIC"));
  assert.ok(ackBranch.length > 0, "the ack topic must be handled");
  assert.match(ackBranch, /setPage\(\(current\) => \{[\s\S]*receiveAck\(current, data\)/);
  assert.match(ackBranch, /receiveAck\(current, data\)/);
  assert.doesNotMatch(body, /currentSiteSection\(\)/, "the ack handler must not read the live DOM section");
  assert.match(body, /if \(!participant\?\.isAgent\) return;/, "non-agent packets are ignored outright");
});

test("the publish payload carries only an allowlisted section and a revision", () => {
  const publish = component.match(/publishData\(encoder\.encode\(JSON\.stringify\(\{([^}]*)\}\)\)/);
  assert.ok(publish, "the publish payload must be an inline literal");
  assert.equal(publish[1].trim(), "section: next, revision: latestRevision");
});
