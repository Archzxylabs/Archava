# Command, provider, and recovery boundaries

> **Status: current source reference — reviewed 29 October 2026.** The [earlier durable Act operating reference](../archive/history/subsystem-drafts-2026-10-29/architecture/DURABLE_ACT_OPERATIONS.md) describes the former platform.

## Current ownership

| Responsibility | Source |
| --- | --- |
| Session and HTTP mutation boundary | [host router](../../crates/host/src/router.rs) |
| CRM read/command/receipt processing | [host CRM](../../crates/host/src/crm.rs) |
| Client pending markers | [slice pending](../../crates/slice/src/pending.rs) |
| Read-only recovery UI | [slice recovery](../../crates/slice/src/recover.rs) |
| Owner plan/apply lifecycle | [owner engine](../../crates/own/src/lib.rs) and [host adapter](../../crates/host/src/own.rs) |
| Provider-attempt component | [providers crate](../../crates/providers/src/lib.rs) |
| Backup, restore, and release operations | [ops crate](../../crates/ops/src/lib.rs) |

## Recovery contract

A response can be lost after a write commits. The client keeps the original pending request identity and uses the status/read-only recovery path rather than silently creating a second intent. A missing receipt is not automatically proof that the first command failed. Exact request/response shapes belong to the typed handlers and their negative/replay tests.

The session/grant, stale-revision, command-identity, transaction, and receipt-visibility checks are separate concerns. Preserve their tests when extracting SQL or handler logic from the CRM host.

## Separate components are not automatic integration

The provider crate contains its own attempt/reservation/reconciliation abstractions. The current host dependency graph does not establish a live integration of every provider component into the product request path. Likewise, a mission checkpoint and a provider-delivery receipt are different records; one does not prove the other's committed outcome.

Operational commands are documented in the [developer guide](../DEVELOPER_GUIDE.md) and [deployment guide](../../deploy/README.md). The operator release verifier's older artifact/profile contract still needs alignment with binary/site packaging, as recorded in [current architecture](README.md).
