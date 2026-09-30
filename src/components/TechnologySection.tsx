import { Cpu, Radio, Shield, Zap, CheckCircle2 } from "lucide-react";
import { sounds } from "../lib/sound";

export function TechnologySection() {
  const pillars = [
    {
      icon: Cpu,
      title: "Gemini Realtime",
      sub: "Cognitive Speech-to-Speech",
      desc: "Direct multimodal neural processing with dynamic intonation, natural interruptions, and ultra-low conversational latency.",
    },
    {
      icon: Radio,
      title: "Realtime Avatar Engine",
      sub: "Live Visual Presence",
      desc: "An avatar gives Gemini's voice a face during the LiveKit conversation.",
    },
    {
      icon: Shield,
      title: "BNB Chain Settlement",
      sub: "Verifiable Access Tokens",
      desc: "BNB Chain records cumulative Ava minute purchases. The server meters allocated room seconds and wallet signatures prevent replay attacks.",
    },
    {
      icon: Zap,
      title: "LiveKit RTC Mesh",
      sub: "Global Edge Low-Latency",
      desc: "Sub-200ms roundtrip audio and video transport worldwide with end-to-end encryption and adaptive bitrate streaming.",
    },
  ];

  return (
    <section className="tech-architecture-section" id="technology">
      <div className="section-container">
        <div className="tech-header-block">
          <span className="section-kicker">03 // SYSTEM ARCHITECTURE</span>
          <h2 className="section-main-heading">
            Sovereign Intelligence. <br />
            <em className="gradient-accent">Cryptographic Proof.</em>
          </h2>
          <p className="tech-intro-p">
            Archava combines live generative AI with onchain minute purchases. The server allocates room seconds against the wallet balance in real time.
          </p>
        </div>

        <div className="tech-bento-grid">
          {pillars.map((pillar, idx) => {
            const Icon = pillar.icon;
            return (
              <div
                key={idx}
                className="tech-bento-cell"
                onMouseEnter={() => sounds.playHover()}
              >
                <div className="cell-spotlight" />
                <div className="cell-icon-wrap">
                  <Icon size={24} />
                </div>
                <div className="cell-sub">{pillar.sub}</div>
                <h3 className="cell-title">{pillar.title}</h3>
                <p className="cell-desc">{pillar.desc}</p>
                <div className="cell-hud-index">MOD_0{idx + 1}</div>
              </div>
            );
          })}
        </div>

        {/* Technical Architecture Specs Strip */}
        <div className="tech-specs-strip">
          <div className="spec-item">
            <CheckCircle2 size={16} className="spec-check" />
            <span>Non-custodial access validation</span>
          </div>
          <div className="spec-item">
            <CheckCircle2 size={16} className="spec-check" />
            <span>Zero server-side credential leakage</span>
          </div>
          <div className="spec-item">
            <CheckCircle2 size={16} className="spec-check" />
            <span>EIP-191 Replay-resistant nonces</span>
          </div>
          <div className="spec-item">
            <CheckCircle2 size={16} className="spec-check" />
            <span>Automatic expired room sweeping</span>
          </div>
        </div>
      </div>
    </section>
  );
}
