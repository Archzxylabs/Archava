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

The current profile is `1.1.0` (host `0.4.0`, config `0.3.0`). Initial modules
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
