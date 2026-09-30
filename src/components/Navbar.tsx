import { useState } from "react";
import { ArrowUpRight, Check, Copy, Volume2, VolumeX, Wallet } from "lucide-react";
import { sounds } from "../lib/sound";

interface NavbarProps {
  wallet: string;
  chainId?: number;
  onConnect: () => void;
  loading: boolean;
}

export function Navbar({ wallet, chainId, onConnect, loading }: NavbarProps) {
  const [copied, setCopied] = useState(false);
  const [soundActive, setSoundActive] = useState(sounds.isEnabled());

  const shortAddress = (addr: string) => addr.slice(0, 6) + "…" + addr.slice(-4);

  const copyWallet = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!wallet) return;
    navigator.clipboard.writeText(wallet);
    setCopied(true);
    sounds.playSuccess();
    setTimeout(() => setCopied(false), 2000);
  };

  const toggleSound = () => {
    const next = sounds.toggle();
    setSoundActive(next);
  };

  return (
    <header className="navbar-container">
      <nav className="glass-nav">
        {/* Brand */}
        <a
          href="#top"
          className="nav-brand"
          onClick={() => sounds.playClick()}
          onMouseEnter={() => sounds.playHover()}
        >
          <span className="brand-mark brand-mark-small">
            <span className="brand-mark-cut" />
          </span>
          <span className="brand-title">ARCHAVA</span>
          <span className="brand-glitch-badge">v1.0</span>
        </a>

        {/* Links */}
        <div className="nav-links">
          <a
            href="#experience"
            className="nav-link"
            onMouseEnter={() => sounds.playHover()}
          >
            Portal
          </a>
          <a
            href="#how-it-works"
            className="nav-link"
            onMouseEnter={() => sounds.playHover()}
          >
            Workflow
          </a>
          <a
            href="#technology"
            className="nav-link"
            onMouseEnter={() => sounds.playHover()}
          >
            Architecture
          </a>
        </div>

        {/* Actions Group */}
        <div className="nav-actions">
          {/* Audio FX Toggle */}
          <button
            type="button"
            className={`nav-icon-btn ${soundActive ? "active" : ""}`}
            onClick={toggleSound}
            title={soundActive ? "Mute interface audio" : "Enable interface audio"}
            aria-label="Toggle interface sound"
          >
            {soundActive ? <Volume2 size={16} /> : <VolumeX size={16} />}
          </button>

          {/* Network Chip */}
          <div className="network-pill">
            <span className="live-dot" />
            <span className="mono-label">BNB #{chainId ?? 97}</span>
          </div>

          {/* Connect / Wallet Pill */}
          {wallet ? (
            <button
              type="button"
              className="wallet-pill connected"
              onClick={copyWallet}
              title="Click to copy wallet address"
            >
              <div className="wallet-dot" />
              <span className="wallet-address">{shortAddress(wallet)}</span>
              {copied ? <Check size={14} className="copied-icon" /> : <Copy size={14} className="copy-icon" />}
            </button>
          ) : (
            <button
              type="button"
              className="connect-btn"
              onClick={() => {
                sounds.playClick();
                onConnect();
              }}
              disabled={loading}
              onMouseEnter={() => sounds.playHover()}
            >
              <Wallet size={15} />
              <span>{loading ? "Connecting..." : "Connect"}</span>
              <ArrowUpRight size={14} className="btn-arrow-icon" />
            </button>
          )}
        </div>
      </nav>
    </header>
  );
}
