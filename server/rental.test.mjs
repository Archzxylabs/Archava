import assert from "node:assert/strict";
import test from "node:test";
import { createCreditReader, createPackQuoter, normalizeWallet, RENTAL_ABI, TOKEN_ABI, purchasedState } from "./rental.mjs";

test("the V2 contract seam exposes token rental quotes, expiry, and purchase events", () => {
  assert.deepEqual(RENTAL_ABI, [
    "function paymentToken() view returns (address)",
    "function quoteRent(uint8 packageId) view returns (uint256)",
    "function expiresAt(address wallet) view returns (uint256)",
    "function hasActiveAccess(address wallet) view returns (bool)",
    "event AccessRented(address indexed wallet, uint8 indexed packageId, uint256 includedMinutes, uint256 expiresAt, uint256 amountPaid)",
  ]);
  assert.deepEqual(TOKEN_ABI, [
    "function decimals() view returns (uint8)",
    "function symbol() view returns (string)",
  ]);
  assert.deepEqual(purchasedState(360n), { purchasedMinutes: 360 });
  assert.throws(() => purchasedState(2n ** 60n), /Invalid purchased minutes/);
  assert.throws(() => purchasedState(-1), /Invalid purchased minutes/);
});

test("wallet and contract configuration reject invalid addresses", () => {
  assert.throws(() => normalizeWallet("not a wallet"), /Invalid wallet/);
  assert.equal(createCreditReader({ contractAddress: "", chainId: 97 }), null);
  assert.equal(createPackQuoter({ contractAddress: "", chainId: 97 }), null);
  assert.throws(() => createCreditReader({ contractAddress: "bad", chainId: 97 }), /RENTAL_CONTRACT/);
});
