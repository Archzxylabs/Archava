# Cold public-contract consumers

Agency/project services (K05) and wholesale/distribution (K04) import the same
packaged `archava-core` and `archava-domain` release. Each fixture uses shared
CRM contact/patch/pipeline contracts and tenant checks; the displayed labels and
project versus commerce extension differ. No core source is modified or forked.

Run from the repository root:

```bash
node tools/rust/cold-consumers.mjs --output target/cold-consumers/new-run
```

The helper creates the two public archives once, then copies those exact bytes
into two independent temporary roots with separate build targets. It generates
and locks each minimal manifest, checks the normal dependency tree, runs six
contract tests and the application in each root with Cargo offline, and removes
owned scratch state on success or failure. Only core, domain and the JSON wire
library are direct dependencies. Cached third-party Cargo dependencies and `tar`
are required. It does not copy private environment files, use a product database,
start a service, or publish a crate. Evidence records input/archive/fixture/lock
hashes, dependency trees, logs, exits and cleanup in the new output directory.

These are independent **contract consumers**. They do not establish configurable
owner labels through UI/chat, installed-module compatibility, persistent CRM
application behavior, upgrade paths for each complete business, or full K04/K05
customer acceptance. Those requirements remain separate from this portable-crate
check. The bank contracts also exercise exact signed allocation remainders and
complete large-value statement controls after packaging. Do not use these twelve
passing tests as whole-product readiness.

For the persistent client boundary, use a verified native distribution:

```bash
node tools/rust/persistent-consumers.mjs --native dist/<candidate> --output target/persistent-consumers/<new-run>
```

The helper checks native artifact hashes and release verification, then requires
the current Core/Domain inputs to match that native package. It packages the
public crates once and builds the [agency](persistent/agency.rs) and
[wholesale](persistent/wholesale.rs) clients in separate temporary roots/targets.
Both use the [shared public CRM client contract](persistent/shared.rs), with no
host, database, web-server or UI library in either client dependency graph.
Their transport port is JSON over standard input/output; the owned test driver
sends those exact envelopes to the real native HTTP boundary.

Two disposable databases and native hosts receive separate identity keys and
reviewed Owner label configurations. The helper verifies persistent contact and
canonical pipeline data, original command replay, current grant revocation,
forged observed-stage/stale-revision refusal, server restart, different project
and order extensions, and tenant/database/session isolation. It never connects
to an application database or live provider. All owned servers, PostgreSQL and
temporary directories close on success or failure; logs, crate archives, exact
client sources, locks, binaries and results remain in the new evidence directory.

This proves a scoped persistent client seam. It does not provide complete agency
project delivery, wholesale purchasing/fulfillment, their financial close, full
industry-kit screens, or actual customer acceptance. Owner UI/chat behavior has
its own browser tests. A CLI fixture is not a completed customer application.
