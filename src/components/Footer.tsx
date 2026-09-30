import { ArrowUp, Terminal, Shield } from "lucide-react";
import { sounds } from "../lib/sound";

export function Footer() {
  const scrollToTop = () => {
    sounds.playClick();
    window.scrollTo({ top: 0, behavior: "smooth" });
  };

  return (
    <footer className="site-footer">
      <div className="section-container footer-flex">
        <div className="footer-left">
          <div className="footer-brand">
            <span className="brand-mark brand-mark-small">
              <span className="brand-mark-cut" />
            </span>
            <span className="brand-title">ARCHAVA</span>
            <span className="footer-status-tag">ONCHAIN_AI</span>
          </div>
          <p className="footer-motto">
            AI Avatars, On Demand. Onchain. Autonomous digital human protocol for BNB Chain.
          </p>
        </div>

        <div className="footer-middle">
          <div className="hackathon-badge">
            <Terminal size={14} />
            <span>BUILT FOR INDONESIA WEB3 HACKATHON 2026</span>
          </div>
          <div className="footer-security-note">
            <Shield size={12} />
            <span>Smart contract verified & nonces audited</span>
          </div>
        </div>

        <div className="footer-right">
          <button
            type="button"
            className="back-to-top-btn"
            onClick={scrollToTop}
            onMouseEnter={() => sounds.playHover()}
          >
            <span>RETURN TO TOP</span>
            <ArrowUp size={15} />
          </button>
        </div>
      </div>
    </footer>
  );
}
