# Remediation log — 27 October 2026

Scope: the dirty working tree audited on 27 October 2026, not only `HEAD`.
No commit or push was made. Evidence below is from this tree; commands that
cannot pass on a clean checkout without `dist/` say so.

## Fixed findings

| ID   | Finding                                                                       | Change                                                                                                                                          | Evidence                                                                                                            |
| ---- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| P1-1 | A peer actor could read a purchasing command it did not own                   | `command_status` now returns `Forbidden` unless `row_actor == principal.id`                                                                     | `status_refuses_a_peer_actor` fails with 200 without the guard, passes with 403; purchasing suite 10/10             |
| P2-1 | Template lockfile pinned a vulnerable `dompurify`                             | Added `overrides.dompurify = 3.4.17`; regenerated with `npx npm@10.9.4`                                                                         | 3-line lockfile diff; template `npm audit --audit-level=high` exits 0                                               |
| P2-2 | No readiness, metrics or structured request logs                              | `/readyz` (2 s DB probe), `/metrics` (label-free Prometheus), JSON `tracing`, INFO request spans                                                | host lib 56/56; `persistent_launcher` emits JSON and serves `/healthz`                                              |
| P2-3 | CI built the Rust slice but not the web bundle                                | `pnpm build:web` step in `.github/workflows/ci.yml`                                                                                             | `pnpm build:web` passes                                                                                             |
| P2-4 | Release evidence stopped at 1.17.0 while the profile moved on                 | `pnpm release:check`, corrected BUILD_STATUS/LOCAL_LIBRARY_RELEASE, tag/manual release workflow                                                 | `pnpm release:check` exit 0; workflow packs, verifies and emits SBOM                                                |
| P2-5 | README described the old build/gate scripts                                   | Corrected production build, gates, probes, deployment and backup sections                                                                       | `README.md`; no stale `build` = web claim                                                                           |
| P2-6 | Backups were plaintext                                                        | AES-256-GCM archive and metadata (`ARCHAVA_BACKUP_KEY`); legacy plaintext restore retained                                                      | `backup-crypto.test.ts` 5/5; `crm:verify-runtime` 44/44; upgrade rehearsal 4/4; purchasing host backup/restore PASS |
| P2-7 | Coverage floors were below measured coverage and critical paths were untested | Backup crypto tests, router readiness/metrics tests, release LICENSE boundary tests, and a full ratchet of all 54 scopes plus the global floors | `pnpm test:coverage` passes at lines 34.9 / functions 43.7 / branches 35.7 / statements 34.02                       |
| P3-1 | Revision ceiling silently clamped at `i32::MAX`                               | `i32::try_from` now refuses with `revision_changed()` instead of aliasing revisions                                                             | `cargo clippy -D warnings`; host lib tests                                                                          |
| P3-2 | No security policy, contribution guide, owners or licence                     | `SECURITY.md`, `CONTRIBUTING.md`, `.github/CODEOWNERS`, proprietary `LICENSE`                                                                   | files present; owner resolves to `@haikarure`                                                                       |
| P3-3 | No deployment artifacts                                                       | systemd unit, Caddyfile, env templates, Dockerfile, compose stack, PostgreSQL init, deployment runbook                                          | `caddy validate`, `systemd-analyze verify`, `docker compose config`, `docker build` all pass                        |
| P3-4 | No dependency automation or SBOM                                              | `.github/dependabot.yml` (root npm, template npm, cargo, Actions), `pnpm sbom` CycloneDX 1.5, release workflow artifact                         | `pnpm sbom` emits 525 components for profile 1.19.0                                                                 |

## Additional correction

Creating the root `LICENSE` made `pnpm pack` inject `LICENSE` into every
workspace tarball. `releaseImplementationHash` treated it as an unexpected
package file and the CRM runtime refused to verify. The verifier now ignores a
root `LICENSE` (it is a legal notice, not implementation) while still refusing
any other unexpected root entry. Two tests cover both sides, including a
cold-installed package that carries the injected licence.

## Verification run

- `pnpm test` — 166 files / 2,000 tests pass under the pinned Node 22.22.3.
- `pnpm test:coverage` — pass at the ratcheted floors above, also under Node
  22.22.3.
- `pnpm typecheck`, `pnpm lint`, `pnpm format:check` — pass.
- `cargo test --workspace --lib` — 379 pass.
- `archava-host` integration suites — 242 pass across 18 binaries.
- `cargo test -p archava-slice --test persistent_launcher` — 1 pass.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` — pass.
- `pnpm build`, `pnpm build:web` — pass.
- `pnpm crm:verify-runtime` — 44/44 under Node 22.22.3; `pnpm crm:verify-upgrade` — 4/4.
- `node tools/library/verify-purchasing-host.mjs` — full browser/host proof PASS, including encrypted backup restore.
- `node tools/rust/wasm-boundary.mjs`, `node tools/rust/erp/verify-purchasing.mjs` — PASS.
- `pnpm release:check`, `pnpm sbom` — pass.
- `docker build -f deploy/Dockerfile`, `caddy validate`, `systemd-analyze verify`, `docker compose config` — pass.

## Residual risks and accepted gaps

- `pnpm rust:verify` was not completed in one invocation: the full runner
  exceeds 40 minutes locally because several PostgreSQL fixtures take
  5–9 minutes each. Its individual gates were run above; this log does not
  claim the aggregate runner is green end-to-end.
- Coverage is materially better on the touched paths, but the aggregate is
  still 34.9% lines (`apps/crm` 14.95%, `apps/web` 12.36%). Raising it further
  is a test-writing program, not a release blocker.
- The template has 12 moderate npm advisories, 0 high/critical.
- `dist/releases/1.19.0` is intentionally absent: the verifier treats a slot
  matching the current identity as an overwrite. The last archived prior is
  1.17.0; archive 1.19.0 only when 1.20.0 opens.
- Backup encryption depends on `ARCHAVA_BACKUP_KEY` being injected from a
  secret manager. There is no KMS integration; a lost key means a lost archive,
  by design.
- The deployment runbook requires migrations to be applied by the owner role
  before boot; the runtime has no migration command and refuses on drift.
