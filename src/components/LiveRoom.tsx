import { lazy, Suspense, useEffect, useState } from "react";
import { Clock3, Mic, MicOff, PhoneOff, Shield, Volume2 } from "lucide-react";
import {
  BarVisualizer,
  DisconnectButton,
  isTrackReference,
  StartAudio,
  useLocalParticipant,
  useTracks,
  useVoiceAssistant,
  VideoTrack,
} from "@livekit/components-react";
import { Track } from "livekit-client";
import { type AvatarSession } from "../lib/rental";
import { sounds } from "../lib/sound";
import { SiteAwareness } from "./SiteAwareness";
import { LiveCallDock } from "./LiveCallDock";

const SpatiusAvatar = lazy(() => import("./SpatiusAvatar").then((module) => ({ default: module.SpatiusAvatar })));

interface LiveRoomProps {
  session: AvatarSession;
  onClose: () => void;
}

const timeLeft = (seconds: number) => {
  const left = Math.max(0, seconds - Math.floor(Date.now() / 1000));
  return String(Math.floor(left / 60)).padStart(2, "0") + ":" + String(left % 60).padStart(2, "0");
};

/** Every LiveKit agent state, so a new SDK state cannot silently fall through. */
type AgentState = ReturnType<typeof useVoiceAssistant>["state"];

/** Motion class on the viewport. Each phase has its own CSS treatment. */
type CallPhase = "speaking" | "thinking" | "listening" | "standby" | "failed";

const CALL_PHASES: Record<AgentState, CallPhase> = {
  speaking: "speaking",
  thinking: "thinking",
  listening: "listening",
  idle: "standby",
  disconnected: "standby",
  "pre-connect-buffering": "standby",
  initializing: "standby",
  connecting: "standby",
  failed: "failed",
};

const STATE_LABELS: Record<AgentState, string> = {
  speaking: "AVA IS SPEAKING",
  thinking: "AVA IS THINKING",
  listening: "AVA IS LISTENING",
  idle: "AVA READY",
  disconnected: "CONNECTING AVA",
  "pre-connect-buffering": "CONNECTING AVA",
  initializing: "CONNECTING AVA",
  connecting: "CONNECTING AVA",
  failed: "AVA OFFLINE",
};

/**
 * Conversation starters for the short visitor call. Ava answers these from her
 * own briefing, so every prompt on screen is one she can actually answer.
 */
const EXAMPLE_QUESTIONS = [
  "What can you do?",
  "Tell me about this page.",
  "How could my business use you?",
] as const;

