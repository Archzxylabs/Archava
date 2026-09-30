import { useCallback, useEffect, useRef, useState } from "react";
import { BriefcaseBusiness, KeyRound, Mic, Wallet, X } from "lucide-react";
import {
  connectWallet,
  endAvatarSession,
  getConfig,
  getPackQuote,
  getCreditStatus,
  buyMinutePack,
  claimDemoUsdt,
  startAnonymousPreview,
  startAvatarSession,
  type AppConfig,
  type AvatarSession,
  type PackQuote,
  type CreditStatus,
} from "./lib/rental";
import {
  isPreviewAvailable,
  previewErrorMessage,
  supportsAvatarRendering,
  SPATIUS_UNSUPPORTED_MESSAGE,
} from "./lib/preview";
import { sounds } from "./lib/sound";
import { useScrollReveal } from "./lib/motion";
import { VideoHero } from "./components/VideoHero";
import { CyberMarquee } from "./components/CyberMarquee";
import { AvatarCatalog } from "./components/AvatarCatalog";
import { DigitalFrontierSection } from "./components/DigitalFrontierSection";
import { BusinessSection } from "./components/BusinessSection";
import { DeveloperAccess } from "./components/DeveloperAccess";
import { BuildPage } from "./components/BuildPage";

async function prepareAvatar(config: AppConfig | null): Promise<void> {
  if (config?.avatarProvider !== "spatius") return;
  const { prepareSpatiusAvatar } = await import("./lib/spatius");
  await prepareSpatiusAvatar(config.spatiusAppId || "", config.spatiusAvatarId || "");
}

