import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import test from "node:test";
import { nextRevision, receiveAck, type PageSyncState } from "./siteAwareness.ts";

const encoder = new TextEncoder();

interface WorkerResult {
  /** True once any packet has been accepted, mirroring SiteContext.section. */
  accepted: boolean;
  /** The pair the worker currently holds — what it would acknowledge. */
  ack: { section: string; revision: number } | null;
}

/**
 * Run the real worker-side SiteContext over some packets and return the pair it
 * ends up holding. This is the only cross-language boundary in the feature, so
 * the bytes the browser publishes are handed to the actual Python module rather
 * than to a mock of it.
 */
function workerAck(packets: unknown[]): WorkerResult {
  const script = `
import json, sys
sys.path.insert(0, ${JSON.stringify(new URL("../../backend", import.meta.url).pathname)})
from site_context import PAGE_TOPIC, SiteContext
site = SiteContext()
for packet in json.load(sys.stdin):
    site.accept_packet(PAGE_TOPIC, packet.encode("utf-8"), "guest-roundtrip")
print(json.dumps({
    "accepted": site.section is not None,
    "ack": None if site.section is None else {"section": site.section, "revision": site.revision},
}))
`;
  const stdout = execFileSync("python3", ["-c", script], {
    input: JSON.stringify(packets.map((packet) => JSON.stringify(packet))),
    encoding: "utf8",
  });
  return JSON.parse(stdout);
}

test("a browser publish round-trips through the worker's SiteContext into the label", () => {
  const revision = nextRevision(0);
  const bytes = encoder.encode(JSON.stringify({ section: "protocol-section", revision }));
  assert.ok(bytes.byteLength < 128, "the browser packet must stay inside the worker's size limit");

  const worker = workerAck([{ section: "protocol-section", revision }]);
  assert.equal(worker.accepted, true);
  assert.deepEqual(worker.ack, { section: "protocol-section", revision });

  const sent: PageSyncState = {
    section: "protocol-section",
    pending: { section: "protocol-section", revision },
    confirmed: null,
  };
  const confirmed = receiveAck(sent, encoder.encode(JSON.stringify(worker.ack)));
  assert.deepEqual(confirmed.confirmed, { section: "protocol-section", revision });
});

test("a stale browser publish is dropped by the worker and confirms nothing", () => {
  const worker = workerAck([
    { section: "protocol-section", revision: 50 },
    { section: "top", revision: 49 },
  ]);
  assert.equal(worker.accepted, true);
  assert.deepEqual(
    worker.ack,
    { section: "protocol-section", revision: 50 },
    "the older resend must not replace the accepted section",
  );
  // The browser's pending send is the stale one, so the only ack the worker
  // could produce belongs to a different send and cannot confirm it.

  const sent: PageSyncState = { section: "top", pending: { section: "top", revision: 49 }, confirmed: null };
  const after = receiveAck(sent, encoder.encode(JSON.stringify(worker.ack)));
  assert.equal(after.confirmed, null);
  assert.deepEqual(after.pending, { section: "top", revision: 49 });
});

test("a packet carrying page text is refused by the worker before any ack exists", () => {
  const worker = workerAck([{ section: "top", revision: 1, siteText: "Buy now, discount inside" }]);
  assert.equal(worker.accepted, false);
  assert.equal(worker.ack, null);
});

test("the worker's ack packet stays small enough for the browser to trust", () => {
  const revision = nextRevision(0);
  const worker = workerAck([{ section: "business-section", revision }]);
  assert.equal(worker.accepted, true);
  assert.ok(encoder.encode(JSON.stringify(worker.ack)).byteLength <= 128, "ack exceeded the browser's size guard");
});
