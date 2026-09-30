import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { createApp } from "./app.mjs";
import { createDependencies, SWEEP_INTERVAL_MS } from "./dependencies.mjs";

/**
 * Process bootstrap for the Archava API.
 *
 * Everything about how the service answers a request lives in `app.mjs`; this
 * file only reads the environment once, starts the periodic sweep, and listens.
 * That split is what lets the tests drive the identical handler over a real
 * socket: they call `createApp` with fakes, and they never import this module.
 */

// `process.loadEnvFile` is version-gated (Node 20.12+). The capability is checked
// before the file is, so an older runtime starts with its inherited environment
// instead of throwing a TypeError on a function that does not exist.
if (typeof process.loadEnvFile === "function" && existsSync(resolve(".env"))) {
  process.loadEnvFile(resolve(".env"));
}

const port = Number(process.env.PORT || 5002);
const dependencies = createDependencies({ env: process.env, cwd: process.cwd() });
const app = createApp(dependencies);

createServer(app.handle).listen(port, () => console.log("Archava API listening on :" + port));

setInterval(() => {
  const nowMs = Date.now();
  // Every collaborator that keeps expiring state prunes itself on the same tick,
  // so an abandoned preview or a paid room at its reserved-time cap both close.
  dependencies.challenges.prune();
  dependencies.dashboardChallenges.prune();
  dependencies.dashboardSessions.prune();
  app.pruneRateBuckets();
  for (const [ticket, session] of dependencies.sessionTickets) {
    if (session.endsAt * 1000 < nowMs) dependencies.sessionTickets.delete(ticket);
  }
  void dependencies.customerSessions.sweep().catch((error) => {
    console.error("Customer session sweep failed:", error);
  });
  void dependencies.closeExpiredRooms().catch((error) => {
    console.error("Room expiry sweep failed:", error);
  });
}, SWEEP_INTERVAL_MS).unref();
