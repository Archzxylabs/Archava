import { ArrowUpRight, AudioLines, ShieldCheck, Wallet } from "lucide-react";
import { sounds } from "../lib/sound";

const steps = [
  {
    number: "01",
    icon: Wallet,
    title: "Connect",
    detail: "Your wallet is your identity. No new account to create.",
  },
  {
    number: "02",
    icon: ShieldCheck,
    title: "Verify",
    detail: "Archava checks purchased minutes and remaining balance before issuing a room token.",
  },
  {
    number: "03",
    icon: AudioLines,
    title: "Converse",
    detail: "Speak with the avatar through a live audio and video session.",
  },
] as const;

const headlineLines = [
  ["Presence", "you", "can", "feel."],
  ["Access", "you", "can", "verify."],
];

export function DigitalFrontierSection({ contractReady, previewReady }: { contractReady: boolean; previewReady: boolean }) {
  return (
    <section className="digital-frontier-section" id="protocol-section">
      <div className="frontier-cad-grid" aria-hidden="true" />
      <div className="frontier-fibonacci-curve" aria-hidden="true" />

      <div className="frontier-container">
        <div className="frontier-heading-wrapper" data-reveal>
          <span className="frontier-section-label">THE ARCHAVA PROTOCOL / 001</span>
          <h2 className="frontier-main-title" aria-label="Presence you can feel. Access you can verify.">
            {headlineLines.map((line, lineIndex) => (
              <span className="frontier-title-line" aria-hidden="true" key={lineIndex}>
                {line.map((word, wordIndex) => (
                  <span
                    className="frontier-title-word"
                    style={{ transitionDelay: `${140 + (lineIndex * 4 + wordIndex) * 90}ms` }}
                    key={wordIndex}
                  >{word}</span>
                ))}
              </span>
            ))}
          </h2>
          <div className="frontier-reflected-title" aria-hidden="true">
            Presence you can feel.<br />Access you can verify.
          </div>
        </div>

        <div className="frontier-split-row">
          <div className="frontier-left-col" data-reveal>
            <span className="frontier-kicker">FROM WALLET TO CONVERSATION</span>
            <div className="frontier-team-cards">
              {steps.map((step) => {
                const Icon = step.icon;
                return (
                  <div key={step.number} className="team-item">
                    <div className="team-portrait-circle"><Icon size={28} strokeWidth={1.5} /></div>
                    <div className="team-meta">
                      <span className="step-number">{step.number} / 03</span>
                      <h3 className="team-name">{step.title}</h3>
                      <p className="team-role">{step.detail}</p>
                    </div>
                  </div>
                );
              })}
            </div>
            <a
              href="#catalog-section"
              className="story-pill-btn"
              onClick={() => sounds.playClick()}
            >
              Explore the avatar <ArrowUpRight size={15} />
            </a>

            <div className="frontier-follow-box">
              <span className="follow-label">POWERED BY</span>
              <div className="follow-icons-row" aria-label="Technology stack">
                <span className="social-badge">BNB Chain</span>
                <span className="social-badge">Gemini</span>
                <span className="social-badge">Avatar engine</span>
                <span className="social-badge">LiveKit</span>
              </div>
            </div>
          </div>

          <div className="frontier-right-col" data-reveal>
            <div className="chrome-avatar-portal">
              <img src="/assets/chrome_circle.jpg" alt="Chrome AI avatar concept artwork" className="chrome-portal-image" />
              <div className="portal-cad-rings" aria-hidden="true" />
            </div>
            <p className="frontier-explanation-p">
              {contractReady
                ? "Minute packs are bought on BNB Chain. Archava checks the wallet's remaining balance before opening a LiveKit room; Gemini handles conversation and the avatar engine brings it on screen. Voice and visuals run offchain."
                : `${previewReady ? "Try Ava in the live preview above." : "The live preview is temporarily offline."} BNB Chain minute packs become available when the contract is connected. Gemini handles conversation; LiveKit and the avatar engine bring it to life offchain.`}
            </p>
            <p className="frontier-disclosure">Portraits on this page are concept artwork. Live sessions feature an AI-generated avatar.</p>
          </div>
        </div>
      </div>
    </section>
  );
}
