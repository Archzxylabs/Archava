import { ArrowLeft, ArrowRight, ArrowUpRight, Check, Clock3, Code2, KeyRound, Mic2, ShieldCheck, Wallet, Zap } from "lucide-react";
import { type AppConfig, type PackQuote, type CreditStatus } from "../lib/rental";

interface BuildPageProps {
  config: AppConfig | null;
  wallet: string;
  credits: CreditStatus | null;
  quote: PackQuote | null;
  selectedPack: number;
  previewReady: boolean;
  loading: string;
  error: string;
  txHash: string;
  onConnect: () => void;
  onBuy: () => void;
  onClaimFaucet: () => void;
  txKind: "purchase" | "faucet";
  onSelectPack: (minutes: number) => void;
  onRefreshQuote: () => void;
  onOpenDeveloper: () => void;
  onClearError: () => void;
}

const shortWallet = (wallet: string) => wallet.slice(0, 6) + "…" + wallet.slice(-4);

const sessionExample = `// Your backend only. Keep the Archava key off the browser.
const response = await fetch(process.env.ARCHAVA_API_URL + "/v1/sessions", {
  method: "POST",
  headers: {
    Authorization: "Bearer " + process.env.ARCHAVA_API_KEY,
    "Content-Type": "application/json",
  },
  body: JSON.stringify({ avatar: "ava" }),
});
if (!response.ok) throw new Error("Ava session unavailable");
const { sessionId, serverUrl, participantToken, endsAt, avatar } =
  await response.json();
// Pass the short-lived room data to your browser, never the API key.`;

const packAbi = `quoteRent(uint8 packageId) → mUSDT base units
rent(uint8 packageId) → transfers approved mUSDT
AccessRented(wallet, packageId, includedMinutes, expiresAt, amountPaid)`;

