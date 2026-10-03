# Archava deployment

Archava follows the useful parts of the Archcore deployment workflow: separate local verification from live evidence, build from a lockfile, keep operator configuration outside source, persist backend state, and run one state-writing API instance. Archava continues to use Vercel for the public frontend and Railway for the API and voice worker.

## Runtime and configuration

Use Node 22.18+ and Python 3.12. `.nvmrc`, CI, and the Docker build use Node 22. The container uses Python 3.12, pinned voice plugins, and production Node dependencies installed from `package-lock.json`. A lockfile mismatch fails the build; there is no `npm install` fallback.

Frontend build configuration is public. Only `VITE_BUSINESS_CONTACT_URL` is optional public configuration here; leave it empty for this hackathon. LiveKit, Gemini, Spatius, Tavus, and credentialed RPC settings stay in Railway variables or a private operator env file. Never put them in `VITE_*` variables or copy them into Git, build artifacts, screenshots, or CI logs.

`ARCHAVA_STATE_DIR` selects the directory containing `keys.json` and `usage.json`. It defaults to `data/` locally and `/app/data` in the container. These files contain key digests, wallet usage, and paid reservations. Mount persistent storage at that path for paid/API use. A writable container directory alone does not prove persistence.

Run exactly one API replica with this JSON store. Dashboard logins, challenges, and preview tickets are intentionally in memory; a restart can require a new sign-in or demo. Persisting the paid ledger does not persist every session.

## Verify before publishing

```bash
npm ci
# Install once in an uncommitted virtual environment.
python3 -m venv .venv
.venv/bin/python -m pip install -r backend/requirements.txt
npx playwright install chromium
npm run verify:local
git diff --check
git status --short
```

`verify:local` runs application tests, backend tests, browser QA, a production build, and artifact checks. Backend tests receive a small clean environment and never load the real operator `.env`; importing worker prompts/tools also does not read it. Browser QA uses isolated API/LiveKit fixtures. These checks prove their own layer and do not prove a real microphone, provider video, wallet transaction, or production deployment.

Commit the reviewed source before a production release and record the commit alongside deployment IDs. The GitHub workflow repeats verification and builds the Docker image without provider credentials. Existing Git integrations may auto-deploy `main`; the workflow itself does not pause those integrations. Required branch checks must be configured separately if releases should wait for CI before merging.

## Railway backend

The current service is `archava-backend` in `production`. The API and Python worker run in one container. `railway.json` requests one replica, on-failure restart, and `/api/ready` as its deployment health check.

`/api/health` checks the HTTP process. `/api/ready` also probes the worker's private loopback health endpoint when `ARCHAVA_WORKER_HEALTH_URL` is configured. Docker sets `http://127.0.0.1:8081/`; no worker address or provider error is returned to visitors. Worker health and startup registration are separate evidence from a successful voice call. See [LiveKit server health checks](https://docs.livekit.io/agents/server/options/).

Before adding a volume, inspect and privately back up existing `keys.json` and `usage.json`. Attaching an empty mount over an existing data directory hides its old files. Preserve them before the deployment, restore them to the mounted directory, and verify their digests before reopening paid/API access. Do not delete a volume to resolve a build problem.

```bash
railway status
railway volume list --json
# For a new/empty service; existing state must be backed up and migrated first.
railway volume --service archava-backend --environment production add --mount-path /app/data --json
railway up --service archava-backend --environment production --detach
railway deployment list --service archava-backend --environment production --limit 3 --json
```

Use the same `WEB_ORIGIN`, LiveKit project, and `ARCHAVA_AGENT_NAME` for the API and worker. Keep service sleeping disabled to avoid a cold demo startup. Release between demo sessions after checking for active Archava rooms; do not close visitors' calls to make a deployment convenient. A service with a mounted volume may have a short deployment interruption, as documented by [Railway health checks](https://docs.railway.com/deployments/healthchecks).

The entrypoint supervises both processes. An unexpected exit, including exit code zero, stops its peer and returns a failing status so the platform can restart it. A planned SIGTERM stops both and exits normally.

## Vercel frontend

The linked Vercel project is still `archava-onchain`; the public domain is `archava.vercel.app`. API rewrites point to the Railway service. The old `archava-onchain.vercel.app` domain retains its native permanent redirect.

Build without moving the main domain, check the deployment, then promote:

```bash
vercel deploy --prod --skip-domain --yes
# Replace NEW_DEPLOYMENT with the URL returned above.
vercel curl / --deployment NEW_DEPLOYMENT
vercel curl /build --deployment NEW_DEPLOYMENT
vercel curl /api/ready --deployment NEW_DEPLOYMENT
vercel promote NEW_DEPLOYMENT --yes
npm run preflight:readonly
```

Keep the previous frontend deployment URL for rollback with `vercel promote PREVIOUS_DEPLOYMENT --yes`. A frontend rollback does not roll back backend code or state. For a backend rollback, use the previous known-good image/deployment, retain the same volume, and repeat readiness checks; confirm its code understands the current ledger format.

## Portable single-server deployment

The same Docker image includes the built frontend, API, and worker. Compose mounts a named volume and binds port 5002 to loopback; put an HTTPS reverse proxy in front of it for public use.

```bash
# Keep this file outside the public repository and restrict its permissions.
ARCHAVA_ENV_FILE=/absolute/path/to/private/archava.env docker compose up --build -d
docker compose ps
curl --fail http://127.0.0.1:5002/api/ready
npm run preflight:readonly -- --origin http://127.0.0.1:5002
```

Set `WEB_ORIGIN` in that env file to the browser's actual public HTTPS origin. Compose overrides the internal port, state path, and worker health URL. `docker compose down` keeps the named volume; `down --volumes` removes it and must not be used as a routine update command.

## After release

`preflight:readonly` makes only public GET requests: API health/readiness/config, testnet pack quotes when configured, and Home/Build routes. It never reads operator env, creates a room, signs a challenge, or transacts. It fails if production worker health is unconfigured. Verify the old domain's path/query/hash redirect in a browser as well.

Then test the actual demo separately: microphone permission, first voice reply, avatar video, mute/unmute, Ava's section navigation, end-call cleanup, and expiry. Record the browser and observed result. A green mock suite or healthy worker cannot substitute for that check.

Keep sanitized release evidence in operator storage: commit, frontend/backend deployment IDs, verification results, and backup location. Keep credentials, raw state, and private session tokens out of the public report.
