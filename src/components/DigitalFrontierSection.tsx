import { ArrowUpRight, AudioLines, MessageCircle, Mic } from "lucide-react";
import { sounds } from "../lib/sound";

const steps = [
  {
    number: "01",
    icon: MessageCircle,
    title: "Try the demo",
    detail: "Tap Talk to Ava. No wallet, account, or payment needed.",
  },
  {
    number: "02",
    icon: Mic,
    title: "Allow your mic",
    detail: "Allow microphone access so Ava can hear you. You can mute it anytime.",
  },
  {
    number: "03",
    icon: AudioLines,
    title: "Start talking",
    detail: "Ask a question naturally. End the conversation whenever you like.",
  },
] as const;

const headlineLines = [
  ["Presence", "you", "can", "feel."],
  ["Conversation", "you", "can", "join."],
];

export function DigitalFrontierSection({ previewReady }: { previewReady: boolean }) {
  return (
    <section className="digital-frontier-section" id="protocol-section">
      <div className="frontier-cad-grid" aria-hidden="true" />
      <div className="frontier-fibonacci-curve" aria-hidden="true" />

      <div className="frontier-container">
        <div className="frontier-heading-wrapper" data-reveal>
          <span className="frontier-section-label">HOW TO TRY ARCHAVA / 001</span>
          <h2 className="frontier-main-title" aria-label="Presence you can feel. Conversation you can join.">
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
            Presence you can feel.<br />Conversation you can join.
          </div>
        </div>

        <div className="frontier-split-row">
          <div className="frontier-left-col" data-reveal>
            <span className="frontier-kicker">FROM ONE TAP TO A CONVERSATION</span>
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
              {previewReady ? "The live demo is open to everyone. Ava listens, answers, and guides you through this page in a natural conversation." : "Explore Ava on this page and check the demo's availability in the avatar section."}
            </p>
            <a className="frontier-access-link" href="/build#access">Want longer calls or API access? Explore BNB Testnet minute packs <ArrowUpRight size={15} /></a>
            <p className="frontier-disclosure">Portraits on this page are concept artwork. Live sessions feature an AI-generated avatar.</p>
          </div>
        </div>
      </div>
    </section>
  );
}
