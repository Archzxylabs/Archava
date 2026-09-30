import assert from "node:assert/strict";
import test from "node:test";
import { createCreditReader, createPackQuoter, normalizeWallet, PACK_ABI, purchasedState } from "./rental.mjs";

test("the contract seam exposes cumulative minute packs, never expiry", () => {
  assert.deepEqual(PACK_ABI, [
    "function purchasedMinutes(address buyer) view returns (uint256)",
    "function quotePack(uint64 minutes) view returns (uint256)",
    "function buyPack(uint64 minutes) payable",
  ]);
  assert.deepEqual(purchasedState(360n), { purchasedMinutes: 360 });
  assert.throws(() => purchasedState(2n ** 60n), /Invalid purchased minutes/);
  assert.throws(() => purchasedState(-1), /Invalid purchased minutes/);
});

test("wallet and contract configuration reject invalid addresses", () => {
  assert.throws(() => normalizeWallet("not a wallet"), /Invalid wallet/);
  assert.equal(createCreditReader({ contractAddress: "", chainId: 97 }), null);
  assert.equal(createPackQuoter({ contractAddress: "", chainId: 97 }), null);
  assert.throws(() => createCreditReader({ contractAddress: "bad", chainId: 97 }), /PACK_CONTRACT/);
});
