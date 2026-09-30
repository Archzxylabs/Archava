# Contract handoff — Ava minute packs

The frontend and API are prepared for **one avatar, Ava, on BNB Smart Chain Testnet (chain ID 97)**. This document is the interface for the teammate writing Solidity. The contract is not implemented here. Do not deploy the earlier `expiresAt / quoteRent / rent` design: the product sells usable minutes, not a 24-hour lease.

## Required ABI and meaning

```solidity
function purchasedMinutes(address buyer) external view returns (uint256);
function quotePack(uint64 minutes) external view returns (uint256 weiAmount);
function buyPack(uint64 minutes) external payable;
```

- Support **exactly 60 and 300** as `minutes` in this MVP. `quotePack` and `buyPack` must both revert for any other value.
- `purchasedMinutes(wallet)` is the **cumulative lifetime number of minutes bought** by that wallet. It starts at zero and only increases. Buying 60 and then 300 must read 360. It must not represent a session expiry or a balance that the contract attempts to decrement.
- `quotePack(minutes)` returns the exact wei that `buyPack(minutes)` requires. Reject both underpayment and overpayment. The frontend calls the quote again immediately before sending the payable transaction and waits for its receipt.
- A purchase belongs to `msg.sender`; a caller cannot credit an arbitrary wallet. Emit an event with buyer, pack minutes, exact wei paid, and new cumulative total. The app does not depend on the event name.
- Price and withdrawal permissions are the contract teammate's choice. Send the exact 60-minute and 300-minute quotes in wei to the app owner before the public demo. Testnet `tBNB` has no IDR retail value. The planning drafts below are unit-economics targets, not onchain quotes or prices approved for checkout:

| Pack | Draft IDR price | Effective IDR per minute |
| --- | ---: | ---: |
| 60 minutes | Rp79,000 | Rp1,316.67 |
| 300 minutes | Rp299,000 | Rp996.67 |

The 300-minute draft is about 24.3% cheaper per minute. These are hackathon positioning drafts, replacing the earlier higher placeholders. `pricing/cost_model.py` records the vendor and exchange-rate assumptions; neither IDR draft is displayed as a live payment amount. They should be reviewed again before a real paid launch, especially if the agent moves to a paid LiveKit Cloud deployment.

## How the product consumes minutes

One purchased minute is **60 seconds of allocated LiveKit room time**. The contract proves purchase; the Archava server meters usage. The server reads `purchasedMinutes(wallet)` for every paid start and computes:

```text
remainingSeconds = max(0, purchasedMinutes * 60
                      - completedUsageSeconds
                      - activeReservedSeconds)
```

The server reserves at most 1,800 seconds for one room, or fewer when the wallet has less remaining. One wallet can have one paid room at a time across the website and **all its API keys**. At room end, the server records elapsed allocated seconds (rounded up to the next second, minimum one) and releases unused reservation. An expired room is closed and settled by the sweeper. The free 120-second preview does not use this ledger. Additional pack purchases add minutes without resetting completed usage.

The ledger is currently a local JSON file written by one Node API process. **Run exactly one API instance with persistent `data/` storage** for the hackathon. Multiple API instances or ephemeral storage would allow inconsistent balances; production would require a transactional database and a stronger reconciliation design. The onchain contract alone cannot attest to offchain conversation duration.

## Existing integration

| Caller | File | What it does |
| --- | --- | --- |
| Browser wallet | `src/lib/rental.ts` | Switches to chain ID 97, calls `quotePack(minutes)`, then `buyPack(minutes)` with exact value and waits for receipt. |
| Browser quote | `GET /api/quote?minutes=60` or `300` | Reads the selected contract quote. |
| Browser balance | `GET /api/access?wallet=…` | Combines cumulative onchain purchase with server usage and reservations. |
| Paid browser call | `POST /api/session` | Requires a one-use wallet signature, then reserves minutes and opens the room. |
| Customer API call | `POST /v1/sessions` | Authenticates the API key, uses its wallet's same balance, then opens the room. |
| Dashboard/customer usage | `/api/developer/usage`, `/v1/usage` | Reports purchased minutes, used seconds, reserved seconds and remaining seconds. |

## Deliver to app owner

1. Contract address on chain 97, verified source, exact ABI, deployment transaction hash, and RPC endpoint.
2. `quotePack(60)` and `quotePack(300)` in wei, with the pricing rule and withdrawal roles.
3. Evidence: zero purchases returns zero; each pack adds cumulatively; unsupported pack and wrong `msg.value` revert; one wallet's purchase does not credit another.
4. A test wallet with a confirmed 60-minute purchase, so the app owner can verify balance, one browser call, one API-key call, early end/refund of unused reserved seconds, and a top-up.

After deployment set `PACK_CONTRACT`, `CHAIN_ID=97`, and `RPC_URL` in the **private server environment**, restart the API, and check both quote routes, `/api/access`, wallet purchase, and both paid session routes. Do not put provider keys in `VITE_*` variables.

The current public Vercel host still points its API rewrite at a temporary Cloudflare Quick Tunnel; the last public check returned `502` for `/api/health`. Replace that route with a reachable API and worker before sharing a live demo. The paid flow cannot be verified end to end until this contract is deployed and connected.
