# Archava

**AI Avatars, On Demand. Onchain.** A self-serve Ava minute-pack MVP for the Indonesia Web3 Hackathon 2026. Visitors can try the live avatar without a wallet. Paid calls use minute packs bought with demo mUSDT on BNB Testnet.

This is separate from Archava Platform's custom integration service. This app offers one ready-to-use avatar. It was built as a new project so the existing Archava demo stays intact.

## What is implemented

- Responsive landing page with one Ava demo entry, standalone `/build` page, and wallet-signed API key dashboard.
- Demo duration, microphone requirements, preparation status, and browser support are shown before starting. Errors remain visible in a dismissible toast; active calls retain microphone and end-call controls while Ava guides the visitor through the site.
- BNB Testnet wallet connection, 60/300-minute mUSDT quotes, test-token faucet, and approved token purchase.
- Server-side purchased-minute and remaining-balance check plus a one-use signed wallet challenge before a LiveKit room/token is issued.
- Gemini realtime voice agent and LiveKit transport, with Spatius rendering the public demo avatar in the browser. Tavus remains available as a fallback provider.
- Product-aware preview: Ava explains the live demo, testnet minute packs when the API is configured, and the difference between Archava Onchain and custom business integrations. It receives the current website section through a restricted LiveKit data message and can guide the visitor to one of the four site sections.
- Optional 120-second anonymous preview for demos; disabled by default in `.env.example`. There is no per-browser, total-session, or concurrent-session usage quota.
- `POST /v1/sessions` for backend integration, bound to an API key's wallet and its remaining minute balance. Website and API sessions draw from the same durable usage ledger.
- One active room per wallet, up to 30 minutes reserved per room, early close returning unused seconds, and a room expiry sweeper.

The BNB Testnet contracts are deployed and verified. This app reads `AccessRented` purchase events for minute credits, checks active contract access, and meters room usage offchain. Paid calls stay unavailable until the API has the contract and token addresses, RPC, LiveKit credentials, and worker configured. No provider secrets are sent to the browser.

## Public deployment status

