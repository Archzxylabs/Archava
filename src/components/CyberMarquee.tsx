export function CyberMarquee() {
  const items = [
    "AI AVATAR ON DEMAND",
    "LIVE CONVERSATION",
    "WALLET-GATED ACCESS",
    "BNB CHAIN MINUTE PACKS",
    "GEMINI REALTIME VOICE",
    "REALTIME AVATAR",
    "LIVEKIT TRANSPORT",
    "ARCHAVA / 2026",
  ];

  return (
    <div className="cyber-marquee-bar" aria-hidden="true">
      <div className="marquee-content-loop">
        {[...items, ...items].map((text, i) => (
          <div key={i} className="marquee-unit">
            <span className="unit-label">{text}</span>
            <span className="unit-arrow">◄►</span>
          </div>
        ))}
      </div>
    </div>
  );
}
