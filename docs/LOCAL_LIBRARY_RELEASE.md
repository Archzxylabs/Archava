# Local reusable library release

The finite acceptance scope is PRD 2.1 §50. Company website, live voice/avatar,
public deployment and the broader ERP catalogue remain separate milestones.
Evidence below describes executed checks, not product/security percentages.

| Item                               | Status      | Evidence                                                                                                                                                                                  |
| ---------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| EXEC-01 supported profile          | Accepted    | 7 profile tests; two cold consumers with identical implementation hashes; 41 isolated persistent runtime checks including changed-package startup refusal, migration ledgers and restore. |
| EXEC-02 composition/lifecycle      | Not started | —                                                                                                                                                                                         |
| EXEC-03 presence boundary          | Not started | —                                                                                                                                                                                         |
| EXEC-04 security/dependency matrix | Not started | —                                                                                                                                                                                         |
| EXEC-05 isolated capacity baseline | Not started | —                                                                                                                                                                                         |
| EXEC-06 upgrade/recovery/handoff   | Not started | —                                                                                                                                                                                         |

The host embeds its trusted resolved profile at build time. The generated
`dist/library/release-profile.json` is an inventory; replacing its sidecar does
not change the host's supported implementation. Package content, internal
dependency versions, migration files and authoritative schema ledgers are
validated before CRM startup. This detects incompatible/tampered installations;
it is not a signature/provenance system against an attacker able to replace
the host binary and its trust root.

Package hashes cover implementation, declarations, migrations and semantic
package metadata. npm normalizes JSON whitespace, so JSON formatting is excluded
from the package metadata digest. Tarball byte hashes remain recorded separately.

Reproduce the profile checks with `pnpm test -- packages/config/test/release.test.ts`,
`pnpm reuse:verify`, `pnpm crm:build` and `pnpm crm:verify-runtime` on the pinned
Node/pnpm versions. Runtime verification creates and removes an isolated private
PostgreSQL installation; it does not replace the owner's data or call providers.

The current profile is `1.3.0` (host `0.6.0`, config `0.5.0`). Initial modules
are mandatory `contacts` (including activities/follow-ups), optional `pipeline`,
optional `assistant`, and `nativeActions` requiring `assistant`. No database data
is dropped. This is a logical composition within a modular monolith: the supported
installation keeps the common artifact closure; selection controls public surfaces
and execution, not uninstalling database ownership. Module metadata and the typed
`planAssembly` API ship in `@archava/config/assembly`. Legacy planning names in
the plan remain unresolved, never implicit installed capabilities.

After `pnpm crm:build`, use `pnpm library:assemble selection.json NEW_DIRECTORY
--dry-run`, then the same command without `--dry-run`. Selection has
`version: "1.0.0"`, the exact `profile`, and `modules` with all four boolean keys
(`contacts` must be true). Generated entries reference installed package exports;
no CRM implementation is copied. Run `npm install --ignore-scripts --no-audit
--no-fund` inside the new private consumer. `pnpm assembly:verify` reproduces the
independent-install, hash, dry-run, dependency and overwrite checks.

For an existing owner host, stop it, place the four-key module selection in a
privately owned mode-0600 file, and run `pnpm crm:modules /absolute/selection.json`.
Restart the host. Old profiles without module selection keep the previous full
behavior. Native disable refuses pending/executing/unknown intents; reconcile or
cancel through the original authorized host first. Startup independently checks
this policy even after manual settings edits. A transient operator connection
only checks unresolved-state existence; request execution still uses the existing
non-owner application role with RLS and current grants. Receipt GET recovery and
pending cancellation remain reconciliation operations, never new execution grants.

The shared `@archava/presence` boundary has strict versioned contracts and a
server-only session gateway. `POST /api/presence/session` accepts only a configured
`tenantId`; identity comes from a verified owner cookie or a server-issued guest
cookie. Owner presence uses current CRM authorization and the offline planner;
guests receive public help only. Neither transport accepts roles, grants, SQL,
approval tokens or execution tools. `/context` accepts an allowlisted section and
a strictly increasing revision; `/turn` binds the current revision, session and
request UUID. Sessions expire within 120 seconds (hard maximum 300); restart
invalidates these ephemeral scopes. Capacity and per-session requests are bounded.
Timeout aborts read work, reports a refusal and keeps the lane busy until late
work settles. Presence creates no new mutation/consent route.

Both public and owner replies use the existing `runTurn` intelligence/policy
implementation. The Python example is a standalone HTTP transport using only its
standard library and public guest scope. It runs from a copied isolated directory
with no owner/database environment. Synthetic transcript evidence is not a live
audio/avatar test. The existing Template LiveKit worker remains an independent
realtime consumer and retains its protocol; connecting its live provider path to
this gateway is a separate gated migration.

`pnpm security:verify` reruns unit, real PostgreSQL CRM/Act/host, reuse and
assembly proofs, then checks the control matrix against their executed labels.
It also scans both the root lock and the installed runtime's npm lock.
The [sanitized matrix](audit/local-library/security.json) records 15 applicable
controls, exact runtime dependencies, licenses/notices and both lock digests.
Each external runtime version is pinned in the host-owned catalogue, installed
using those pins and checked at startup. These pins capture the tested dependency
combination; they are not a vulnerability-driven forced downgrade.

The template's 13 moderate findings derive from
[GHSA-8988-4f7v-96qf](https://github.com/advisories/GHSA-8988-4f7v-96qf):
oversized inbound baggage can allocate excess memory; transport header limits
reduce the HTTP impact but do not prove safety of arbitrary transports. The
owner profile does not install this dependency chain. Template dependency/client
reachability has not been proved; vendor-compatible remediation and its missing
seven Python SDK integration checks remain open realtime release gates. No SDK
downgrade, override or claim of unexploitable code is inferred from scanner output.

External SPDX metadata and shipped notice files are inventoried; generated
`dist/security-proof/third-party-notices.txt` contains their included notices.
Internal artifacts remain private with no declared public license. Public
distribution and client IP terms need their own legal/contract review. This is
scoped engineering evidence, not a penetration test or certification.
