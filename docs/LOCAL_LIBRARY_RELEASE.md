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
