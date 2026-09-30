import { randomBytes } from "node:crypto";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { normalizeWallet } from "./rental.mjs";

// One minute bought onchain is 60 seconds of allocated room time. The same
// durable ledger holds browser and API sessions, so neither can bypass the
// other's reservation. This store is for ONE Node process on ONE disk.
function canonicalWallet(value) {
  try { return normalizeWallet(value) === value; }
  catch { return false; }
}

function validRecord(value) {
  return value && typeof value.id === "string" && typeof value.wallet === "string"
    && canonicalWallet(value.wallet) && typeof value.keyId === "string"
    && typeof value.roomName === "string" && typeof value.endedAt === "string"
    && Number.isSafeInteger(value.seconds) && value.seconds > 0;
}

function validHold(value) {
  return value && typeof value.id === "string" && typeof value.ticket === "string"
    && canonicalWallet(value.wallet) && typeof value.keyId === "string"
    && typeof value.roomName === "string" && Number.isSafeInteger(value.startedAtMs)
    && Number.isSafeInteger(value.endsAt) && Number.isSafeInteger(value.reservedSeconds)
    && value.reservedSeconds > 0 && value.reservedSeconds <= 1800;
}

function toPublic(record) {
  return {
    id: record.id, keyId: record.keyId, wallet: record.wallet,
    roomName: record.roomName, allocatedSeconds: record.seconds, endedAt: record.endedAt,
  };
}

export function publicUsage(records) {
  return {
    allocatedSeconds: records.reduce((sum, entry) => sum + entry.allocatedSeconds, 0),
    sessionCount: records.length,
    sessions: records,
  };
}

export class CreditError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

