import { normalizeWallet } from "./rental.mjs";

/**
 * Reads cumulative purchased minutes from the contract. A single durable
 * ledger then subtracts completed usage and active reservations. The same gate
 * is used by the browser and customer API before either opens a room.
 */
export function createEntitlementGate({ readCredits, usage } = {}) {
  return async function checkEntitlement(walletInput) {
    let wallet;
    try { wallet = normalizeWallet(walletInput); }
    catch { return { ok: false, status: 400, error: "Invalid wallet address" }; }
    if (typeof readCredits !== "function") {
      return { ok: false, status: 503, error: "Minute-pack contract not configured" };
    }
    let purchasedMinutes;
    try {
      const chain = await readCredits(wallet);
      purchasedMinutes = chain?.purchasedMinutes;
      if (!Number.isSafeInteger(purchasedMinutes) || purchasedMinutes < 0) throw new Error("Invalid chain balance");
    } catch {
      return { ok: false, status: 502, error: "Could not read purchased minutes" };
    }
    let balance;
    try { balance = await usage.balance(wallet, purchasedMinutes); }
    catch { return { ok: false, status: 503, error: "Usage ledger unavailable" }; }
    if (balance.reservedSeconds > 0) {
      return { ok: false, status: 409, error: "An Ava session is already active for this wallet" };
    }
    if (balance.remainingSeconds <= 0) {
      return { ok: false, status: 403, error: "No Ava minutes remaining. Buy a minute pack." };
    }
    return { ok: true, wallet, ...balance };
  };
}
