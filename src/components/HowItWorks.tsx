import { AudioLines, LockKeyhole, Wallet } from "lucide-react";
import { sounds } from "../lib/sound";

export function HowItWorks() {
  const steps = [
    {
      number: "01",
      icon: Wallet,
      tag: "IDENTITY",
      title: "Connect & Challenge",
      desc: "Connect MetaMask, Rabby, or any Web3 wallet. Our server issues a cryptographically signed challenge (EIP-191) to verify wallet custody without gas fees.",
    },
    {
      number: "02",
      icon: LockKeyhole,
      tag: "SMART CONTRACT",
      title: "Onchain Minute Packs",
      desc: "Buy Ava minutes on BNB Chain. Archava checks cumulative purchases and its usage ledger before opening a paid room.",
    },
    {
      number: "03",
      icon: AudioLines,
      tag: "NEURAL STREAM",
      title: "Live Neural Presence",
      desc: "Archava reserves available seconds and provisions a short-lived LiveKit token. Gemini powers the conversation while the avatar responds on screen.",
    },
  ];

  return (
    <section className="how-workflow-section" id="how-it-works">
      <div className="section-container">
        <div className="section-header-row">
          <div className="section-badge-title">
            <span className="section-kicker">02 // PROTOCOL WORKFLOW</span>
            <h2 className="section-main-heading">
              Three Steps. <br />
              <em className="gradient-accent">Zero Friction Access.</em>
            </h2>
          </div>
          <p className="section-subtext">
            From raw wallet signature to a photorealistic digital human. Engineered with zero-trust Web3 principles.
          </p>
        </div>

        <div className="workflow-cards-grid">
          {steps.map((step) => {
            const Icon = step.icon;
            return (
              <div
                key={step.number}
                className="workflow-card"
                onMouseEnter={() => sounds.playHover()}
              >
                <div className="workflow-card-glow" />
                <div className="card-top-indicator">
                  <span className="step-num-badge">{step.number}</span>
                  <span className="step-tag">{step.tag}</span>
                </div>
                <div className="workflow-icon-bubble">
                  <Icon size={26} />
                </div>
                <h3 className="workflow-card-title">{step.title}</h3>
                <p className="workflow-card-desc">{step.desc}</p>
                <div className="workflow-card-bracket" />
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
