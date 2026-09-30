# Archava Session API — integration guide

**Status: local MVP, 30 Sep 2026.** The public Vercel host's API tunnel was last measured unavailable (`/api/health` returned `502`). The minute-pack contract is not deployed. Use a running local or private API for development; the paid flow needs the team's contract before end-to-end testing.

## Product and balance

Ava is the only avatar. The free 120-second landing-page preview does not consume purchased minutes. A wallet buys 60 or 300 minutes on BNB Chain Testnet; the contract's `purchasedMinutes(wallet)` is cumulative. The Archava API subtracts completed room usage and currently reserved seconds. One purchased minute is 60 seconds of allocated room time, measured from room allocation, including any wait before a participant joins. A session reserves at most 30 minutes, or the wallet's smaller remaining balance. Unused reserved seconds return when the room is closed. One wallet can hold only one paid room across the website and **all API keys**.

```text
remainingSeconds = max(0, purchasedMinutes * 60
                      - usedSeconds - reservedSeconds)
```

An API key is free to create and is only authentication. It belongs to the wallet that signed into the developer dashboard. It cannot select another wallet or obtain free preview rooms. Store the key on **your backend**, never in a browser. Provider credentials stay on Archava's server. The returned participant token and public avatar identifiers may be passed to your browser.

The operator must run one API process with persistent `data/` storage. This file-backed ledger is not safe to share among multiple API processes; production requires a transactional database and provider reconciliation. Onchain purchases prove credits; conversation duration is measured offchain.

## Wallet and developer key

1. Connect the buyer's wallet on the website, choose 60 or 300 minutes, and pay the contract quote. The checkout requotes immediately before the transaction.
2. Sign the separate dashboard challenge at `POST /api/developer/challenge`, then send the signature to `POST /api/developer/login`. The server sets an HttpOnly dashboard cookie.
3. `POST /api/developer/keys` with `{ "label": "my app" }` returns one `apiKey` secret. Save it: listing the key later returns only metadata. `GET /api/developer/keys` lists keys; `POST /api/developer/keys/:id/revoke` revokes one.
4. `GET /api/developer/usage` reports the wallet's balance and usage. Key creation does not add minutes.

## Open, inspect, and close a session

```http
POST /v1/sessions
Authorization: Bearer archava_sk_live_…
Content-Type: application/json

{"avatar":"ava"}
```

The `avatar` field can be omitted; any value other than `"ava"` returns `400`. A successful `201` response is:

```json
{
  "sessionId": "sess_…",
  "serverUrl": "wss://…livekit.cloud",
  "participantToken": "eyJ…",
  "endsAt": 1790000000,
  "avatar": { "provider": "spatius", "appId": "…", "avatarId": "…" }
}
```

The response carries a short-lived LiveKit room token. Ava is already in the room; connect the customer browser promptly. `endsAt` is a Unix second timestamp for this room, not the wallet's purchase expiry. A customer backend should close the room as soon as its user finishes:

```http
POST /v1/sessions/<sessionId>/end
Authorization: Bearer archava_sk_live_…
```

Only the key that opened a room may close it. The response includes `seconds`, the allocated seconds charged. A second valid key under the same wallet still cannot close that session, though both keys share the wallet balance. `GET /v1/sessions` lists the calling key's open rooms. The server sweeps expired rooms every 10 seconds, so abandoned calls eventually settle.

`GET /v1/usage` returns the calling key's **wallet-wide** accounting:

```json
{
  "wallet": "0x…",
  "allocatedSeconds": 75,
  "sessionCount": 1,
  "sessions": [],
  "purchasedMinutes": 60,
  "usedSeconds": 75,
  "reservedSeconds": 0,
  "remainingSeconds": 3525,
  "active": true
}
```

`sessions` contains actual completed session records in a real response; the empty list above is abbreviated. The dashboard uses the same ledger, so website calls and API calls both count. A returned `remainingSeconds` includes the deduction for any active room reservation. The preview never appears here.

## Errors and scope

| HTTP | Meaning |
| --- | --- |
| `400` | Unknown avatar, unsupported pack, or malformed wallet/request |
| `401` | Missing, invalid, or revoked API key; invalid wallet/dashboard signature |
| `403` | No Ava minutes remaining |
| `404` | Unknown session ID on end route |
| `409` | This wallet already has a paid session |
| `502` | Chain read or room provider failed |
| `503` | Contract/provider/usage ledger unavailable |

The server checks the key, then the chain and balance, then reserves seconds before opening a vendor room. A rejected request does not open a paid room. Price in tBNB comes from `GET /api/quote?minutes=60` or `300` after contract deployment. No IDR retail price is approved; the Rp79,000/60 and Rp299,000/300 figures in `pricing/` are internal hackathon drafts.
