import { useState } from "react";
import { ArrowRight, ArrowUpRight, Check, Copy, ExternalLink, KeyRound, Mic, Wallet, X, Zap } from "lucide-react";
import { type AppConfig, type AvatarSession, type PackQuote, type CreditStatus } from "../lib/rental";
import { LiveRoom } from "./LiveRoom";
import { LiveKitRoom, RoomAudioRenderer } from "@livekit/components-react";
import { sounds } from "../lib/sound";

interface AvatarCatalogProps {
  config: AppConfig | null;
  wallet: string;
  credits: CreditStatus | null;
  quote: PackQuote | null;
  selectedPack: number;
  session: AvatarSession | null;
  loading: string;
  error: string;
  txHash: string;
  onConnect: () => void;
  onBuy: () => void;
  onClaimFaucet: () => void;
  txKind: "purchase" | "faucet";
  onSelectPack: (minutes: number) => void;
  onStartSession: () => void;
  onTryPreview: () => void;
  onCloseSession: () => void;
  onClearError: () => void;
  onOpenDeveloper: () => void;
}

const chapters = [
  {
    label: "01 / THE PRESENCE",
    title: "Meet the face behind the voice.",
    body: "Archava is one ready-to-use digital host: speak naturally, and the conversation gets a face in real time.",
  },
  {
    label: "02 / TRY IT NOW",
    title: "Talk to the live avatar first.",
    body: "Start a conversation right now, no wallet needed. Minute packs add paid access for your own calls and API key; the conversation stays live and offchain.",
  },
] as const;

