import { ArrowRight, ArrowUpRight, KeyRound, Mic, RefreshCw, Wallet } from "lucide-react";
import { LiveKitRoom, RoomAudioRenderer } from "@livekit/components-react";
import { type AppConfig, type AvatarSession, type CreditStatus } from "../lib/rental";
import { isPreviewAvailable, previewDurationLabel, SPATIUS_UNSUPPORTED_MESSAGE } from "../lib/preview";
import { LiveRoom } from "./LiveRoom";

interface AvatarCatalogProps {
  config: AppConfig | null;
  configStatus: "loading" | "ready" | "error";
  avatarStatus: "idle" | "loading" | "ready" | "error";
  previewSupported: boolean;
  wallet: string;
  credits: CreditStatus | null;
  session: AvatarSession | null;
  loading: string;
  openingLabel: string;
  onConnect: () => void;
  onStartSession: () => void;
  onTryPreview: () => void;
  onCloseSession: () => void;
  onOpenDeveloper: () => void;
  onRetryConfig: () => void;
}

export function AvatarCatalog({
  config, configStatus, avatarStatus, previewSupported, wallet, credits, session,
  loading, openingLabel, onConnect, onStartSession, onTryPreview, onCloseSession,
  onOpenDeveloper, onRetryConfig,
}: AvatarCatalogProps) {
  const previewAvailable = isPreviewAvailable(config);
  const checking = configStatus === "loading";
  const hasCredits = Boolean(credits?.active);

  return (
    <section className="avatar-catalog-section" id="catalog-section" aria-labelledby="catalog-title">
      <div className="catalog-grid-container">
        <div className="catalog-left-col" data-reveal>
          <div className="brand-massive-row">
            <h2 className="catalog-brand-title" id="catalog-title">Meet Ava<span className="brand-period">.</span></h2>
          </div>
          <span className="catalog-chapter-label">ONE LIVE HOST / REALTIME CONVERSATION</span>
          <h3 className="catalog-chapter-title">A face for the conversation.</h3>
          <p className="catalog-statement-p">Speak naturally with Archava's live AI host. Ask about this page, explore what she can do, or see how an avatar could help your business.</p>

          {!session && (
            <div className="catalog-demo-entry">
              <button
                type="button"
                className="catalog-demo-btn"
                onClick={previewAvailable ? onTryPreview : onRetryConfig}
                disabled={!!loading || checking || !previewSupported}
                aria-describedby="catalog-demo-note"
              >
                {previewAvailable ? <Mic size={17} /> : <RefreshCw size={17} />}
                <span>{!previewSupported ? "Browser not supported" : loading === "preview" ? openingLabel : checking ? "Checking availability…" : previewAvailable ? "Talk to Ava" : "Check availability"}</span>
                <ArrowUpRight size={17} />
              </button>
              <p id="catalog-demo-note" className="demo-note">
                {!previewSupported
                  ? SPATIUS_UNSUPPORTED_MESSAGE
                  : checking
                    ? "Checking the live demo. No wallet or sign-in needed."
                    : previewAvailable
                      ? `${previewDurationLabel(config?.previewSeconds)} · microphone needed. No wallet or sign-in. End whenever you like.`
                      : "The live demo is currently offline. Check availability again or explore the integration guide."}
              </p>
              {previewAvailable && previewSupported && (
                <span className="avatar-preparation-note" role="status">
                  {avatarStatus === "loading" ? "Preparing Ava in the background…" : avatarStatus === "error" ? "Preparation paused. Tap Talk to Ava to retry." : avatarStatus === "ready" ? "Ava is prepared and ready to connect." : "Ava will prepare when you start."}
                </span>
              )}
            </div>
          )}

          <div className="catalog-starters">
            <span className="catalog-chapter-label">TRY ASKING</span>
            <ul aria-label="Conversation ideas">
              <li>“What can you do?”</li>
              <li>“Tell me about this page.”</li>
              <li>“How could my business use you?”</li>
            </ul>
          </div>

          <details className="catalog-paid-access">
            <summary>Use purchased minutes or API keys</summary>
            <p>Paid calls and API access use your wallet's minute balance. The free demo stays open without a wallet.</p>
            {!wallet ? (
              <button type="button" className="catalog-developer-btn" onClick={onConnect} disabled={!!loading}>
                <Wallet size={16} /><span>{loading === "connect" ? "Connecting…" : "Connect wallet for paid access"}</span>
              </button>
            ) : (
              <>
                <p>{credits ? `${Math.floor(credits.remainingSeconds / 60)}m ${credits.remainingSeconds % 60}s available` : "Checking your minute balance…"}</p>
                {hasCredits && !session && (
                  <button type="button" className="catalog-demo-btn" onClick={onStartSession} disabled={!!loading || !config?.livekitReady || !previewSupported}>
                    <Mic size={16} /><span>{loading === "session" ? openingLabel : "Start paid conversation"}</span>
                  </button>
                )}
                <button type="button" className="catalog-developer-btn" onClick={onOpenDeveloper}><KeyRound size={16} /><span>Open API key dashboard</span><ArrowUpRight size={16} /></button>
              </>
            )}
            <a className="catalog-developer-btn" href="/build#access">Explore minute packs <ArrowRight size={16} /></a>
          </details>

          <a className="catalog-business-link" href="#business-section">
            <span>Explore the possibilities</span>
            <strong>Business use cases <ArrowUpRight size={15} /></strong>
          </a>
        </div>

        <div className="catalog-right-col" data-reveal>
          {session ? (
            <div className="catalog-live-session-wrapper" id="live-avatar-room" tabIndex={-1} aria-label="Live conversation with Ava">
              <LiveKitRoom
                token={session.token}
                serverUrl={session.serverUrl}
                connect={session.avatarProvider !== "spatius"}
                audio={true}
                video={false}
                options={session.avatarProvider === "spatius" ? { singlePeerConnection: false } : undefined}
                onDisconnected={onCloseSession}
                className="livekit-root-shell"
              >
                <RoomAudioRenderer />
                <LiveRoom session={session} onClose={onCloseSession} />
              </LiveKitRoom>
            </div>
          ) : (
            <figure className="catalog-avatar-poster">
              <img src="/assets/archava_pink_clean.webp" alt="Concept artwork representing Ava, Archava's live AI host" loading="lazy" decoding="async" />
              <div className="catalog-poster-shade" aria-hidden="true" />
              <div className="catalog-poster-label"><span>AVA / ONE LIVE HOST</span><strong>Voice. Presence. Conversation.</strong></div>
              <figcaption>Concept artwork. Your live session displays the connected AI avatar.</figcaption>
            </figure>
          )}
        </div>
      </div>
    </section>
  );
}
