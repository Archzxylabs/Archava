import { type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ArrowUpRight, Mic, MicOff, PhoneOff } from "lucide-react";

interface LiveCallDockProps {
  remaining: string;
  stateLabel: string;
  muted: boolean;
  error: string;
  onToggleMic: () => void;
  onEnd: () => void;
  audioAction?: ReactNode;
}

export function LiveCallDock({ remaining, stateLabel, muted, error, onToggleMic, onEnd, audioAction }: LiveCallDockProps) {
  const returnToAvatar = () => {
    const target = document.getElementById("live-avatar-room");
    target?.scrollIntoView({ behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth", block: "center" });
    target?.focus({ preventScroll: true });
  };
  return createPortal(
    <aside className="live-call-dock" aria-label="Active call controls">
      <div className="call-dock-status"><span className="live-dot-green" aria-hidden="true" /><span>{stateLabel}</span><span className="call-dock-timer" aria-label="Time remaining">{remaining}</span></div>
      <div className="call-dock-actions">
        <button type="button" onClick={returnToAvatar} className="call-dock-return">Back to Ava <ArrowUpRight size={16} /></button>
        <button type="button" onClick={onToggleMic} aria-label={muted ? "Unmute microphone" : "Mute microphone"} aria-pressed={!muted} className="call-dock-mic">{muted ? <MicOff size={18} /> : <Mic size={18} />}<span>{muted ? "Mic off" : "Mic on"}</span></button>
        {audioAction}
        <button type="button" onClick={onEnd} className="call-dock-end" aria-label="End conversation"><PhoneOff size={18} /><span>End call</span></button>
      </div>
      {error && <p className="call-dock-error" role="alert">{error}</p>}
    </aside>,
    document.body,
  );
}