export function createUsageStore({ path, now = Date.now } = {}) {
  let state = { records: [], holds: [] };
  let loadPromise;
  let tail = Promise.resolve();

  async function load() {
    if (loadPromise) return loadPromise;
    loadPromise = (async () => {
      let raw;
      try { raw = await readFile(path, "utf8"); }
      catch (error) {
        if (error.code === "ENOENT") return;
        throw error;
      }
      let parsed;
      try { parsed = JSON.parse(raw); }
      catch { throw new Error("Usage ledger is corrupt; paid access is paused"); }
      // Import the prior history rather than restoring a purchased balance as
      // though no one had used it. Existing records were already room seconds.
      const records = Array.isArray(parsed) ? parsed : parsed?.records;
      const holds = Array.isArray(parsed) ? [] : parsed?.holds;
      if (!Array.isArray(records) || !Array.isArray(holds) ||
          !records.every(validRecord) || !holds.every(validHold)) {
        throw new Error("Usage ledger is invalid; paid access is paused");
      }
      state = { records, holds };
    })();
    return loadPromise;
  }

  async function flush() {
    await mkdir(dirname(path), { recursive: true });
    const tmp = `${path}.tmp-${process.pid}-${randomBytes(4).toString("hex")}`;
    await writeFile(tmp, JSON.stringify({ version: 2, ...state }, null, 2), "utf8");
    await rename(tmp, path);
  }

  function mutate(action) {
    const run = tail.then(async () => {
      await load();
      const previous = structuredClone(state);
      try {
        const result = action();
        await flush();
        return result;
      } catch (error) {
        state = previous;
        throw error;
      }
    });
    tail = run.catch(() => {});
    return run;
  }

  async function read(action) {
    await tail;
    await load();
    return action();
  }

  function totals(wallet) {
    const usedSeconds = state.records.filter((r) => r.wallet === wallet)
      .reduce((sum, r) => sum + r.seconds, 0);
    const reservedSeconds = state.holds.filter((h) => h.wallet === wallet)
      .reduce((sum, h) => sum + h.reservedSeconds, 0);
    return { usedSeconds, reservedSeconds };
  }

  async function balance(walletInput, purchasedMinutes) {
    const wallet = normalizeWallet(walletInput);
    if (!Number.isSafeInteger(purchasedMinutes) || purchasedMinutes < 0 ||
        !Number.isSafeInteger(purchasedMinutes * 60)) throw new Error("Invalid purchased minutes");
    return read(() => {
      const { usedSeconds, reservedSeconds } = totals(wallet);
      return {
        purchasedMinutes, usedSeconds, reservedSeconds,
        remainingSeconds: Math.max(0, purchasedMinutes * 60 - usedSeconds - reservedSeconds),
        active: purchasedMinutes * 60 > usedSeconds + reservedSeconds,
      };
    });
  }

  async function reserve({ wallet: input, keyId, roomName, purchasedMinutes, maxSeconds = 1800 }) {
    const wallet = normalizeWallet(input);
    if (!Number.isSafeInteger(purchasedMinutes) || purchasedMinutes < 0 ||
        !Number.isSafeInteger(purchasedMinutes * 60)) throw new Error("Invalid purchased minutes");
    if (!Number.isSafeInteger(maxSeconds) || maxSeconds <= 0) throw new Error("Invalid session cap");
    return mutate(() => {
      if (state.holds.some((hold) => hold.wallet === wallet)) {
        throw new CreditError(409, "An Ava session is already active for this wallet");
      }
      const { usedSeconds, reservedSeconds } = totals(wallet);
      const available = purchasedMinutes * 60 - usedSeconds - reservedSeconds;
      if (available <= 0) throw new CreditError(403, "No Ava minutes remaining. Buy a minute pack.");
      const reserved = Math.min(maxSeconds, available);
      const startedAtMs = now();
      const hold = {
        id: "sess_" + randomBytes(10).toString("hex"),
        ticket: randomBytes(32).toString("hex"),
        keyId: String(keyId), wallet, roomName: String(roomName),
        startedAtMs, endsAt: Math.floor(startedAtMs / 1000) + reserved,
        reservedSeconds: reserved,
      };
      state.holds.push(hold);
      return { ...hold };
    });
  }

  async function release(id) {
    return mutate(() => {
      const index = state.holds.findIndex((hold) => hold.id === id);
      if (index < 0) return false;
      state.holds.splice(index, 1);
      return true;
    });
  }

  async function settle(id, endedAtMs = now()) {
    return mutate(() => {
      const index = state.holds.findIndex((hold) => hold.id === id);
      if (index < 0) return null;
      const hold = state.holds[index];
      const elapsed = Math.ceil((Math.min(endedAtMs, hold.endsAt * 1000) - hold.startedAtMs) / 1000);
      const seconds = Math.max(1, Math.min(hold.reservedSeconds, elapsed));
      const record = {
        id: "use_" + randomBytes(8).toString("hex"),
        sessionId: hold.id, keyId: hold.keyId, wallet: hold.wallet,
        roomName: hold.roomName, seconds, endedAt: new Date(endedAtMs).toISOString(),
      };
      state.holds.splice(index, 1);
      state.records.push(record);
      return { ...toPublic(record), seconds };
    });
  }

  async function findByTicket(ticket) {
    return read(() => {
      const hold = state.holds.find((entry) => entry.ticket === ticket);
      return hold ? { ...hold } : null;
    });
  }

  async function findHold(id) {
    return read(() => {
      const hold = state.holds.find((entry) => entry.id === id);
      return hold ? { ...hold } : null;
    });
  }

  async function listHolds() { return read(() => state.holds.map((hold) => ({ ...hold }))); }

  // Kept for importing historical paid usage and older operator scripts.
  async function record({ keyId, wallet: walletInput, roomName, seconds }) {
    if (typeof keyId !== "string" || !keyId.trim()) return null;
    if (!Number.isSafeInteger(seconds) || seconds <= 0) return null;
    const wallet = normalizeWallet(walletInput);
    return mutate(() => {
      const record = {
        id: "use_" + randomBytes(8).toString("hex"), keyId, wallet,
        roomName: String(roomName || "").slice(0, 128), seconds,
        endedAt: new Date(now()).toISOString(),
      };
      state.records.push(record);
      return toPublic(record);
    });
  }

  async function list(walletInput) {
    const wallet = normalizeWallet(walletInput);
    return read(() => state.records.filter((record) => record.wallet === wallet)
      .sort((a, b) => b.endedAt.localeCompare(a.endedAt)).map(toPublic));
  }

  async function summary(walletInput) { return publicUsage(await list(walletInput)); }

  return { balance, reserve, release, settle, findByTicket, findHold, listHolds, record, list, summary, flush };
}
