import { useEffect, useRef, useState } from "react";
import { ArrowUpRight, Check, Copy, Info, KeyRound, Volume2, VolumeX, Wallet } from "lucide-react";
import { sounds } from "../lib/sound";

interface VideoHeroProps {
  wallet: string;
  chainId?: number;
  contractConnected: boolean;
  onConnect: () => void;
  loading: boolean;
  onRentClick: () => void;
  previewEnabled: boolean;
  previewLoading: boolean;
  onTryPreview: () => void;
  walletConnected: boolean;
  onOpenDeveloper: () => void;
}

export function VideoHero({
  wallet,
  chainId,
  contractConnected,
  onConnect,
  loading,
  onRentClick,
  previewEnabled,
  previewLoading,
  onTryPreview,
  walletConnected,
  onOpenDeveloper,
}: VideoHeroProps) {
  const canvasRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);
  const [soundActive, setSoundActive] = useState(sounds.isEnabled());
  const shortAddress = (address: string) => address.slice(0, 6) + "…" + address.slice(-4);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        canvasRef.current?.style.setProperty("--hero-parallax", Math.min(window.scrollY, 700) * 0.11 + "px");
      });
    };
    window.addEventListener("scroll", update, { passive: true });
    return () => {
      window.removeEventListener("scroll", update);
      cancelAnimationFrame(frame);
    };
  }, []);

  const copyWallet = async () => {
    if (!wallet) return;
    await navigator.clipboard.writeText(wallet);
    setCopied(true);
    sounds.playSuccess();
    window.setTimeout(() => setCopied(false), 2000);
  };

  const explore = () => {
    sounds.playClick();
    onRentClick();
  };

  return (
    <section className="video-hero-root" id="top">
      <div className="video-hero-canvas" ref={canvasRef}>
        <picture className="hero-picture">
          <source media="(max-width: 520px)" srcSet="/assets/archava_hero_mobile.webp" />
          <img
            src="/assets/archava_hero_clean.webp"
            alt="Concept portrait of a futuristic AI avatar"
            className="hero-bg-image"
          />
        </picture>
        <div className="hero-signal-stage" aria-hidden="true">
          {["top", "middle", "bottom"].map((slice) => (
            <picture className={`hero-signal-slice hero-signal-${slice}`} key={slice}>
              <source media="(max-width: 520px)" srcSet="/assets/archava_hero_mobile.webp" />
              <img src="/assets/archava_hero_clean.webp" alt="" />
            </picture>
          ))}
          <span className="hero-signal-noise" />
        </div>
        <div className="hero-signal-flash" aria-hidden="true" />
        <div className="hero-technical-overlay" aria-hidden="true">
          <div className="hero-grid-lines" />
          <div className="hero-fibonacci-arc" />
          <div className="hero-crosshair-center">+</div>
        </div>

        <header className="hero-navbar">
          <nav className="nav-left-links" aria-label="Main navigation">
            <a href="#top" className="nav-item active">Home</a>
            <a href="#catalog-section" className="nav-item">The avatar</a>
            <a href="#protocol-section" className="nav-item">How it works</a>
            <a href="#business-section" className="nav-item">For business</a>
            <a href="/build" className="nav-item">Build with Ava</a>
          </nav>
          <div className="nav-right-actions">
            <button
              type="button"
              className={"nav-round-btn" + (soundActive ? " active" : "")}
              onClick={() => setSoundActive(sounds.toggle())}
              aria-label={soundActive ? "Turn interface sounds off" : "Turn interface sounds on"}
              title={soundActive ? "Interface sounds on" : "Interface sounds off"}
            >
              {soundActive ? <Volume2 size={15} /> : <VolumeX size={15} />}
            </button>
            <a className="nav-round-btn" href="#protocol-section" aria-label="Learn how Archava works" title="How it works">
              <Info size={15} />
            </a>
            {walletConnected && (
              <button type="button" className="nav-dev-btn" onClick={onOpenDeveloper}>
                <KeyRound size={14} />
                <span>Developer</span>
              </button>
            )}
            {wallet ? (
              <button type="button" className="hero-wallet-pill" onClick={() => void copyWallet()} title="Copy wallet address">
                <span className="live-dot-green" />
                <span className="mono-addr">{shortAddress(wallet)}</span>
                {copied ? <Check size={13} /> : <Copy size={13} />}
              </button>
            ) : (
              <button type="button" className="hero-connect-pill" onClick={onConnect} disabled={loading}>
                <Wallet size={14} />
                <span>{loading ? "Connecting…" : "Connect wallet"}</span>
              </button>
            )}
          </div>
        </header>

        {/* Desktop keeps the section links in the navbar; on small screens the
            navbar collapses, so this compact strip keeps a usable jump to the
            avatar and business sections. */}
        <nav className="hero-section-jump" aria-label="Jump to a section">
          <span className="hero-section-jump-label">JUMP TO</span>
          <a className="hero-jump-link" href="#catalog-section">The avatar</a>
          <a className="hero-jump-link" href="#business-section">For business</a>
          <a className="hero-jump-link" href="/build">Build</a>
        </nav>

        <div className="hero-left-hud">
          <span className="hud-sub-label">ARCHAVA / TRY THE LIVE AVATAR FIRST</span>
          <div className="hud-scale-gauge" aria-hidden="true">
            <div className="scale-ticks"><span /><span /><span /><span /><span /></div>
            <div className="scale-indicator"><span className="scale-line" /><span className="scale-num">001</span></div>
          </div>
          <div className="hud-empower-box">
            <h2 className="hud-headline">A real conversation.<br />A face to remember.</h2>
            <button type="button" className="hud-next-btn" onClick={explore}>
              <span>Meet the avatar</span><span className="next-bar" />
            </button>
          </div>
        </div>

        <div className="hero-bottom-left-card">
          <div className="glass-card-inner">
            <div className="card-top-icon-row">
              <span className="brand-mark-circle"><span className="cut-pie" /></span>
              <span className="card-tag">ARCHAVA // 01</span>
            </div>
            <p className="card-body-text">
              One ready-to-use digital host. Start a conversation right now, no wallet needed — and ask her about this page or how a business could use her.
            </p>
          </div>
        </div>

        <div className="hero-center-typography">
          <h1 className="hero-massive-title"><span>TALK TO</span><span>AVA.</span></h1>
          <span className="hero-sub-kicker">ARCHAVA / ONE LIVE AI AVATAR, OPEN TO EVERYONE</span>
        </div>

        <button type="button" className="hero-right-circle-widget" onClick={explore} aria-label="Explore the Archava avatar">
          <span className="circle-inner-image">
            <img src="/assets/archava_pink_clean.webp" alt="" />
            <span className="circle-text-overlay">Meet Archava</span>
          </span>
          <span className="circle-arrow-badge"><ArrowUpRight size={16} /></span>
        </button>

        <div className="hero-cta-stack">
          <button type="button" className="hero-neon-pill-btn" onClick={previewEnabled ? onTryPreview : explore} onMouseEnter={() => sounds.playHover()} disabled={previewLoading}>
            <span>{previewLoading ? "Opening Archava…" : previewEnabled ? "Talk to Ava" : "Explore avatar"}</span><ArrowUpRight size={16} />
          </button>
          {previewEnabled && (
            <span className="hero-preview-hint">Live voice and avatar · no wallet, no sign-in</span>
          )}
          <span className="hero-access-hint">
            {contractConnected
              ? "Minute packs configured · connect a wallet to check balance"
              : previewEnabled
                ? "Onchain access opening soon · live preview available"
                : "Onchain access opening soon · preview currently offline"}
          </span>
        </div>
        <span className="hero-chain-label">{chainId === 97 || !chainId ? "BNB CHAIN TESTNET" : "CHAIN ID " + chainId} · AI-GENERATED VISUALS</span>
      </div>
    </section>
  );
}