export function AvatarCatalog({
  config,
  wallet,
  credits,
  quote,
  selectedPack,
  session,
  loading,
  error,
  txHash,
  onConnect,
  onBuy,
  onClaimFaucet,
  txKind,
  onSelectPack,
  onStartSession,
  onTryPreview,
  onCloseSession,
  onClearError,
  onOpenDeveloper,
}: AvatarCatalogProps) {
  const [activeCardIndex, setActiveCardIndex] = useState<0 | 1>(0);
  const [copiedTx, setCopiedTx] = useState(false);
  const hasCredits = Boolean(credits?.active);
  const canBuy = Boolean(wallet && config?.contractAddress && quote && !loading);
  const readyToTalk = Boolean(wallet && config?.livekitReady);
  const previewAvailable = Boolean(config?.previewEnabled && config.livekitReady);

  const selectCard = (index: 0 | 1) => {
    sounds.playClick();
    setActiveCardIndex(index);
  };

  const copyTx = async () => {
    if (!txHash) return;
    await navigator.clipboard.writeText(txHash);
    setCopiedTx(true);
    sounds.playSuccess();
    window.setTimeout(() => setCopiedTx(false), 2000);
  };

  const scanUrl = config?.chainId === 56
    ? "https://bscscan.com/tx/"
    : "https://testnet.bscscan.com/tx/";

  return (
    <section className="avatar-catalog-section" id="catalog-section">
      <div className="catalog-grid-container">
        <div className="catalog-left-col" data-reveal>
          <div className="brand-massive-row">
            <h2 className="catalog-brand-title">Archava<span className="brand-period">.</span></h2>
            <span className="catalog-archava-tag">THE LIVE EXPERIENCE</span>
          </div>

          <div className="catalog-drag-controls" aria-label="Experience chapters">
            <button type="button" className="drag-arrow-btn" onClick={() => selectCard(0)} aria-label="Show avatar chapter">◄</button>
            <span className="drag-label">0{activeCardIndex + 1} / 02</span>
            <button type="button" className="drag-arrow-btn" onClick={() => selectCard(1)} aria-label="Show access chapter">►</button>
          </div>

          <span className="catalog-chapter-label">{chapters[activeCardIndex].label}</span>
          <h3 className="catalog-chapter-title" key={chapters[activeCardIndex].title}>
            {chapters[activeCardIndex].title}
          </h3>
          <p className="catalog-statement-p" key={chapters[activeCardIndex].body}>
            {chapters[activeCardIndex].body}
          </p>

          {previewAvailable && !session && (
            <div className="catalog-demo-entry">
              <button type="button" className="catalog-demo-btn" onClick={onTryPreview} disabled={!!loading}>
                <Mic size={16} />
                <span>{loading === "preview" ? "Opening Archava…" : "Try Archava live"}</span>
                <ArrowUpRight size={16} />
              </button>
              <span>No wallet needed. Just tap in and start talking.</span>
            </div>
          )}

          {wallet && (
            <button type="button" className="catalog-developer-btn" onClick={onOpenDeveloper}>
              <KeyRound size={15} />
              <span>Developer access · API keys</span>
              <ArrowUpRight size={15} />
            </button>
          )}

          <div className="catalog-progress-track" aria-label={"Chapter " + (activeCardIndex + 1) + " of 2"}>
            <div className="progress-fill-lime" style={{ width: activeCardIndex === 0 ? "50%" : "100%" }} />
            <span className="track-marker-left">01</span>
            <span className="track-marker-right">02</span>
          </div>
          <p className="catalog-concept-note">Portraits are concept artwork. The live session displays the connected AI avatar.</p>
          <a className="catalog-business-link" href="#business-section" onClick={() => sounds.playClick()}>
            <span>Using Archava for a business?</span>
            <strong>Explore a custom integration <ArrowUpRight size={14} /></strong>
          </a>
        </div>

        <div className="catalog-right-col" data-reveal>
          {session ? (
            <div className="catalog-live-session-wrapper">
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
            <div className="catalog-cards-carousel">
              <div
                className={"catalog-avatar-card card-cyan" + (activeCardIndex === 0 ? " active" : "")}
                role="button"
                tabIndex={0}
                aria-pressed={activeCardIndex === 0}
                aria-label="Chapter 01, the presence: meet the face behind the voice"
                onClick={() => selectCard(0)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter" && event.key !== " ") return;
                  event.preventDefault();
                  selectCard(0);
                }}
              >
                <img src="/assets/archava_cyan_clean.webp" alt="Concept portrait of a cyan-haired AI avatar" className="card-bg-img" />
                <span className="card-decode-scan" aria-hidden="true" />
                <div className="card-image-shade" />
                <span className="frost-overlay-tag top-right" aria-hidden="true"><span className="lime-number">01</span><ArrowUpRight size={18} /></span>
                <div className="frost-overlay-tag bottom-left">
                  <span className="tag-eyebrow">THE PRESENCE</span>
                  <span className="tag-title">A face for the conversation</span>
                  <span className="tag-subtitle">Realtime voice · avatar video</span>
                </div>
              </div>

              <div
                className={"catalog-avatar-card card-pink" + (activeCardIndex === 1 ? " active" : "")}
                role="button"
                tabIndex={0}
                aria-pressed={activeCardIndex === 1}
                aria-label="Chapter 02, try it now: talk to the live avatar first"
                onClick={() => selectCard(1)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter" && event.key !== " ") return;
                  event.preventDefault();
                  selectCard(1);
                }}
              >
                <img src="/assets/archava_pink_clean.webp" alt="Concept portrait of a pink-haired AI avatar" className="card-bg-img" />
                <span className="card-decode-scan" aria-hidden="true" />
                <div className="card-image-shade" />
                <span className="frost-overlay-tag top-right" aria-hidden="true"><span className="lime-number">02</span><ArrowUpRight size={18} /></span>
                <div className="card-access-panel">
                  <span className="tag-eyebrow">TRY IT NOW</span>
                  <h3>Enter the live room.</h3>
                  {credits && <span className="card-quote">{Math.floor(credits.remainingSeconds / 60)}m {credits.remainingSeconds % 60}s available</span>}
                  {quote && <span className="card-quote">{quote.minutes} min · {quote.tokenAmount} {quote.symbol}</span>}
                  {!config?.contractAddress && <span className="card-contract-status">Onchain minute packs opening soon</span>}
                  {wallet && config?.contractAddress && <div className="pack-options" role="group" aria-label="Choose minute pack">{(config.packMinutes || [60, 300]).map((minutes) => <button key={minutes} type="button" className={selectedPack === minutes ? "selected" : ""} onClick={(event) => { event.stopPropagation(); onSelectPack(minutes); }}>{minutes} min</button>)}</div>}
                  {wallet && config?.paymentTokenAddress && <button type="button" className="card-preview-btn" onClick={(event) => { event.stopPropagation(); onClaimFaucet(); }} disabled={!!loading}>{loading === "faucet" ? "Claiming demo mUSDT…" : "Claim 100 demo mUSDT"}</button>}
                  {!wallet ? (
                    <button
                      type="button"
                      className="card-rent-btn"
                      onClick={(event) => { event.stopPropagation(); previewAvailable ? onTryPreview() : onConnect(); }}
                      disabled={!!loading}
                    >
                      {previewAvailable ? <Mic size={15} /> : <Wallet size={15} />}
                      <span>{loading === "preview" ? "Opening Archava…" : loading === "connect" ? "Connecting…" : previewAvailable ? "Try Archava" : "Connect wallet"}</span><ArrowRight size={15} />
                    </button>
                  ) : !config?.contractAddress && previewAvailable ? (
                    <button
                      type="button"
                      className="card-rent-btn"
                      onClick={(event) => { event.stopPropagation(); onTryPreview(); }}
                      disabled={!!loading}
                    >
                      <Mic size={15} /><span>{loading === "preview" ? "Opening Archava…" : "Try Archava"}</span><ArrowRight size={15} />
                    </button>
                  ) : hasCredits ? (
                    <>
                      <button type="button" className="card-rent-btn active-lease" onClick={(event) => { event.stopPropagation(); onStartSession(); }} disabled={!readyToTalk || !!loading}>
                        <Mic size={15} /><span>{loading === "session" ? "Opening room…" : "Start conversation"}</span><ArrowRight size={15} />
                      </button>
                      <button type="button" className="card-preview-btn" onClick={(event) => { event.stopPropagation(); onBuy(); }} disabled={!canBuy}>Top up {selectedPack} minutes</button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="card-rent-btn"
                      onClick={(event) => { event.stopPropagation(); onBuy(); }}
                      disabled={!canBuy}
                    >
                      <Zap size={15} /><span>{loading === "buy" ? "Confirming pack…" : `Buy ${selectedPack} minutes`}</span><ArrowRight size={15} />
                    </button>
                  )}
                  {wallet && config?.contractAddress && previewAvailable && !hasCredits && (
                    <button
                      type="button"
                      className="card-preview-btn"
                      onClick={(event) => { event.stopPropagation(); onTryPreview(); }}
                      disabled={!readyToTalk || !!loading}
                    >
                      {loading === "preview" ? "Opening preview…" : "Try the free preview"}
                    </button>
                  )}
                </div>
              </div>
            </div>
          )}

          {error && (
            <div className="catalog-alert error" role="alert">
              <span>{error}</span>
              <button type="button" onClick={onClearError} aria-label="Dismiss error"><X size={14} /></button>
            </div>
          )}
          {txHash && (
            <div className="catalog-alert success" role="status">
              <Check size={16} />
              <span>{txKind === "faucet" ? "Demo mUSDT claimed" : "Minute pack confirmed"}: {txHash.slice(0, 10)}…</span>
              <button type="button" onClick={() => void copyTx()} aria-label="Copy transaction hash">
                {copiedTx ? <Check size={12} /> : <Copy size={12} />}
              </button>
              <a href={scanUrl + txHash} target="_blank" rel="noreferrer">
                View transaction <ExternalLink size={11} />
              </a>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
