# Security policy

## Supported versions

| Profile            | Supported | Notes                          |
| ------------------ | --------- | ------------------------------ |
| `1.19.x`           | Yes       | Current candidate              |
| `1.18.x` and older | No        | Upgrade to the current profile |

Only the current release profile receives routine security fixes. Older
profiles must upgrade; no backport is promised.

## Reporting a vulnerability

**Do not open a public issue for a security defect.**

Use GitHub's private vulnerability reporting for this repository
(**Security → Advisories → Report a vulnerability**). If that channel is not
available to you, contact the repository owner directly and include:

- the affected profile/commit and deployment shape (single-tenant or
  multi-tenant, which ledgers are mounted);
- the exact request or sequence that demonstrates the issue;
- the security boundary crossed (tenant isolation, authority, session,
  injection, file handling, supply chain, availability);
- a minimal proof of concept and its observed result;
- any suggested remediation.

Never include live credentials, session tokens, customer data, or production
backup material in a report. Use synthetic tenants and local databases.

## Scope

In scope:

- the Rust host (`crates/host`, `crates/adapters`,
  `crates/slice`) and its HTTP surface;
- tenant isolation, session/CSRF handling, authority and approval binding;
- historical TypeScript host/operator material recoverable from git history;
- backup, restore and release tooling that handles credentials or tenant data;
- committed configuration that weakens the documented security posture.

Out of scope:

- vulnerabilities in third-party dependencies with no demonstrated impact on
  this project (report them upstream as well);
- findings that require an already-compromised host, database owner
  credentials, or the ability to modify the deployment;
- social engineering, physical access, or denial of service against a
  deployment you do not own;
- missing hardening headers on responses that are not rendered by a browser,
  unless they enable a demonstrated attack.

## Response

Reports are acknowledged on a best-effort basis, normally within three business
days. Triage prioritizes reproducible boundary crossings over scanner output.
A fix is released with a new profile when the ledger or API changes; the
advisory records affected versions, the boundary, and the upgrade path.

## Defensive baseline

The platform already relies on these invariants. A report that breaks one of
them is a security finding, not a feature request:

- no tenant is inferred from a request body; every tenant comes from the
  authenticated session and is re-checked inside the database transaction;
- runtime roles are non-owner and every tenant table has forced row-level
  security;
- authority is resolved from membership rows, never from submitted role or
  tier fields;
- single-use approvals are bound to the command, tenant, actor and nonce;
- passwords use scrypt at the pinned parameters; session and CSRF values are
  HMAC digests, never raw tokens at rest;
- cookies are `HttpOnly`, `SameSite=Strict`, and `Secure` on a trusted HTTPS
  origin; CSRF is per-token;
- SQL is parameterized; HTML is escaped by default;
- backups are AES-256-GCM encrypted, and a missing key refuses rather than
  writing plaintext.
