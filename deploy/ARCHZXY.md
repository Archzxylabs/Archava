# Archava at app.archzxy.com

Deployment target prepared for the owner on 4 November 2026. Public DNS/HTTPS
has not been changed. The existing `archzxy.com` site stays on Netlify.
Archava's current Rust/Axum server and PostgreSQL need a persistent server.

After that server is selected, follow [native installation](NATIVE_INSTALL.md)
and [deployment](README.md) with:

```bash
ARCHAVA_DOMAIN=app.archzxy.com
ARCHAVA_UPSTREAM=127.0.0.1:8347
```

These are Caddy environment settings; the runtime service separately uses
`ARCHAVA_BIND_ADDR=127.0.0.1:8347` and
`ARCHAVA_TRUSTED_PROXIES=127.0.0.1,::1` for host-local Caddy.

In the Cloudflare DNS zone, create an `A` record named `app` pointing to the
selected server's public IPv4 address, initially **DNS only**. Add `AAAA` only
if the server has working public IPv6. Open ports 80/443 for Caddy and keep
PostgreSQL and the runtime listener private. No apex DNS change is needed.

Keep the subdomain DNS only with the supplied Caddy configuration. Enabling
Cloudflare's HTTP proxy requires an explicit Caddy client-IP policy for
Cloudflare's current ingress ranges and a verified forwarding chain; otherwise
login budgets group clients by the Cloudflare edge address. Do not broadly
trust caller-provided forwarding headers to work around that grouping.

Accept the public deployment only after checking the actual hostname's valid
certificate, login/CSRF/security headers, two-client login budget isolation,
private metrics, production backup/restore and alert delivery. The local TLS
rehearsal uses its own CA and does not prove public DNS or ACME issuance.
