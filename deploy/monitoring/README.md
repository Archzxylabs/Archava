# Runtime monitoring

The runtime exposes bounded Prometheus counters, a fixed latency histogram,
process start time, and `archava_db_ready`. Readiness includes current writer
identity: a fenced or drained process must not remain ready. The public Caddy
route blocks `/metrics`; use a host-local scraper or an explicitly authorized
private scraper. A remote scrape sends `x-archava-metrics-token`, not a user
session. Keep its token in a secret file.

`prometheus.yml` targets a scraper running on the application host. Validate
both files with your installed Prometheus version before loading them:

```bash
promtool check config deploy/monitoring/prometheus.yml
promtool check rules deploy/monitoring/alerts.yml
```

Copy the configuration/rules to your monitoring deployment and configure its
Alertmanager route and actual on-call receiver. These files do not install or
start a monitoring service. A configuration file alone does not prove that an
alert reached an operator: test a stopped application and a controlled writer
drain on staging, then record detection and delivery times.

`alerts.yml` covers unavailable scrapes, database/writer refusal, sustained
server errors, and global p95 request latency. Its global 250 ms threshold does
not certify the separate 50 ms internal-read target. Metrics have no user,
tenant, URL, request-ID or provider-secret labels.

For incidents, preserve the current candidate identifier, recent JSON logs,
`/readyz` response, database health and writer epoch. Diagnose before restarting
or taking over a writer. Follow the [installation and recovery guide](../README.md).
Backups need an operator-owned schedule, off-host retention and periodic
isolated restore evidence; no schedule or notification receiver is enabled by
this repository template.
