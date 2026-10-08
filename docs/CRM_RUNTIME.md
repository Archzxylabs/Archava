# Persistent CRM host and identity — controlled local operation

The separate host in `apps/crm` consumes installed compiled library packages and
the same dashboard used by the two M2 consumers. Core and CRM have no mandatory
identity-provider dependency. `@archava/identity-pg` supplies this host's bounded
password/session implementation; another host can supply an external identity
adapter. This is a locally verified operational slice of M3. Public deployment,
TLS/proxy configuration, MFA/SSO, automatic recovery, provider integrations and
AI writes have separate release gates.

## Start

Use Node 22.22.3, pnpm 10.15.1 and PostgreSQL tooling (`pg_config`, `initdb`,
`pg_ctl`, `pg_dump`, `pg_restore`) under a non-root POSIX account.

```sh
pnpm crm:build
pnpm crm:setup
pnpm crm:start
```

The default address is `http://127.0.0.1:4175`. Setup generates an owner account
and stores its initial credential in `.local/crm/initial-owner.json`, mode 0600.
The password is never printed. Read it privately to sign in, then change the
password through the account form and remove the obsolete bootstrap file.
Repeat setup preserves accounts, records and secrets.

State lives in `.local/crm`, mode 0700, outside generated `dist`. Graceful shutdown
closes the host and the PostgreSQL process it started, preserving files. Admin
commands preserve an already-running database. PostgreSQL uses a private Unix
socket with TCP disabled. Local trust authentication relies on the OS account and
directory permissions; this installation serves the operator's machine.

`ARCHAVA_CRM_HOME` selects another private directory. `runtime.json` contains the
database/port, stable command fingerprint key, separate identity key and tenant
labels. Labels do not grant membership. Preserve the fingerprint key across
normal restarts; rotation needs explicit retry reconciliation. The two initial
configurations are hospitality and professional services. Existing `/workspace`
and `crm:dev` references retain their original demo behavior.

## Accounts

Enrollment is operator-only. Supply an owned mode-0600 JSON file:

```sh
pnpm crm:user add /absolute/private/account.json
pnpm crm:user reset /absolute/private/reset.json
```

Enrollment JSON contains `username`, `password`, `role` (`owner`, `staff`,
`viewer`) and a `tenants` array of configured ids. Reset JSON contains only
`username` and `password`. Enrollment/memberships commit atomically. Staff see
assigned records; owners see all authorized records; viewers cannot write.
Reset invalidates every old session and leaves an inactive account inactive.
Private credential files are temporary operator input, not a public import API.

Passwords contain 15–128 characters, retain spaces/unicode and use salted scrypt
with the OWASP 32 MiB profile (`N=32768`, `r=8`, `p=3`). Two concurrent KDF calls
bound per-process memory; excess work receives a retry response. No new hashing
dependency was installed. This uses
[Node's scrypt](https://nodejs.org/download/release/v22.22.3/docs/api/crypto.html#cryptoscryptpassword-salt-keylen-options-callback)
and an approved [OWASP profile](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#scrypt).

Sessions carry a random 256-bit token in an HttpOnly, SameSite=Strict cookie;
only keyed token hashes persist. Idle expiry is 30 minutes, absolute expiry eight
hours. Logout, password change/reset and disabled accounts invalidate future
authentication; an already-authorized request may finish. Durable login budgets
allow five failed account attempts per 15 minutes and 60 total source attempts
per five minutes. Source identity comes from the socket, not forwarding headers.
Unknown accounts perform the same fixed KDF work. Lifecycle events store metadata.

Every write requires the configured Origin; authenticated writes also require
a session-bound CSRF header. The browser stores that header in memory and stores
no business data/session credentials in localStorage. The verified identity then
passes through the existing CRM membership/resource checks. These controls follow
[OWASP session guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
and [CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).

Loopback HTTP is the default. A configured public origin must use HTTPS; cookies
then use `__Host-archava_session`, Secure, Path=/ and no Domain. The host still
binds loopback for a trusted TLS proxy. Public proxy/exposure, capacity and identity
policy require separate verification. Browsers cannot provision roles or accounts.

## Backup and recovery

```sh
pnpm crm:backup
pnpm crm:restore /absolute/private/backup.dump new_database_name
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:user recover /absolute/private/recovery.json
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:start
```

Backup produces a private custom-format database archive and adjacent
`.runtime.json` with fingerprint/configuration metadata. Preserve both as sensitive
backup material. Optional output paths must use a private directory; existing
archives are never overwritten. Records, memberships, command receipts, audit,
identity accounts and migration ledgers are included. Scheduling, off-machine
encryption/storage and retention remain operator work.

Restore accepts trusted owned archives, creates a fresh database and refuses an
existing target. It verifies migrations, reapplies grants and writes a new recovery
profile without changing the original profile/database. It preserves the command
fingerprint key, rotates the identity key, deletes restored sessions and makes
restored accounts inactive. Recover selected accounts with new passwords before
enabling access. Recovery JSON has the reset shape. Older backups therefore cannot
silently resurrect revoked sessions or old passwords. Original accounts are unchanged.
Failed restoration leaves its fresh target for inspection; it does not drop another
database. Use a fresh target/profile for another attempt.

The workflow uses [pg_dump's custom archive](https://www.postgresql.org/docs/current/app-pgdump.html)
and [pg_restore](https://www.postgresql.org/docs/current/app-pgrestore.html).
Tests cover actual initial-schema data, memberships, audit and retry receipts;
they do not establish a later schema upgrade, an RPO or an RTO.

## Verification and remaining gates

```sh
pnpm crm:verify-runtime
# Hold the isolated fixture for browser checks:
pnpm crm:verify-runtime -- --browser
# In a second shell:
pnpm e2e:crm-runtime
```

The verifier uses private disposable state and the compiled host/CLI, then removes
only its fixture. It does not read `.local/crm` or existing database credentials.
Proof is generated in `dist/crm-runtime-proof/results.json`. The CI CRM job runs
the runtime verifier; local browser evidence is separate. M1/M2 consumers and the
original suite remain regression gates.

`/healthz` exposes sanitized database status (200/503). Connection/request errors
contain no SQL, credentials or customer fields. Alert routing, telemetry aggregation,
cost budgets, public load/adversarial verification, MFA/SSO, email enrollment and
automatic backup retention remain deployment work. Machine CRM writes stay refused;
the Pending Act attempt/recovery tasks and shared realtime gaps remain open.
No company website or full ERP was added.