The [Archzxylabs/Archava](https://github.com/Archzxylabs/Archava) `main` branch is connected to the existing Vercel project. Pushes to `main` trigger production deployments.

The showcase is deployed at [archava.vercel.app](https://archava.vercel.app). Since October 3, 2026, `archava-onchain.vercel.app` permanently redirects to that domain with HTTP `308`, preserving paths, query strings, and browser fragments such as `#catalog-section`. The redirect is configured in the existing Vercel project's domain settings and remains independent of frontend builds. The Vercel project is still named `archava-onchain`.

[vercel.json](vercel.json) routes `/api/*` and `/v1/*` to the Railway backend at `archava-backend-production.up.railway.app`. On October 3, 2026, the updated frontend and voice-worker guidance were deployed. The API returned `200`, the worker registered with LiveKit, and the legacy domain's root, `/build`, query strings, API health path, and catalog fragment were verified through the redirect. Five production browser checks passed, including 320/390/1440px layouts, accessible mobile pages, visible wallet errors, and no avatar downloads on `/build`. These checks did not open a voice room or make a wallet transaction.

An earlier public preview on September 30, 2026, created a LiveKit room, received the host agent, and closed successfully. The UI shows an offline state if the API becomes unreachable. Paid calls depend on the contract/token addresses, RPC, LiveKit credentials, and worker configuration; a paid transaction was not repeated during the October 3 UI update.

The landing page's **For business** section shows potential custom integrations as hackathon examples. Without `VITE_BUSINESS_CONTACT_URL`, its CTA is **Try the live demo**, with a Build-page fallback when the preview is unavailable. Business contact is intentionally omitted for this showcase. A future deployment can set that public build variable to an official `mailto:` or WhatsApp link.

## Run locally

Use Node 22.18+ and Python 3.12 or 3.13. CI and Docker use Node 22 and Python 3.12.

```bash
npm ci
cp .env.example .env
# Fill the private QuickNode RPC_URL, LiveKit/Gemini credentials, and your
# selected avatar provider's credentials. Contract addresses are public defaults.
uv venv --python 3.12 .venv
uv pip install --python .venv/bin/python -r backend/requirements.txt
```

Run these in separate terminals from this directory:

```bash
npm run api
npm run dev
.venv/bin/python backend/agent.py dev
```

Open `http://localhost:5174`. Vite proxies `/api` to port 5002. For a local voice preview before contract deployment, set `ENABLE_PREVIEW=true` and configure LiveKit and Gemini.

## Avatar provider switch

Spatius is the active provider. Set `AVATAR_PROVIDER=spatius` and fill `SPATIUS_API_KEY`, `SPATIUS_APP_ID`, and `SPATIUS_AVATAR_ID` in the private `.env`, then restart both the API and Python worker. The current public demo uses the team's selected Spatius Avatar ID. The API key stays server-side; the frontend receives only the public App ID and Avatar ID. The browser loads avatar assets before requesting a live session so the preview timer starts when the avatar is ready. The API refuses to start in Spatius mode if any of the three values is missing. `AVATAR_PROVIDER=tavus` with `TAVUS_API_KEY` and `FACE_ID` is the fallback.

Spatius uses the LiveKit Agents plugin in the worker and AvatarKit's client renderer in the browser. Its LiveKit adapter requires `RTCRtpScriptTransform`; unsupported browsers see an explanation before entering the preview or paid session. The frontend loads Spatius code only for Spatius sessions and attaches the renderer to its LiveKit room before connecting. The single concept portrait is separate from the live Spatius avatar selected by `SPATIUS_AVATAR_ID`. Home retains background preparation for quick demo startup; `/build` skips avatar downloads until a session is requested.

The integration follows the [Spatius LiveKit server guide](https://docs.spatius.ai/livekit-agents/server), [client guide](https://docs.spatius.ai/livekit-agents/client), and [RTC adapter guide](https://docs.spatius.ai/sdk-reference/web-sdk/rtc-adapter). It was tested with the live Spatius avatar, LiveKit room, Gemini worker, and a browser microphone. The API explicitly dispatches `ARCHAVA_AGENT_NAME` and removes unrelated automatically dispatched agents from Archava rooms when a LiveKit project has other workers.

The live preview also shares the visitor's current section (`Home`, `The avatar`, `How it works`, or `For business`) with the voice worker. The worker accepts only those section IDs from a LiveKit guest, never page text or arbitrary URLs. The avatar can use a tool to read the current section or scroll to one of the four sections. Product facts are maintained in `backend/knowledge.py`; update them whenever minute packs or business integrations become active.

For temporary hosting from a local PC, run a second API process on port 5003 with `WEB_ORIGIN=https://archava.vercel.app`, `ENABLE_PREVIEW=true`, and `PREVIEW_SECONDS=120`. If the existing demo service is installed, inspect or restart it with `systemctl --user status archava-public-api.service` or `systemctl --user restart archava-public-api.service`. A Cloudflare Quick Tunnel can forward that port; using this alternative requires changing [vercel.json](vercel.json) to rewrite `/api/*` and `/v1/*` to the tunnel instead of the current Railway backend. Keep the API, tunnel, and Python worker running while sharing it. Quick Tunnel URLs change when the tunnel restarts; update the rewrite and redeploy if that happens. Preview usage is not capped; `ENABLE_PREVIEW=false` disables it.

For a single-server build, run `npm run build` and `npm run api`; the API serves `dist` on port 5002. Set `WEB_ORIGIN` to the browser origin (`http://localhost:5002` for local testing, or your public HTTPS origin after deployment). Run the Python worker as a separate process connected to the same LiveKit project.

## Testnet contracts

The app is wired to the verified BNB Testnet deployments:

| Contract | Address |
| --- | --- |
| ArchavaRentalV2 | `0xefaacd259136d40cd26a8672697b591c1f84200c` |
| Archava Mock USDT (mUSDT) | `0xd63574ac426169cda5495e1e196bf64d124267d0` |

The 60- and 300-minute packs use package IDs 1 and 2. Checkout reads `quoteRent`, requests the public test faucet when needed, approves the exact mUSDT amount, then calls `rent`. The API sums `AccessRented` events from `RENTAL_DEPLOYMENT_BLOCK`, checks `hasActiveAccess`, and subtracts completed and reserved room seconds from the shared usage ledger. This is testnet mUSDT, not official USDT or a production payment method.

Keep `RPC_URL` server-side and set it to the team's QuickNode BNB Testnet endpoint. `RENTAL_CONTRACT`, `PAYMENT_TOKEN_CONTRACT`, and `RENTAL_DEPLOYMENT_BLOCK` are public configuration; `CHAIN_ID` must stay `97`. Run one API instance with persistent `data/` storage for the hackathon demo.

## API flow

1. `GET /api/config` gives public chain and readiness settings. `POST /api/preview` starts an anonymous, limited LiveKit demo without a wallet.
2. For paid access, `GET /api/quote?minutes=60` or `300` reads the contract price; `GET /api/access?wallet=...` reports purchased, used, reserved, and remaining time.
3. `POST /api/challenge` issues a one-use message bound to a wallet and site origin. The wallet signs it without a gas transaction.
4. `POST /api/session` verifies the signature, checks remaining minutes, reserves up to 30 minutes, and creates a paid LiveKit room.
5. `POST /api/session/end` closes either room using an opaque ticket; the server also sweeps expired rooms every 10 seconds.

The API's nonce store and preview tickets are held in memory; paid reservations and usage are in `data/usage.json`. Run one API instance with persistent storage for the hackathon demo. A scaled service would use a transactional shared store and LiveKit webhooks for abandoned sessions.

## Checks

```bash
npx playwright install chromium
npm run verify:local
# Run separately against the deployed app; this sends GET requests only.
npm run preflight:readonly
```

For individual checks, use `npm test`, `npm run test:backend`, `npm run test:ui`, `npm run build`, and `npm run verify:artifacts`. Backend tests run with a clean environment and do not read the operator's private `.env`.

The tests cover preview room cleanup, minute reservations and settlement, signed challenge replay/expiry, customer API sessions, and allowed page context and navigation. The 20 browser checks cover 320–1440px layouts, keyboard access, dialog focus, offline recovery, clipboard feedback, avatar preload policy, and persistent call controls during agent navigation. They use isolated API and LiveKit fixtures without opening real rooms, microphones, wallets, or transactions. Screenshots and results are written to `.tmp-test/ui/`; `ARCHAVA_BROWSER_EXECUTABLE` can select an existing Chromium binary.

A full video call needs real LiveKit, Gemini, and the selected avatar provider's credentials; a paid session needs the team's deployed contract.

## Deployment workflow

See the [deployment guide](docs/deployment/DEPLOYMENT.md) for the Vercel/Railway release steps, worker readiness, persistent backend state, safe updates and rollback, and a portable Docker Compose deployment. The container includes the built frontend and supervises both the API and voice worker. CI repeats local verification and builds that image; production checks remain separate.
