import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  AlertTriangle,
  Check,
  Copy,
  Info,
  KeyRound,
  LogOut,
  RotateCcw,
  ShieldAlert,
  X,
} from "lucide-react";
import { sounds } from "../lib/sound";
import {
  authenticateDeveloper,
  createDeveloperKey,
  developerErrorMessage,
  developerKeyStatus,
  fetchDeveloperUsage,
  fetchDeveloperWallet,
  formatAllocatedSessionTime,
  listDeveloperKeys,
  logoutDeveloper,
  needsDeveloperSignIn,
  revokeDeveloperKey,
  type DeveloperKey,
  type DeveloperUsage,
} from "../lib/developer";

interface DeveloperAccessProps {
  wallet: string;
  contractConnected: boolean;
  onClose: () => void;
}

const LABEL_MAX = 40;

function containDialogFocus(event: KeyboardEvent<HTMLDialogElement>) {
  if (event.key !== "Tab") return;
  const controls = [...event.currentTarget.querySelectorAll<HTMLElement>(
    'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
  )].filter(element => element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden");
  const first = controls[0];
  const last = controls[controls.length - 1];
  if (!first) { event.preventDefault(); event.currentTarget.focus(); return; }
  if (!controls.includes(document.activeElement as HTMLElement) || (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  }
}

const shortWallet = (address: string) =>
  address.length > 12 ? `${address.slice(0, 6)}…${address.slice(-4)}` : address;

const shortTimestamp = (value: string | null) => {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toISOString().slice(0, 10);
};

export function DeveloperAccess({ wallet, contractConnected, onClose }: DeveloperAccessProps) {
  const [authenticated, setAuthenticated] = useState(false);
  const [keys, setKeys] = useState<DeveloperKey[]>([]);
  const [usage, setUsage] = useState<DeveloperUsage | null>(null);
  /** Set when the server answered but its usage numbers could not be read. */
  const [usageError, setUsageError] = useState("");
  const [label, setLabel] = useState("");
  /** The only copy of the secret. Lives in this state until the panel is dismissed. */
  const [created, setCreated] = useState<{ apiKey: string; prefix: string; label: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState<"none" | "load" | "create" | "revoke">("none");
  const [revokingId, setRevokingId] = useState("");
  /** Revocation is destructive and instant, so it is a two-step confirm. */
  const [confirmRevokeId, setConfirmRevokeId] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const copyTimer = useRef(0);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    dialog?.showModal();
    closeRef.current?.focus();
    return () => {
      dialog?.close();
      document.body.style.overflow = previousOverflow;
      if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    };
  }, []);

  useEffect(() => () => window.clearTimeout(copyTimer.current), []);

  /**
   * Usage is read on its own, after the keys: a number that cannot be read must
   * not blank the key list, and a key list that failed must not be reported as
   * a usage problem.
   */
  const loadUsage = useCallback(async () => {
    setUsage(null);
    setUsageError("");
    try {
      setUsage(await fetchDeveloperUsage());
    } catch (cause) {
      if (needsDeveloperSignIn(cause)) setAuthenticated(false);
      setUsageError(developerErrorMessage(cause));
    }
  }, []);

  const loadDashboard = useCallback(async () => {
    setBusy("load");
    setError("");
    setUsageError("");
    try {
      const [walletInfo, keyList] = await Promise.all([
        fetchDeveloperWallet(),
        listDeveloperKeys(),
      ]);
      setAuthenticated(Boolean(walletInfo.wallet));
      setKeys(keyList);
      await loadUsage();
    } catch (cause) {
      if (needsDeveloperSignIn(cause)) setAuthenticated(false);
      setError(developerErrorMessage(cause));
    } finally {
      setBusy("none");
    }
  }, [loadUsage]);

  /** An existing dashboard cookie is reused; otherwise one signature is requested. */
  const signIn = useCallback(async () => {
    setError("");
    setNotice("");
    setBusy("load");
    sounds.playClick();
    try {
      await authenticateDeveloper(wallet);
      await loadDashboard();
      setNotice("Dashboard unlocked. That signature only proves you control this wallet for this session.");
      sounds.playSuccess();
    } catch (cause) {
      setError(developerErrorMessage(cause));
    } finally {
      setBusy("none");
    }
  }, [wallet, loadDashboard]);

  const createKey = useCallback(async () => {
    const trimmed = label.trim();
    if (!trimmed) {
      setError("Give the key a label so you can recognise it later.");
      return;
    }
    setError("");
    setNotice("");
    setBusy("create");
    sounds.playClick();
    try {
      const result = await createDeveloperKey(trimmed);
      setCreated({ apiKey: result.apiKey, prefix: result.prefix, label: result.label });
      setKeys(await listDeveloperKeys());
      setLabel("");
      sounds.playSuccess();
    } catch (cause) {
      if (needsDeveloperSignIn(cause)) setAuthenticated(false);
      setError(developerErrorMessage(cause));
    } finally {
      setBusy("none");
    }
  }, [label]);

  const revokeKey = useCallback(async (id: string) => {
    setError("");
    setNotice("");
    setBusy("revoke");
    setRevokingId(id);
    sounds.playClick();
    try {
      await revokeDeveloperKey(id);
      setKeys(await listDeveloperKeys());
      setNotice("Key revoked. Requests still using it now fail closed.");
      sounds.playSuccess();
    } catch (cause) {
      if (needsDeveloperSignIn(cause)) setAuthenticated(false);
      setError(developerErrorMessage(cause));
    } finally {
      setBusy("none");
      setRevokingId("");
      setConfirmRevokeId("");
    }
  }, []);

  const copySecret = useCallback(async () => {
    if (!created) return;
    try {
      await navigator.clipboard.writeText(created.apiKey);
      setCopied(true);
      sounds.playSuccess();
      window.clearTimeout(copyTimer.current);
      copyTimer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setError("Couldn't copy the key. Select the key text to copy it manually, or allow clipboard access and try again.");
    }
  }, [created]);

  const dismissSecret = useCallback(() => {
    setCreated(null);
    setCopied(false);
    setNotice("Secret hidden. It cannot be shown again — create a new key if it is lost.");
  }, []);

  useEffect(() => () => {
    // Leaving the panel beats any escape hatch: whatever the visitor clicks,
    // the one-time secret stops existing when this component unmounts.
    setCreated(null);
  }, []);

  const signOut = useCallback(async () => {
    setBusy("load");
    sounds.playClick();
    try {
      await logoutDeveloper();
    } catch {
      /* Losing the cookie server-side is fine; the local session is dropped anyway. */
    }
    setAuthenticated(false);
    setKeys([]);
    setUsage(null);
    setUsageError("");
    setCreated(null);
    setError("");
    setNotice("Dashboard session closed.");
    setBusy("none");
  }, []);

  const activeKeys = keys.filter((key) => developerKeyStatus(key) === "active").length;
  const noWallet = !wallet;

  return (
    <dialog ref={dialogRef} className="developer-access-section" id="developer-access" tabIndex={-1} aria-labelledby="developer-access-title" aria-describedby="developer-access-description" onKeyDown={containDialogFocus} onCancel={(event) => { event.preventDefault(); onClose(); }}>
      <div className="developer-access-inner">
        {/* No `data-reveal`: the scroll observer runs once at app mount, before this
            overlay exists, so the head would never leave its opacity:0 start state. */}
        <header className="developer-access-head">
          <div className="developer-access-title-row">
            <h2 id="developer-access-title" className="developer-access-title">
              Developer access<span className="brand-period">.</span>
            </h2>
            <span className="developer-access-tag">API KEYS</span>
          </div>
          <p className="developer-access-lede" id="developer-access-description">
            Build on the same realtime voice and live avatar Archava uses. Connect a wallet, sign one
            developer challenge, then create keys that authenticate Archava API requests.
          </p>
          <p className="developer-access-free">
            <Info size={13} aria-hidden="true" />
            Talking to Ava on this page stays free and wallet-free. This dashboard only manages API
            keys — it never gates the preview.
          </p>
          <a className="developer-doc-link" href="/build#api">See the session API and minute packs ↗</a>
          <button
            ref={closeRef}
            type="button"
            className="developer-access-close"
            onClick={() => {
              // Close drops the secret even if the visitor never clicked it away.
              setCreated(null);
              onClose();
            }}
          >
            <X size={14} aria-hidden="true" /><span>Close</span>
          </button>
        </header>

        {error && (
          <div className="developer-alert error" role="alert">
            <AlertTriangle size={15} aria-hidden="true" />
            <span>{error}</span>
            <button type="button" onClick={() => setError("")} aria-label="Dismiss error"><X size={12} /></button>
          </div>
        )}
        {notice && (
          <div className="developer-alert notice" role="status">
            <Check size={15} aria-hidden="true" />
            <span>{notice}</span>
          </div>
        )}

        <div className="developer-grid">
          <div className="developer-panel developer-wallet-panel">
            <span className="developer-index">01 / AUTHENTICATE</span>
            <h3 className="developer-panel-title">
              <ShieldAlert size={15} aria-hidden="true" /> Wallet authenticity
            </h3>
            <p className="developer-copy">
              Connecting a wallet is not proof of ownership. The first dashboard action asks your
              wallet to sign a message naming this dashboard and a single-use nonce. That signature is
              exchanged for a session cookie only this dashboard uses.
            </p>
            <dl className="developer-wallet-facts">
              <div>
                <dt>Wallet</dt>
                <dd className="developer-mono">{wallet ? shortWallet(wallet) : "not connected"}</dd>
              </div>
              <div>
                <dt>Dashboard session</dt>
                <dd>{authenticated ? "authenticated" : "sign-in required"}</dd>
              </div>
            </dl>

            {authenticated ? (
              <button type="button" className="developer-btn ghost" onClick={signOut} disabled={busy !== "none"}>
                <LogOut size={15} aria-hidden="true" /><span>Close dashboard session</span>
              </button>
            ) : (
              <>
                <button
                  type="button"
                  className="developer-btn primary"
                  onClick={signIn}
                  disabled={busy !== "none" || noWallet}
                >
                  <KeyRound size={15} aria-hidden="true" />
                  <span>
                    {noWallet
                      ? "Connect a wallet to continue"
                      : busy === "load" ? "Signing challenge…" : "Sign developer challenge"}
                  </span>
                </button>
                {noWallet && (
                  <p className="developer-hint">
                    No wallet connected. Connecting one here does not affect the free preview, which
                    stays wallet-free.
                  </p>
                )}
              </>
            )}
          </div>

          <div className={`developer-panel developer-keys-panel${authenticated ? "" : " is-locked"}`}>
            <span className="developer-index">02 / API KEYS</span>
            <h3 className="developer-panel-title">
              <KeyRound size={15} aria-hidden="true" /> Keys
              <span className="developer-count">{activeKeys} active</span>
            </h3>

            {!authenticated ? (
              <p className="developer-copy developer-locked-copy">
                Sign the developer challenge to create and manage keys.
              </p>
            ) : (
              <>
                <div className="developer-create-row">
                  <label className="developer-field">
                    <span className="developer-field-label">Key label</span>
                    <input
                      type="text"
                      value={label}
                      maxLength={LABEL_MAX}
                      placeholder="e.g. hackathon demo"
                      onChange={(event) => setLabel(event.target.value)}
                    />
                  </label>
                  <button
                    type="button"
                    className="developer-btn primary"
                    onClick={createKey}
                    disabled={busy !== "none" || !label.trim()}
                  >
                    <KeyRound size={15} aria-hidden="true" />
                    <span>{busy === "create" ? "Creating…" : "Create key"}</span>
                  </button>
                </div>

                {created && (
                  <div className="developer-secret" role="alert">
                    <span className="developer-secret-flag">
                      <AlertTriangle size={12} aria-hidden="true" /> SHOWN ONCE
                    </span>
                    <code className="developer-secret-value">{created.apiKey}</code>
                    <div className="developer-secret-actions">
                      <button type="button" className="developer-btn small" onClick={copySecret}>
                        {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
                        <span>{copied ? "Copied" : "Copy"}</span>
                      </button>
                      <button type="button" className="developer-btn ghost small" onClick={dismissSecret}>
                        <span>Hide</span>
                      </button>
                    </div>
                    <p className="developer-secret-warning">
                      Store this secret server-side now. Archava keeps only a hash, so it can never be
                      shown again and nobody can recover it for you. It is not written to
                      localStorage, cookies, or the URL.
                    </p>
                  </div>
                )}

                <ul className="developer-key-list">
                  {keys.length === 0 && (
                    <li className="developer-key-empty">No keys yet. Create your first Archava API key.</li>
                  )}
                  {keys.map((key) => {
                    const status = developerKeyStatus(key);
                    const confirmingThis = confirmRevokeId === key.id;
                    return (
                      <li key={key.id} className={`developer-key-row is-${status}`}>
                        <div className="developer-key-main">
                          <span className="developer-key-label">{key.label}</span>
                          <code className="developer-key-prefix">{key.prefix}…</code>
                          <span className={`developer-badge ${status}`}>
                            {status === "active" ? "ACTIVE" : "REVOKED"}
                          </span>
                        </div>
                        <dl className="developer-key-meta">
                          <div>
                            <dt>Created</dt>
                            <dd>{shortTimestamp(key.createdAt)}</dd>
                          </div>
                          <div>
                            <dt>Last used</dt>
                            <dd>{shortTimestamp(key.lastUsedAt)}</dd>
                          </div>
                        </dl>
                        {status === "active" && (
                          confirmingThis ? (
                            <div className="developer-key-confirm" role="group" aria-label={`Confirm revoking ${key.label}`}>
                              <span className="developer-confirm-label">Revoke permanently?</span>
                              <button
                                type="button"
                                className="developer-btn danger small"
                                onClick={() => void revokeKey(key.id)}
                                disabled={busy !== "none"}
                              >
                                <RotateCcw size={13} aria-hidden="true" />
                                <span>{busy === "revoke" && revokingId === key.id ? "Revoking…" : "Yes, revoke"}</span>
                              </button>
                              <button
                                type="button"
                                className="developer-btn ghost small"
                                onClick={() => setConfirmRevokeId("")}
                                disabled={busy !== "none"}
                              >
                                <span>Keep key</span>
                              </button>
                            </div>
                          ) : (
                            <button
                              type="button"
                              className="developer-btn ghost small"
                              onClick={() => setConfirmRevokeId(key.id)}
                              disabled={busy !== "none"}
                            >
                              <RotateCcw size={13} aria-hidden="true" />
                              <span>Revoke</span>
                            </button>
                          )
                        )}
                      </li>
                    );
                  })}
                </ul>
              </>
            )}
          </div>

          <div className={`developer-panel developer-usage-panel${authenticated ? "" : " is-locked"}`}>
            <span className="developer-index">03 / USAGE</span>
            <h3 className="developer-panel-title">Ava minute balance</h3>
            <p className="developer-copy">
              Your wallet and API keys share one balance. One purchased minute gives 60 seconds
              of allocated room time. A room reserves up to 30 minutes, then unused seconds return
              when it closes. The free preview never draws from this balance.
            </p>
            {authenticated && usageError && (
              <p className="developer-entitlement off" role="status">
                <AlertTriangle size={13} aria-hidden="true" />
                {usageError}
              </p>
            )}
            {!authenticated ? (
              <p className="developer-copy developer-locked-copy">Sign in to read usage for your keys.</p>
            ) : (
              <>
                <div className="developer-usage-stats">
                  <div>
                    <span className="developer-stat-value">{usage?.balanceAvailable ? formatAllocatedSessionTime(usage.remainingSeconds ?? 0) : "—"}</span>
                    <span className="developer-stat-label">Available</span>
                  </div>
                  <div>
                    <span className="developer-stat-value">{usage ? usage.sessionCount : "—"}</span>
                    <span className="developer-stat-label">Sessions</span>
                  </div>
                  <div>
                    <span className="developer-stat-value">
                      {usage ? formatAllocatedSessionTime(usage.allocatedSeconds) : "—"}
                    </span>
                    <span className="developer-stat-label">Used room time</span>
                  </div>
                </div>
                <p className="developer-copy developer-note">
                  Creating a key does <strong>not</strong> add minutes. Buy a pack with this wallet,
                  then use the same balance from the site or your own integration.
                </p>
              </>
            )}
          </div>

          <div className="developer-panel developer-pricing-panel">
            <span className="developer-index">04 / MINUTE PACKS</span>
            <h3 className="developer-panel-title">
              Pay for Ava minutes
              <span className="developer-badge draft">DEMO BUILD</span>
            </h3>
            <p className="developer-copy">
              Choose a 60 or 300 minute pack on the build page. The checkout reads the exact
              mUSDT quote, approves that amount, and rents the package on BNB Testnet. More packs add minutes.
            </p>
            <p className="developer-copy">
              <strong>Available time</strong> subtracts completed room usage and currently reserved
              sessions from your onchain purchases. End a call to return any unused reservation.
            </p>
            <p className={`developer-entitlement ${contractConnected ? "on" : "off"}`}>
              <Info size={13} aria-hidden="true" />
              {contractConnected
                ? "A contract address is configured. Archava verifies purchased minutes and remaining balance before each paid session."
                : "The minute-pack contract is not configured yet. API keys stay manageable, and paid sessions open after contract integration."}
            </p>
          </div>
        </div>

        <footer className="developer-access-foot">
          <span>Archava API · realtime voice, avatar rendering, and LiveKit rooms</span>
          <span className="developer-access-foot-status">
            {authenticated ? `authenticated · ${wallet ? shortWallet(wallet) : ""}` : "sign-in required"}
          </span>
        </footer>
      </div>
    </dialog>
  );
}