export function BuildPage({
  config,
  wallet,
  credits,
  quote,
  selectedPack,
  previewReady,
  loading,
  error,
  txHash,
  onConnect,
  onBuy,
  onClaimFaucet,
  txKind,
  onSelectPack,
  onRefreshQuote,
  onOpenDeveloper,
  onClearError,
}: BuildPageProps) {
  const contractConfigured = Boolean(config?.contractAddress);
  const hasCredits = Boolean(credits?.active);
  const explorer = config?.chainId === 56 ? "https://bscscan.com/tx/" : "https://testnet.bscscan.com/tx/";
  const accessLabel = !contractConfigured
    ? "AWAITING CONTRACT"
    : hasCredits
      ? "MINUTES AVAILABLE"
      : quote
        ? "PACK AVAILABLE"
        : "CHECKING CONTRACT";

  return (
    <main className="build-page" id="top">
      <div className="build-page-grid" aria-hidden="true" />
      <header className="build-nav">
        <a className="build-brand" href="/" aria-label="Archava home">ARCHAVA<span>.</span><small>BUILD</small></a>
        <nav aria-label="Build page navigation">
          <a href="#access">Onchain access</a>
          <a href="#api">Session API</a>
          <a href="/#catalog-section">Talk to Ava <ArrowUpRight size={14} /></a>
        </nav>
      </header>

      <section className="build-hero" aria-labelledby="build-title">
        <div className="build-hero-copy">
          <span className="build-kicker"><span className="build-status-dot" /> ONE AVATAR / TWO WAYS IN</span>
          <h1 id="build-title">Bring Ava<br /><em>into your world.</em></h1>
          <p>
            Explore Ava in the browser. When the live preview is online, talk to her without a wallet. Buy Ava minutes with a wallet to keep talking or use an Archava API key from your own backend.
          </p>
          <div className="build-hero-actions">
            <a className="build-button build-button-primary" href="/#catalog-section"><Mic2 size={17} /> {previewReady ? "Talk to Ava" : "Explore Ava"} <ArrowUpRight size={17} /></a>
            <a className="build-button build-button-quiet" href="#access">Explore access <ArrowRight size={17} /></a>
          </div>
          <div className="build-hero-footnote">{previewReady ? "LIVE PREVIEW / NO WALLET NEEDED" : "LIVE PREVIEW / CURRENTLY OFFLINE"} · API KEYS / WALLET SIGNATURE · MINUTE PACKS / BNB CHAIN TESTNET</div>
        </div>
        <div className="build-hero-art" aria-label="Concept artwork of Ava">
          <img src="/assets/archava_hero_clean.webp" alt="Concept portrait representing Ava" />
          <div className="build-art-grid" aria-hidden="true" />
          <div className="build-art-label"><span>AVA / 001</span><strong>ONE LIVE HOST</strong><span>CONCEPT PORTRAIT</span></div>
        </div>
      </section>

      <section className="build-route-strip" aria-label="How Archava works">
        <div><span>01 / EXPERIENCE</span><strong>Meet Ava in the browser</strong><small>{previewReady ? "Anonymous live preview" : "Live preview offline"}</small></div>
        <ArrowRight size={18} aria-hidden="true" />
          <div><span>02 / ACCESS</span><strong>Buy Ava minutes</strong><small>Mock USDT on BNB Testnet</small></div>
        <ArrowRight size={18} aria-hidden="true" />
        <div><span>03 / INTEGRATE</span><strong>Open a room by API</strong><small>Key stays on your backend</small></div>
      </section>

      <section className="build-section build-access" id="access" aria-labelledby="access-title" data-reveal>
        <div className="build-section-intro">
          <span className="build-section-index">01 / ONCHAIN ACCESS</span>
          <h2 id="access-title">Your minutes.<br /><span>Your Ava sessions.</span></h2>
          <p>Minute packs belong to a wallet. Archava reads the wallet's rental events and active access from BNB Testnet, then subtracts allocated room time from its server ledger. Your website calls and API key share one balance.</p>
        </div>
        <div className="build-access-card">
          <div className="build-card-top"><span>ACCESS TERMINAL</span><span className={hasCredits || quote ? "build-ready" : "build-pending"}>{accessLabel}</span></div>
          <div className="build-access-row"><span>NETWORK</span><strong>{!config ? "UNVERIFIED" : config.chainId === 56 ? "BNB SMART CHAIN" : "BNB CHAIN TESTNET"}</strong></div>
          <div className="build-access-row"><span>WALLET</span><strong>{wallet ? shortWallet(wallet) : "NOT CONNECTED"}</strong></div>
          <div className="build-access-row"><span>CONTRACT</span><strong>{contractConfigured ? "CONFIGURED" : "NOT CONNECTED"}</strong></div>
          {contractConfigured && <div className="build-access-row"><span>AVAILABLE</span><strong>{credits ? `${Math.floor(credits.remainingSeconds / 60)}m ${credits.remainingSeconds % 60}s` : wallet ? "CHECKING BALANCE" : "CONNECT TO CHECK"}</strong></div>}
          {config && <div className="pack-options" role="group" aria-label="Choose minute pack">{(config.packMinutes || [60, 300]).map((minutes) => <button key={minutes} type="button" className={selectedPack === minutes ? "selected" : ""} onClick={() => onSelectPack(minutes)}>{minutes} min</button>)}</div>}
          {contractConfigured && quote && <div className="build-quote"><span>{quote.minutes} AVA MINUTES</span><strong>{quote.tokenAmount} {quote.symbol}</strong></div>}
          {!contractConfigured ? (
            <p className="build-card-note">The contract address is not configured in this build. {previewReady ? "The free preview is available from the landing page." : "The live preview is currently offline."}</p>
          ) : !wallet ? (
            <button className="build-button build-button-primary build-card-action" type="button" onClick={onConnect} disabled={!!loading}><Wallet size={16} /> {loading === "connect" ? "Connecting…" : "Connect wallet"}</button>
          ) : quote ? (
            <button className="build-button build-button-primary build-card-action" type="button" onClick={onBuy} disabled={!!loading}><Zap size={16} /> {loading === "buy" ? "Confirming pack…" : `Buy ${selectedPack} minutes`}</button>
          ) : (
            <button className="build-button build-button-quiet build-card-action" type="button" onClick={onRefreshQuote}>Quote unavailable · retry</button>
          )}
          {wallet && config?.paymentTokenAddress && <button className="build-button build-button-quiet build-card-action" type="button" onClick={onClaimFaucet} disabled={!!loading}><Wallet size={16} /> {loading === "faucet" ? "Claiming demo mUSDT…" : "Claim 100 demo mUSDT"}</button>}
          {hasCredits && <a className="build-button build-button-quiet build-card-action" href="/#catalog-section"><Check size={16} /> Talk with Ava <ArrowUpRight size={16} /></a>}
          {error && <div className="build-error" role="alert"><span>{error}</span><button type="button" onClick={onClearError}>Dismiss</button></div>}
          {txHash && <a className="build-tx" href={explorer + txHash} target="_blank" rel="noreferrer">{txKind === "faucet" ? "Demo mUSDT claimed" : "Minute pack confirmed"} · View transaction <ArrowUpRight size={13} /></a>}
          <span className="build-card-disclaimer"><Clock3 size={13} /> mUSDT is a testnet demo token with no real value. Keep tBNB for gas. Each call runs up to 30 minutes; unused reserved seconds return when you end it.</span>
        </div>
      </section>

      <section className="build-section build-api" id="api" aria-labelledby="api-title" data-reveal>
        <div className="build-section-intro">
          <span className="build-section-index">02 / SESSION API</span>
          <h2 id="api-title">One request.<br /><span>One live Ava.</span></h2>
          <p>Archava creates the voice and avatar room behind one session API. Your backend holds the Archava key; your browser receives only the short lived room token and public avatar identifiers.</p>
          <div className="build-api-cta">
            {!wallet ? (
              <button className="build-button build-button-primary" type="button" onClick={onConnect} disabled={!!loading}><Wallet size={16} /> Connect wallet for API keys</button>
            ) : (
              <button className="build-button build-button-primary" type="button" onClick={onOpenDeveloper}><KeyRound size={16} /> Open API key dashboard</button>
            )}
            <span>Creating a key is free. Paid sessions draw from the same wallet minute balance.</span>
          </div>
        </div>
        <div className="build-code-card">
          <div className="build-code-head"><span><Code2 size={16} /> BACKEND / JAVASCRIPT</span><span>POST /v1/sessions</span></div>
          <pre><code>{sessionExample}</code></pre>
          <div className="build-code-foot"><ShieldCheck size={16} /><span>Keep the Archava key on your server. Return only the short lived participant token to your browser.</span></div>
        </div>
      </section>

      <section className="build-contract-note" aria-labelledby="contract-note-title" data-reveal>
        <div><span className="build-section-index">03 / CONTRACT INTERFACE</span><h2 id="contract-note-title">Minutes bought onchain.</h2><p>The contract emits each wallet's purchased minutes and access expiry. Archava meters room time offchain and shares the remaining balance with the wallet and its API keys.</p></div>
        <pre><code>{packAbi}</code></pre>
      </section>

      <footer className="build-footer"><a href="/"><ArrowLeft size={15} /> Back to Ava</a><span>ARCHAVA / HACKATHON BUILD · ONE AVATAR, LIVE VOICE</span></footer>
    </main>
  );
}
