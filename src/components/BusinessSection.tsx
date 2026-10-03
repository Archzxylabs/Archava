import { ArrowUpRight, AudioLines, BookOpenText, Store, Ticket, UtensilsCrossed } from "lucide-react";
import { sounds } from "../lib/sound";

interface BusinessSectionProps {
  contactHref: string;
  previewEnabled: boolean;
  onTryPreview: () => void;
}

const industries = [
  { label: "Hospitality", icon: UtensilsCrossed },
  { label: "Retail", icon: Store },
  { label: "Education", icon: BookOpenText },
  { label: "Events", icon: Ticket },
  { label: "Customer experience", icon: AudioLines },
] as const;

export function BusinessSection({ contactHref, previewEnabled, onTryPreview }: BusinessSectionProps) {
  return (
    <section className="business-section" id="business-section">
      <div className="business-grid" aria-hidden="true" />
      <div className="business-orbit business-orbit-outer" aria-hidden="true" />
      <div className="business-orbit business-orbit-inner" aria-hidden="true" />

      <div className="business-container">
        <div className="business-heading" data-reveal>
          <span className="business-eyebrow">ARCHAVA FOR BUSINESS / CUSTOM INTEGRATION</span>
          <h2>Give your business<br /><span>a presence.</span></h2>
          <div className="business-heading-mark" aria-hidden="true">A<span>✳</span></div>
        </div>

        <div className="business-lower" data-reveal>
          <div className="business-description">
            <span className="business-index">01 / BUILT AROUND YOU</span>
            <p>
              Let Archava welcome people, answer questions, and guide their next step. We shape the avatar, knowledge, and conversation around how your business works.
            </p>
          </div>

          <div className="business-action">
            <span className="business-index">02 / WHERE IT FITS</span>
            <div className="business-industries" aria-label="Example industries">
              {industries.map(({ label, icon: Icon }) => (
                <span className="business-industry" key={label}><Icon size={14} strokeWidth={1.7} />{label}</span>
              ))}
            </div>
            {contactHref ? (
              <a className="business-cta" href={contactHref} target={contactHref.startsWith("https:") ? "_blank" : undefined} rel={contactHref.startsWith("https:") ? "noreferrer" : undefined} onClick={() => sounds.playClick()}>
                <span>Bring Archava to your business</span><ArrowUpRight size={20} />
              </a>
            ) : previewEnabled ? (
              <button type="button" className="business-cta" onClick={onTryPreview}>
                <span>Try the live demo</span><ArrowUpRight size={20} />
              </button>
            ) : (
              <a className="business-cta" href="/build#api" onClick={() => sounds.playClick()}>
                <span>Explore integration options</span><ArrowUpRight size={20} />
              </a>
            )}
            <span className="business-action-note">
              {contactHref
                ? "Tell us what you run. We’ll explore where a live AI host could help."
                : "Hackathon demo. These industries illustrate possible custom integrations; try Ava or explore how to build with her."}
            </span>
          </div>
        </div>

        <div className="business-footer">
          <span>© ARCHAVA 2026 · BUILT FOR INDONESIA WEB3 HACKATHON</span>
          <a href="#top">BACK TO TOP ↑</a>
        </div>
      </div>
    </section>
  );
}