export default function App() {
  useScrollReveal();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [wallet, setWallet] = useState("");
  const [credits, setCredits] = useState<CreditStatus | null>(null);
  const [quote, setQuote] = useState<PackQuote | null>(null);
  const [selectedPack, setSelectedPack] = useState(60);
  const quoteRequest = useRef(0);
  const [session, setSession] = useState<AvatarSession | null>(null);
  const [developerOpen, setDeveloperOpen] = useState(false);
  const [loading, setLoading] = useState("");
  const [error, setError] = useState("");
  const [txHash, setTxHash] = useState("");
  const [txKind, setTxKind] = useState<"purchase" | "faucet">("purchase");
  const [nextSteps, setNextSteps] = useState(false);
  const [lastCallPaid, setLastCallPaid] = useState(false);

  useEffect(() => {
    getConfig()
      .then((next) => {
        setConfig(next);
        if (next.avatarProvider === "spatius" && "RTCRtpScriptTransform" in globalThis) {
          void prepareAvatar(next).catch(() => {});
        }
      })
      .catch(() => setError("Archava preview is temporarily offline. Please try again later."));

  }, []);

  const refreshQuote = useCallback(() => {
    const request = ++quoteRequest.current;
    if (!config?.contractAddress) {
      setQuote(null);
      return;
    }
    setQuote(null);
    void getPackQuote(selectedPack).then((next) => {
      if (request === quoteRequest.current && next.minutes === selectedPack) setQuote(next);
    }).catch(() => { if (request === quoteRequest.current) setQuote(null); });
  }, [config?.contractAddress, selectedPack]);

  useEffect(() => refreshQuote(), [refreshQuote]);

  const handleSelectPack = (minutes: number) => {
    if (!config?.packMinutes?.includes(minutes)) return;
    setQuote(null);
    setSelectedPack(minutes);
  };

  useEffect(() => {
    if (!session) return;
    const onPageHide = () => {
      navigator.sendBeacon(
        "/api/session/end",
        new Blob([JSON.stringify({ ticket: session.ticket })], { type: "application/json" })
      );
    };
    window.addEventListener("pagehide", onPageHide);
    return () => window.removeEventListener("pagehide", onPageHide);
  }, [session]);

  const closeSession = useCallback(() => {
    if (!session) return;
    sounds.playClick();
    setSession(null);
    setLastCallPaid(!session.preview);
    // The call is over: offer the visitor one clear next step instead of
    // dropping them back into the catalog with no direction.
    setNextSteps(true);
    void endAvatarSession(session.ticket)
      .then(() => { if (!session.preview && wallet && config) void refreshCredits(wallet, config); })
      .catch(() => {});
    window.setTimeout(() => {
      document.getElementById("post-call-panel")?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 120);
  }, [session, wallet, config]);

  const refreshCredits = useCallback(async (address: string, appConfig: AppConfig) => {
    if (!appConfig.contractAddress) {
      setCredits(null);
      return;
    }
    try {
      setCredits(await getCreditStatus(address));
    } catch (cause) {
      setCredits(null);
      setError(cause instanceof Error ? cause.message : "Failed to read minute balance");
    }
  }, []);

  useEffect(() => {
    if (!wallet || !config?.contractAddress) return;
    const onReturn = () => { if (!document.hidden) void refreshCredits(wallet, config); };
    window.addEventListener("focus", onReturn);
    document.addEventListener("visibilitychange", onReturn);
    return () => {
      window.removeEventListener("focus", onReturn);
      document.removeEventListener("visibilitychange", onReturn);
    };
  }, [wallet, config, refreshCredits]);

  useEffect(() => {
    if (!window.ethereum?.on) return;
    const handleAccounts = (...args: unknown[]) => {
      const accounts = args[0] as string[] | undefined;
      const next = accounts?.[0] || "";
      setWallet(next);
      setCredits(null);
      // Wallet-scoped dashboard state must not survive an account switch.
      setDeveloperOpen(false);
      setSession((current) => {
        if (current) void endAvatarSession(current.ticket).catch(() => {});
        return null;
      });
      setNextSteps(false);
      if (next && config) void refreshCredits(next, config);
    };
    window.ethereum.on("accountsChanged", handleAccounts);
    return () => window.ethereum?.removeListener?.("accountsChanged", handleAccounts);
  }, [config, refreshCredits]);

  const handleConnect = async () => {
    setError("");
    setLoading("connect");
    sounds.playClick();
    try {
      const address = await connectWallet();
      setWallet(address);
      sounds.playSuccess();
      if (config) await refreshCredits(address, config);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Wallet connection rejected");
    } finally {
      setLoading("");
    }
  };

  const handleBuy = async () => {
    if (!config || !wallet || !quote) return;
    setError("");
    setTxHash("");
    setTxKind("purchase");
    setLoading("buy");
    sounds.playClick();
    try {
      const hash = await buyMinutePack(config, selectedPack, wallet);
      setTxHash(hash);
      sounds.playSuccess();
      await refreshCredits(wallet, config);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Minute-pack transaction failed");
    } finally {
      setLoading("");
    }
  };

  const handleClaimFaucet = async () => {
    if (!config || !wallet) return;
    setError("");
    setTxHash("");
    setTxKind("faucet");
    setLoading("faucet");
    sounds.playClick();
    try {
      const hash = await claimDemoUsdt(config, wallet);
      setTxHash(hash);
      sounds.playSuccess();
      await refreshCredits(wallet, config);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Demo mUSDT faucet failed");
    } finally {
      setLoading("");
    }
  };

  const scrollToCatalog = useCallback(() => {
    document.getElementById("catalog-section")?.scrollIntoView({ behavior: "smooth" });
  }, []);

  const scrollToBusiness = useCallback(() => {
    document.getElementById("business-section")?.scrollIntoView({ behavior: "smooth" });
  }, []);

  const handleStart = async () => {
    if (!wallet) return;
    setError("");
    setNextSteps(false);
    if (!supportsAvatarRendering(config)) {
      setError(SPATIUS_UNSUPPORTED_MESSAGE);
      return;
    }
    setLoading("session");
    sounds.playClick();
    try {
      await prepareAvatar(config);
      const next = await startAvatarSession(wallet);
      setSession(next);
      sounds.playSuccess();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not open neural session");
    } finally {
      setLoading("");
    }
  };

  const handleTryPreview = async () => {
    if (!previewAvailable || session) return;
    setError("");
    setNextSteps(false);
    if (!supportsAvatarRendering(config)) {
      setError(SPATIUS_UNSUPPORTED_MESSAGE);
      scrollToCatalog();
      return;
    }
    setLoading("preview");
    sounds.playClick();
    try {
      await prepareAvatar(config);
      const next = await startAnonymousPreview();
      setSession(next);
      sounds.playSuccess();
      window.setTimeout(scrollToCatalog, 80);
    } catch (cause) {
      setError(previewErrorMessage(cause));
      scrollToCatalog();
    } finally {
      setLoading("");
    }
  };

  const handleScrollToRent = () => {
    sounds.playClick();
    scrollToCatalog();
  };

  /**
   * The dashboard is wallet-authenticated but never gates the preview: it is a
   * separate surface that only appears once a wallet is connected.
   */
  const openDeveloper = useCallback(() => {
    if (!wallet) return;
    sounds.playClick();
    setDeveloperOpen(true);
  }, [wallet]);

  const closeDeveloper = useCallback(() => {
    sounds.playClick();
    setDeveloperOpen(false);
  }, []);

  const previewAvailable = isPreviewAvailable(config);

  if (window.location.pathname.replace(/\/$/, "") === "/build") {
    return (
      <div className="archava-site-root">
        <BuildPage
          config={config}
          wallet={wallet}
          credits={credits}
          quote={quote}
          selectedPack={selectedPack}
          previewReady={previewAvailable}
          loading={loading}
          error={error}
          txHash={txHash}
          onConnect={handleConnect}
          onBuy={handleBuy}
          onClaimFaucet={handleClaimFaucet}
          txKind={txKind}
          onSelectPack={handleSelectPack}
          onRefreshQuote={refreshQuote}
          onOpenDeveloper={openDeveloper}
          onClearError={() => setError("")}
        />
        {developerOpen && wallet && (
          <DeveloperAccess
            key={wallet.toLowerCase()}
            wallet={wallet}
            contractConnected={Boolean(config?.contractAddress)}
            onClose={closeDeveloper}
          />
        )}
      </div>
    );
  }

  return (
    <div className="archava-site-root">
      <VideoHero
        wallet={wallet}
        chainId={config?.chainId}
        onConnect={handleConnect}
        loading={loading === "connect"}
        onRentClick={handleScrollToRent}
        previewEnabled={previewAvailable && !session}
        previewLoading={loading === "preview"}
        onTryPreview={handleTryPreview}
        walletConnected={Boolean(wallet)}
        onOpenDeveloper={openDeveloper}
        contractConnected={Boolean(config?.contractAddress)}
      />

      <CyberMarquee />

      <AvatarCatalog
        config={config}
        wallet={wallet}
        credits={credits}
        quote={quote}
        selectedPack={selectedPack}
        session={session}
        loading={loading}
        error={error}
        txHash={txHash}
        onConnect={handleConnect}
        onBuy={handleBuy}
        onClaimFaucet={handleClaimFaucet}
        txKind={txKind}
        onSelectPack={handleSelectPack}
        onStartSession={handleStart}
        onTryPreview={handleTryPreview}
        onCloseSession={closeSession}
        onClearError={() => setError("")}
        onOpenDeveloper={openDeveloper}
      />

      {/*
        Shown only after a call ends, so every visitor — including one whose
        two minutes simply ran out — gets one clear next step instead of an
        unexplained return to the catalog.
      */}
      {nextSteps && !session && (
        <section className="post-call-panel" id="post-call-panel" aria-label="After your call">
          <div className="post-call-inner">
            <div className="post-call-head">
              <span className="post-call-eyebrow">CALL ENDED / WHAT NEXT</span>
              <button type="button" className="post-call-dismiss" onClick={() => setNextSteps(false)} aria-label="Dismiss next steps">
                <X size={14} />
              </button>
            </div>
            <h2 className="post-call-title">Thanks for talking to Ava.</h2>
            <div className="post-call-actions">
              {lastCallPaid && wallet && credits?.active && credits.reservedSeconds === 0 ? (
                <button
                  type="button"
                  className="post-call-btn primary"
                  onClick={() => { sounds.playClick(); void handleStart(); }}
                  disabled={!!loading}
                >
                  <Mic size={15} /><span>{loading === "session" ? "Opening room…" : "Continue with your minutes"}</span>
                </button>
              ) : lastCallPaid && config?.contractAddress ? (
                <a className="post-call-btn primary" href="/build#access">
                  <Wallet size={15} /><span>Check balance or top up minutes</span>
                </a>
              ) : previewAvailable ? (
                <button
                  type="button"
                  className="post-call-btn primary"
                  onClick={() => { sounds.playClick(); void handleTryPreview(); }}
                  disabled={!!loading}
                >
                  <Mic size={15} /><span>{loading === "preview" ? "Opening Archava…" : "Talk to Ava again"}</span>
                </button>
              ) : (
                <span className="post-call-unavailable">The live preview is offline right now.</span>
              )}
              <button
                type="button"
                className="post-call-btn"
                onClick={() => { wallet ? openDeveloper() : handleConnect(); }}
              >
                {wallet ? <KeyRound size={15} /> : <Wallet size={15} />}
                <span>{wallet ? "Developer access · API keys" : "Connect wallet for developer access"}</span>
              </button>
              <button
                type="button"
                className="post-call-btn"
                onClick={() => { sounds.playClick(); scrollToBusiness(); }}
              >
                <BriefcaseBusiness size={15} /><span>For business · custom integration</span>
              </button>
            </div>
          </div>
        </section>
      )}

      <DigitalFrontierSection contractReady={Boolean(config?.contractAddress)} previewReady={previewAvailable} />
      <BusinessSection
        contactHref={import.meta.env.VITE_BUSINESS_CONTACT_URL || ""}
        previewEnabled={previewAvailable && !session}
        onTryPreview={handleTryPreview}
      />

      {/*
        `key={wallet}` remounts the dashboard on an account switch, so cached
        keys, the signed-in session, and any revealed secret cannot leak into
        the next wallet's view.
      */}
      {developerOpen && wallet && (
        <DeveloperAccess
          key={wallet.toLowerCase()}
          wallet={wallet}
          contractConnected={Boolean(config?.contractAddress)}
          onClose={closeDeveloper}
        />
      )}
    </div>
  );
}
