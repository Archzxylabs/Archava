import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { normalizeWallet } from "./rental.mjs";

const KEY_PREFIX = "archava_sk_live_";
const SECRET_BYTES = 32;
const LABEL_LIMIT = 64;
const DEFAULT_LABEL = "Archava customer key";

function digest(secret) {
  return createHash("sha256").update(secret).digest("hex");
}

function sameLengthEqual(left, right) {
  const a = Buffer.from(String(left));
  const b = Buffer.from(String(right));
  // timingSafeEqual throws on a length mismatch, which is itself the answer here.
  return a.length === b.length && timingSafeEqual(a, b);
}

function isRecord(value) {
  return Boolean(value) && typeof value === "object"
    && typeof value.id === "string" && typeof value.wallet === "string"
    && typeof value.secretHash === "string" && typeof value.prefix === "string";
}

/**
 * Customer API keys for the Archava session product.
 *
 * The secret is shown to the developer exactly once and only ever stored as a
 * sha256 digest. There is deliberately no balance field on a key: a wallet's
 * chain purchases and shared usage ledger determine all of its keys' access.
 *
 * Storage is a single JSON file for the one local Node instance behind the
 * current Vercel rewrite. It is not shared, not replicated, and does not exist
 * inside a serverless function that never runs this module.
 */
export function createKeyStore({ path, now = Date.now, randomBytes: random = randomBytes } = {}) {
  let records = [];
  let loaded = false;

  async function load() {
    if (loaded) return records;
    loaded = true;
    let text = "";
    try {
      text = await readFile(path, "utf8");
    } catch (error) {
      if (error.code === "ENOENT") return records;
      throw error;
    }
    try {
      const parsed = JSON.parse(text);
      if (Array.isArray(parsed)) records = parsed.filter(isRecord);
    } catch {
      // Fail closed for this boot rather than half-loading, but keep the file
      // on disk under a new name so an operator can still read what was there.
      await rename(path, path + ".corrupt-" + now()).catch(() => {});
    }
    return records;
  }

  async function flush() {
    await mkdir(dirname(path), { recursive: true });
    // Stage then rename: a crash mid-write must leave the previous file intact.
    const staging = path + ".tmp-" + process.pid;
    await writeFile(staging, JSON.stringify(records, null, 2), "utf8");
    await rename(staging, path);
  }

  async function issue(walletInput, { label } = {}) {
    await load();
    const wallet = normalizeWallet(walletInput);
    const secret = KEY_PREFIX + random(SECRET_BYTES).toString("hex");
    const record = {
      id: "key_" + randomBytes(5).toString("hex"),
      wallet,
      secretHash: digest(secret),
      prefix: secret.slice(-8),
      label: String(label ?? "").trim().slice(0, LABEL_LIMIT) || DEFAULT_LABEL,
      createdAt: new Date(now()).toISOString(),
      lastUsedAt: null,
      revokedAt: null,
    };
    records.push(record);
    try {
      await flush();
    } catch (error) {
      records = records.filter((entry) => entry.id !== record.id);
      throw error;
    }
    return {
      id: record.id,
      apiKey: secret,
      wallet,
      label: record.label,
      prefix: record.prefix,
      createdAt: record.createdAt,
    };
  }

  async function list(walletInput) {
    await load();
    const wallet = normalizeWallet(walletInput);
    return records
      .filter((record) => record.wallet === wallet)
      .sort((left, right) => left.createdAt.localeCompare(right.createdAt))
      .map(({ id, wallet: owner, prefix, label, createdAt, lastUsedAt, revokedAt }) =>
        ({ id, wallet: owner, prefix, label, createdAt, lastUsedAt, revokedAt }));
  }

  async function findBySecret(secret) {
    if (typeof secret !== "string" || !secret.startsWith(KEY_PREFIX)) return null;
    await load();
    for (const record of records) {
      if (sameLengthEqual(digest(secret), record.secretHash)) {
        return record.revokedAt ? null : record;
      }
    }
    return null;
  }

  async function revoke(id, walletInput, atNow = now) {
    await load();
    const wallet = normalizeWallet(walletInput);
    const record = records.find((entry) => entry.id === id);
    if (!record || record.wallet !== wallet || record.revokedAt) return null;
    record.revokedAt = new Date(atNow()).toISOString();
    try {
      await flush();
    } catch (error) {
      record.revokedAt = null;
      throw error;
    }
    return record;
  }

  async function touch(id, atNow = now) {
    await load();
    const record = records.find((entry) => entry.id === id);
    if (!record) return null;
    record.lastUsedAt = new Date(atNow()).toISOString();
    // A housekeeping timestamp must never be the reason a session fails.
    await flush().catch(() => {});
    return record;
  }

  return { issue, list, findBySecret, revoke, touch, flush };
}

/** The narrow view a customer session is allowed to see about a key. */
export function activeKey(record) {
  if (!record || record.revokedAt) return null;
  return { id: record.id, wallet: record.wallet, prefix: record.prefix };
}
