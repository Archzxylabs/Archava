import { useEffect, useRef, useState } from "react";
import { useRoomContext } from "@livekit/components-react";
import { AvatarView } from "@spatius/avatarkit";
import { AvatarPlayer, LiveKitProvider } from "@spatius/avatarkit-rtc";
import { type AvatarSession } from "../lib/rental";
import { prepareSpatiusAvatar } from "../lib/spatius";

interface SpatiusAvatarProps {
  session: AvatarSession;
}

export function SpatiusAvatar({ session }: SpatiusAvatarProps) {
  const room = useRoomContext();
  const containerRef = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    let connectionAttempted = false;
    let disposed = false;
    let view: AvatarView | undefined;
    let player: AvatarPlayer | undefined;

    const start = async () => {
      const avatar = await prepareSpatiusAvatar(session.spatiusAppId || "", session.spatiusAvatarId || "");
      if (cancelled || !containerRef.current) return;

      containerRef.current.replaceChildren();
      view = new AvatarView(avatar, containerRef.current);
      view.onFirstRendering = () => { if (!cancelled) setReady(true); };
      player = new AvatarPlayer(new LiveKitProvider(), view);
      await player.attach(room);
      if (cancelled) return;

      connectionAttempted = true;
      await room.connect(session.serverUrl, session.token);
      if (cancelled) return;
      await room.localParticipant.setMicrophoneEnabled(true);
    };

    const dispose = async () => {
      if (disposed) return;
      disposed = true;
      await player?.detach().catch(() => {});
      if (connectionAttempted) await room.disconnect().catch(() => {});
      view?.dispose();
    };

    const startup = start().catch(async (cause: unknown) => {
      await dispose();
      if (!cancelled) {
        setError(cause instanceof Error ? cause.message : "Could not display the Spatius avatar.");
      }
    });
    return () => {
      cancelled = true;
      void startup.then(dispose);
    };
  }, [room, session.serverUrl, session.token, session.spatiusAppId, session.spatiusAvatarId]);

  return (
    <div className="spatius-avatar-surface">
      <div className="spatius-avatar-canvas" ref={containerRef} />
      {!ready && (
        <div className="spatius-avatar-status" role={error ? "alert" : "status"}>
          <span className="live-pulse-dot" />
          <span>{error || "Preparing Ava's live avatar…"}</span>
        </div>
      )}
    </div>
  );
}
