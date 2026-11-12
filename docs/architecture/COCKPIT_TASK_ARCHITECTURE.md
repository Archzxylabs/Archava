# Cockpit mission components

> **Status: current source reference.** PostgreSQL persistence, restart and retry boundaries were revised during the [readiness takeover](../archive/audits/evidence/launch-2026-10-29/). This page distinguishes durable mission commands from execution of external business effects. The [earlier design draft](../archive/history/subsystem-drafts-2026-10-29/architecture/COCKPIT_TASK_ARCHITECTURE.md) remains historical.

## Ownership

| Component | Responsibility |
| --- | --- |
| [Mission and steps](../../crates/cockpit/src/mission.rs) | Mission data, unique step indices, dependency/cycle validation |
| [Flight status](../../crates/cockpit/src/flight_status.rs) | Legal mission-state transitions |
| [CockpitEngine](../../crates/cockpit/src/lib.rs) | Pure in-memory execution component; the host validates a proposed change on an isolated instance |
| [Checkpoint data](../../crates/cockpit/src/durable.rs) | Library checkpoint structure; its existence is not a replay scheduler |
| [Worker reports](../../crates/cockpit/src/subagent.rs) | Caller-supplied outcome, summary, payload and token-count metadata |
| [HTTP adapter](../../crates/host/src/cockpit.rs) | Authenticated database transactions, current grant resolution, mission locking, persistence and advisory cache publication |
| [SQL ledger](../../migrations/cockpit-pg/001_cockpit_durable_missions.sql) | Missions, steps, events and checkpoint snapshots |

## HTTP contract

The [router](../../crates/host/src/router.rs) mounts:

- `POST /api/v1/cockpit/missions`: create a validated mission; `auto_launch` sets its initial flight state in the same transaction.
- `GET /api/v1/cockpit/missions/{id}?tenant=TENANT`: read durable state.
- `POST /api/v1/cockpit/missions/{id}/steps/{step_index}/complete?tenant=TENANT`: record a typed worker result.
- `POST /api/v1/cockpit/missions/{id}/abort?tenant=TENANT`: abort with a reason.
- `POST /api/v1/cockpit/missions/{id}/steps/{step_index}/approve?tenant=TENANT`: record human approval under the selected policy.
- `POST /api/v1/cockpit/missions/{id}/run-next?tenant=TENANT`: execute an eligible registered read or approved business step.

Use the explicit `tenant` query parameter for ID-based operations so they work after a cold restart. Earlier callers may use a warm cache's tenant hint; the hint is only a selector and never authorizes access. Request fields are defined by the handler's typed structs.

The host authenticates the session and checks CSRF for writes. It resolves current membership within the same tenant transaction, using the existing membership-lock function so grant revocation cannot race the command. Assist-only members cannot mutate missions. Assigned-scope members are restricted to missions they initiated. Tool-specific business execution requires its own authorization boundary.

## Persistence and retry

The database is authoritative for HTTP reads and writes, including when a RAM projection exists. A mission row lock serializes mutations across processes. Mission reads share-lock the row while reading its steps.

Creation, optional launch, steps and the creation event commit together. Completion commits the step result, mission status, event and checkpoint together. All passed steps move the mission to `touchdown`; that status alone does not prove a financial invariant or an external provider effect. Abort status and its event also commit together.

A database failure returns a masked 503. A failed transaction does not publish the proposed state into the shared cache. The cache is updated only after commit and is never the authority for a later command.

An identical completed worker report can be retried without another effect or event. A changed report conflicts with 409; state/dependency conflicts also return 409. Legacy completed events without a full report cannot establish identical replay and must be inspected through the durable read path. Invalid fields or indices outside the SQL integer range are refused before insertion.

## Execution and worker limits

Registered business tools use the [finite dispatcher](../../crates/host/src/business.rs) and existing domain handlers. Business steps require approval and current domain grants. The stable mission/step request key survives a lost checkpoint; retry recovers the domain receipt without a second business effect. Caller-supplied completion cannot claim success for such a step. Native forms can create the reviewed mission, and mission detail shows its inputs and verified output. See [reviewed workflows](../modules/REVIEWED_WORKFLOWS.md).

Other worker reports remain caller-supplied metadata. These handlers do not implement an autonomous scheduler, general compensation engine, enforced compression ratio, or complete financial verification. External provider delivery still needs its own verified receipt and recovery boundary.

## Verification

[Product workflow tests](../../crates/host/tests/product_workflows.rs) cover all 13 additional native module contracts, real business effects, approval, forged reports and injected checkpoint failure. [Host regression tests](../../crates/host/tests/cockpit.rs) exercise real PostgreSQL under the non-owner runtime role: failed persistence rollback, auto-launch/abort failures, cold restart, dependency ordering, identical/conflicting replay, concurrent completion, assigned scope, revoked membership and CSRF refusal. [Raw red and green runs](../archive/audits/evidence/launch-2026-10-29/) retain their execution scope; they are not live-provider or deployment certifications.

```bash
cargo test -p archava-host --test cockpit --locked -- --test-threads=1
```
