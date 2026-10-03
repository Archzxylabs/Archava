import { useEffect, useRef, useState } from "react";
import { useConnectionState, useRoomContext } from "@livekit/components-react";
import { ConnectionState, RoomEvent } from "livekit-client";
import {
  beginSend,
  currentSiteSection,
  dropAckExpectation,
  isAcknowledged,
  isSiteSection,
  NAVIGATION_TOPIC,
  nextRevision,
  PAGE_ACK_TOPIC,
  PAGE_TOPIC,
  publishRetryDelay,
  receiveAck,
  siteSections,
  type PageSyncState,
  type SiteSection,
} from "../lib/siteAwareness";

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const MAX_PUBLISH_ATTEMPTS = 4;

export function SiteAwareness() {
  const room = useRoomContext();
  const connection = useConnectionState();
  const [section, setSection] = useState<SiteSection>(currentSiteSection);
  const [page, setPage] = useState<PageSyncState>(() => ({
    section: currentSiteSection(),
    pending: null,
    confirmed: null,
  }));
  const [confirmedSection, setConfirmedSection] = useState<SiteSection | null>(null);
  const retry = useRef<ReturnType<typeof setTimeout>>(undefined);
  const attempts = useRef(0);

  useEffect(() => () => { clearTimeout(retry.current); }, []);

  useEffect(() => {
    if (connection !== ConnectionState.Connected) {
      setConfirmedSection(null);
      return;
    }
    let lastSent: SiteSection | null = null;
    let latestRevision = 0;
    let frame = 0;
    let disposed = false;

    const sendPage = (force = false) => {
      const next = currentSiteSection();
      setSection(next);
      if (!force && next === lastSent) return;
      lastSent = next;
      latestRevision = nextRevision(latestRevision);
      setConfirmedSection(null);
      setPage((current) => beginSend(current, next, latestRevision, true));
      void room.localParticipant.publishData(encoder.encode(JSON.stringify({ section: next, revision: latestRevision })), {
        reliable: true,
        topic: PAGE_TOPIC,
      }).catch(() => {
        lastSent = null;
        setPage((current) => dropAckExpectation(current));
        clearTimeout(retry.current);
        if (attempts.current < MAX_PUBLISH_ATTEMPTS && !disposed) {
          retry.current = setTimeout(() => sendPage(true), publishRetryDelay(attempts.current));
          attempts.current += 1;
        }
      });
    };

    const schedulePage = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => sendPage());
    };

    const onParticipantConnected = (participant: { isAgent: boolean }) => {
      if (participant.isAgent) {
        attempts.current = 0;
        sendPage(true);
      }
    };

    // The ack is matched against the send it answers, not against what is on
    // screen right now: a packet that arrives after the visitor has already
    // scrolled must not confirm the new section.
    const onDataReceived = (data: Uint8Array, participant: { isAgent: boolean } | undefined, _kind: unknown, topic?: string) => {
      if (!participant?.isAgent) return;
      if (topic === PAGE_ACK_TOPIC) {
        setPage((current) => {
          const next = receiveAck(current, data);
          if (next.confirmed) setConfirmedSection(next.confirmed.section);
          return next;
        });
        return;
      }
      if (topic !== NAVIGATION_TOPIC || data.byteLength > 128) return;
      try {
        const payload = JSON.parse(decoder.decode(data));
        if (!isSiteSection(payload?.section)) return;
        document.getElementById(payload.section)?.scrollIntoView({ behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth", block: "start" });
        window.setTimeout(() => sendPage(true), 700);
      } catch { /* Ignore malformed packets. */ }
    };

    window.addEventListener("scroll", schedulePage, { passive: true });
    window.addEventListener("resize", schedulePage);
    window.addEventListener("hashchange", schedulePage);
    room.on(RoomEvent.ParticipantConnected, onParticipantConnected);
    room.on(RoomEvent.DataReceived, onDataReceived);
    sendPage(true);

    return () => {
      disposed = true;
      clearTimeout(retry.current);
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener("scroll", schedulePage);
      window.removeEventListener("resize", schedulePage);
      window.removeEventListener("hashchange", schedulePage);
      room.off(RoomEvent.ParticipantConnected, onParticipantConnected);
      room.off(RoomEvent.DataReceived, onDataReceived);
    };
  }, [connection, room]);

  const confirmed = confirmedSection === section && isAcknowledged(page);

  return (
    <div className="live-site-awareness" aria-live="polite">
      <span className={"live-site-awareness-dot" + (confirmed ? " confirmed" : "")} />
      <span>{confirmed ? "AVA HAS THIS SECTION" : "SYNCING PAGE CONTEXT"}</span>
      <strong>{siteSections.find((item) => item.id === section)?.label}</strong>
    </div>
  );
}