export function LiveRoom({ session, onClose }: LiveRoomProps) {
  const tracks = useTracks([Track.Source.Camera]);
  const avatarTrack =
    tracks.find((item) => isTrackReference(item) && item.participant.identity === "tavus-avatar-agent") ??
    tracks.find((item) => isTrackReference(item) && item.participant.isAgent);

  const { state, audioTrack } = useVoiceAssistant();
  const { localParticipant } = useLocalParticipant();
  const [remaining, setRemaining] = useState(timeLeft(session.endsAt));
  const [micError, setMicError] = useState("");
  // One starter rotates out every few seconds so the visitor always has an
  // example on screen without hiding the call.
  const [promptIndex, setPromptIndex] = useState(0);
  const isMuted = !localParticipant?.isMicrophoneEnabled;
  const callPhase: CallPhase = CALL_PHASES[state];
  const stateLabel = STATE_LABELS[state];

  useEffect(() => {
    document.body.classList.add("has-live-call");
    return () => document.body.classList.remove("has-live-call");
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setRemaining(timeLeft(session.endsAt));
      if (session.endsAt <= Math.floor(Date.now() / 1000)) {
        onClose();
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [session.endsAt, onClose]);

  useEffect(() => {
    const rotate = window.setInterval(() => {
      setPromptIndex((current) => (current + 1) % EXAMPLE_QUESTIONS.length);
    }, 6000);
    return () => window.clearInterval(rotate);
  }, []);

  const toggleMic = async () => {
    try {
      sounds.playClick();
      setMicError("");
      const targetState = !localParticipant.isMicrophoneEnabled;
      await localParticipant.setMicrophoneEnabled(targetState);
    } catch {
      setMicError("Microphone permission required for conversation.");
    }
  };

  const handleEnd = () => {
    sounds.playClick();
    onClose();
  };

  return (
    <div className="live-room-container">
      {/* Sci-Fi HUD Header */}
      <div className="live-hud-top">
        <div className="live-status-group">
          <span className="live-pulse-dot" />
          <span className="live-status-title">
            {session.preview ? "DEMO PREVIEW SESSION" : "VERIFIED ONCHAIN SESSION"}
          </span>
        </div>

        <div className="live-timer-badge">
          <Clock3 size={13} />
          <span className="timer-digits">{remaining}</span>
        </div>
      </div>

      {/* Main Video Viewport with HUD Brackets */}
      <div className={`live-video-viewport call-phase-${callPhase}`}>
        {/* Corner HUD Reticles */}
        <div className="hud-corner-bracket top-left" />
        <div className="hud-corner-bracket top-right" />
        <div className="hud-corner-bracket bottom-left" />
        <div className="hud-corner-bracket bottom-right" />

        {session.avatarProvider === "spatius" ? (
          <Suspense fallback={<div className="avatar-connecting-screen">Loading avatar renderer…</div>}>
            <SpatiusAvatar session={session} />
          </Suspense>
        ) : avatarTrack && isTrackReference(avatarTrack) ? (
          <VideoTrack trackRef={avatarTrack} className="avatar-video-feed" />
        ) : (
          <div className="avatar-connecting-screen">
            <div className="connecting-radar">
              <div className="radar-circle-1" />
              <div className="radar-circle-2" />
              <div className="radar-sweep" />
              <span className="brand-mark brand-mark-small">
                <span className="brand-mark-cut" />
              </span>
            </div>
            <h4 className="connecting-title">BRINGING AVA ONLINE</h4>
            <p className="connecting-sub">Connecting avatar video and realtime voice…</p>
            <div className="connecting-tags">
              <span>LIVEKIT ROOM</span>
              <span>AI-GENERATED AVATAR</span>
            </div>
          </div>
        )}

        <div className="live-call-motion" aria-hidden="true" data-active={callPhase}>
          <span className="call-scan-beam" />
          <span className="call-orbit"><span /><span /><span /><i /></span>
          <span className="call-signal-caption">ARCHAVA / LIVE PRESENCE</span>
        </div>

        {/* Floating Voice State Indicator */}
        <div className="floating-agent-state" role="status" aria-live="polite">
          <div className="agent-indicator-pill">
            <span className="indicator-glow-dot" />
            <span className="indicator-text">
              {stateLabel}
            </span>
          </div>
        </div>
      </div>

      {/* Live Controls Deck */}
      <div className="live-controls-deck">
        <div className="audio-visualizer-group">
          <div className="mini-voice-viz">
            <BarVisualizer state={state} barCount={12} trackRef={audioTrack} />
          </div>
          <span className="viz-label">
            <Volume2 size={13} /> REALTIME AUDIO FEED
          </span>
        </div>

        <div className="controls-actions-group">
          <button
            type="button"
            className={`mic-control-btn ${!isMuted ? "active" : "muted"}`}
            onClick={toggleMic}
            title={isMuted ? "Unmute microphone" : "Mute microphone"}
            aria-label={isMuted ? "Unmute microphone" : "Mute microphone"}
            aria-pressed={!isMuted}
          >
            {!isMuted ? <Mic size={17} /> : <MicOff size={17} />}
            <span>{!isMuted ? "MIC ON" : "MUTED"}</span>
          </button>

          <StartAudio label="ALLOW AUDIO" className="mic-control-btn active" />

          <DisconnectButton className="end-call-btn" onClick={handleEnd} aria-label="End conversation">
            <PhoneOff size={15} />
            <span>END CALL</span>
          </DisconnectButton>
        </div>
      </div>

      {micError && <div className="live-error-bar">{micError}</div>}

      <div className="live-guide-row">
        <SiteAwareness />
        <p className="live-example-question">
          <span className="live-example-label">TRY ASKING</span>
          <span className="live-example-text" key={EXAMPLE_QUESTIONS[promptIndex]}>&ldquo;{EXAMPLE_QUESTIONS[promptIndex]}&rdquo;</span>
        </p>
      </div>

      <div className="live-footer-note">
        <span><Shield size={11} /> {session.preview ? "Short demo preview · AI-generated avatar" : "Wallet verified · Ava minutes reserved · AI-generated avatar"}</span>
      </div>
      <LiveCallDock
        remaining={remaining}
        stateLabel={stateLabel}
        muted={isMuted}
        error={micError}
        onToggleMic={() => void toggleMic()}
        onEnd={handleEnd}
        audioAction={<StartAudio label="Allow audio" className="call-dock-audio" />}
      />
    </div>
  );
}
