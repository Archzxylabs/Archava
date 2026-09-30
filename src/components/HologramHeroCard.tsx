import React, { useRef, useState } from "react";
import { Cpu, Radio, Sparkles, Volume2, VolumeX, Zap } from "lucide-react";
import { sounds } from "../lib/sound";

interface HologramHeroCardProps {
  onExplore: () => void;
  isReady: boolean;
}

export function HologramHeroCard({ onExplore, isReady }: HologramHeroCardProps) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [rotate, setRotate] = useState({ x: 0, y: 0 });
  const [glare, setGlare] = useState({ x: 50, y: 50, opacity: 0 });
  const [activePersona, setActivePersona] = useState<"host" | "analyst" | "agent">("host");
  const [ambientVoicePlaying, setAmbientVoicePlaying] = useState(false);

  const personas = [
    { id: "host", label: "01 / ARCHAVA_HOST", tag: "Humanoid Host", desc: "Natural conversationalist & digital ambassador" },
    { id: "analyst", label: "02 / DEFAI_ANALYST", tag: "Alpha Intelligence", desc: "Live onchain trends & smart contract diagnostics" },
    { id: "agent", label: "03 / CYPHER_PILOT", tag: "Autonomous Exec", desc: "Direct Web3 execution & verifiable transactions" },
  ] as const;

  const handleMouseMove = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!cardRef.current) return;
    const rect = cardRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;

    const rotX = -((y - centerY) / centerY) * 12; // tilt max 12 deg
    const rotY = ((x - centerX) / centerX) * 12;

    setRotate({ x: rotX, y: rotY });
    setGlare({
      x: (x / rect.width) * 100,
      y: (y / rect.height) * 100,
      opacity: 0.28,
    });
  };

  const handleMouseLeave = () => {
    setRotate({ x: 0, y: 0 });
    setGlare((prev) => ({ ...prev, opacity: 0 }));
  };

  const toggleDemoAudio = (e: React.MouseEvent) => {
    e.stopPropagation();
    sounds.playClick();
    if (ambientVoicePlaying) {
      setAmbientVoicePlaying(false);
    } else {
      setAmbientVoicePlaying(true);
      sounds.playBlip(720, 0.2);
      setTimeout(() => sounds.playBlip(980, 0.25), 180);
      setTimeout(() => sounds.playBlip(840, 0.3), 400);
      setTimeout(() => setAmbientVoicePlaying(false), 2400);
    }
  };

  return (
    <div
      ref={cardRef}
      className="hero-3d-wrapper"
      onMouseMove={handleMouseMove}
      onMouseLeave={handleMouseLeave}
      style={{
        transform: `perspective(1000px) rotateX(${rotate.x}deg) rotateY(${rotate.y}deg)`,
      }}
    >
      {/* Specular glare overlay */}
      <div
        className="card-glare"
        style={{
          background: `radial-gradient(circle at ${glare.x}% ${glare.y}%, rgba(255, 255, 255, ${glare.opacity}), transparent 60%)`,
        }}
      />

      {/* Cyber Grid Background */}
      <div className="card-cyber-grid" />

      {/* Card Header Telemetry */}
      <div className="card-hud-header">
        <div className="hud-badge">
          <span className="hud-dot pulsing" />
          <span className="mono-text">{personas.find((p) => p.id === activePersona)?.label}</span>
        </div>
        <div className="hud-meta">
          <span className="mono-sub">LATENCY: 140MS</span>
          <span className="hud-status-pill">{isReady ? "NODE ONLINE" : "TESTNET READY"}</span>
        </div>
      </div>

      {/* Hologram Core Display */}
      <div className="card-hologram-stage">
        <div className="hologram-rings">
          <div className="ring ring-1" />
          <div className="ring ring-2" />
          <div className="ring ring-3" />
        </div>

        {/* Central Glowing Avatar Presence */}
        <div className={`avatar-core-orb ${ambientVoicePlaying ? "speaking-now" : ""}`}>
          <div className="orb-inner">
            <span className="brand-mark">
              <span className="brand-mark-cut" />
            </span>
          </div>
          <div className="orb-scanner-line" />
        </div>

        {/* HUD Crosshairs & Corner Brackets */}
        <div className="bracket bracket-tl" />
        <div className="bracket bracket-tr" />
        <div className="bracket bracket-bl" />
        <div className="bracket bracket-br" />

        {/* Soundwave Visualizer Bar */}
        <div className="hologram-visualizer">
          {[40, 75, 95, 60, 85, 100, 70, 50, 90, 65, 45, 80, 55].map((h, i) => (
            <span
              key={i}
              className={`viz-bar ${ambientVoicePlaying ? "animated" : ""}`}
              style={{
                height: ambientVoicePlaying ? `${h}%` : `${Math.max(15, (h * 0.4))}%`,
                animationDelay: `${i * 0.08}s`,
              }}
            />
          ))}
        </div>

        {/* Floating Audio Demo Button */}
        <button
          type="button"
          className="audio-preview-toggle"
          onClick={toggleDemoAudio}
          title={ambientVoicePlaying ? "Voice active" : "Test neural audio frequency"}
        >
          {ambientVoicePlaying ? <Volume2 size={15} /> : <VolumeX size={15} />}
          <span>{ambientVoicePlaying ? "SYNTHESIZING..." : "PREVIEW VOICE"}</span>
        </button>
      </div>

      {/* Interactive Persona Selector Bar */}
      <div className="card-personas-row">
        {personas.map((p) => (
          <button
            key={p.id}
            type="button"
            className={`persona-pill ${activePersona === p.id ? "active" : ""}`}
            onClick={() => {
              sounds.playClick();
              setActivePersona(p.id);
            }}
            onMouseEnter={() => sounds.playHover()}
          >
            <Sparkles size={11} />
            <span>{p.tag}</span>
          </button>
        ))}
      </div>

      {/* Card Footer with Quick Action */}
      <div className="card-hud-footer">
        <div className="hud-desc-block">
          <p className="hud-persona-desc">{personas.find((p) => p.id === activePersona)?.desc}</p>
          <div className="hud-specs-row">
            <span><Cpu size={12} /> GEMINI 2.0 FLASH</span>
            <span><Radio size={12} /> ARCHAVA LIVE</span>
            <span><Zap size={12} /> LIVEKIT RTC</span>
          </div>
        </div>
        <button
          type="button"
          className="card-action-btn"
          onClick={() => {
            sounds.playClick();
            onExplore();
          }}
          onMouseEnter={() => sounds.playHover()}
        >
          <span>ENGAGE</span>
          <div className="btn-arrow">→</div>
        </button>
      </div>
    </div>
  );
}
