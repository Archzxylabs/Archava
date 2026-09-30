export function StatsMarquee() {
  const items = [
    { label: "REALTIME NEURAL VOICE", badge: "GEMINI 2.0" },
    { label: "REALTIME AVATAR", badge: "ARCHAVA LIVE" },
    { label: "TRUSTLESS RENTALS", badge: "BNB SMART CHAIN" },
    { label: "EDGE TRANSPORT", badge: "WEBRTC / LIVEKIT" },
    { label: "SUB-200MS LATENCY", badge: "STREAMING" },
    { label: "DECENTRALIZED IDENTITY", badge: "EIP-712" },
  ];

  return (
    <div className="marquee-wrapper" aria-hidden="true">
      <div className="marquee-track">
        {[...items, ...items].map((item, idx) => (
          <div key={idx} className="marquee-item">
            <span className="marquee-glyph">◈</span>
            <span className="marquee-text">{item.label}</span>
            <span className="marquee-tag">{item.badge}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
