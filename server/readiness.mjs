/** Probe a local worker without exposing its address or credentials. */
export function createReadinessCheck({ workerHealthUrl = "", fetcher = fetch, timeoutMs = 1500 } = {}) {
  let workerUrl;
  if (workerHealthUrl) {
    try {
      workerUrl = new URL(workerHealthUrl);
      if (workerUrl.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(workerUrl.hostname)
        || workerUrl.username || workerUrl.password) throw new Error();
    } catch { throw new Error("ARCHAVA_WORKER_HEALTH_URL must be a credential-free loopback HTTP URL"); }
  }
  return async () => {
    if (!workerUrl) return { ok: true, checks: { api: "ok", worker: "not_configured" } };
    try {
      const response = await fetcher(workerUrl, { signal: AbortSignal.timeout(timeoutMs), redirect: "error" });
      const healthy = response.status === 200;
      await response.body?.cancel();
      return { ok: healthy, checks: { api: "ok", worker: healthy ? "ok" : "unavailable" } };
    } catch { return { ok: false, checks: { api: "ok", worker: "unavailable" } }; }
  };
}
