# Orbz memory components

> **Status: current source reference — updated 4 November 2026.** The [earlier design draft](../archive/history/subsystem-drafts-2026-10-29/architecture/ORBZ_MEMORY_ARCHITECTURE.md) describes broader ambitions and is not a current capability guarantee.

## Ownership

| Component | Actual responsibility |
| --- | --- |
| [URI model](../../crates/orbz/src/uri.rs) | Parsing and rendering Orbz resource identifiers |
| [Progressive nodes](../../crates/orbz/src/progressive.rs) | Caller-provided summary/overview/detail fields and tier formatting |
| [Temporal coordinates](../../crates/orbz/src/temporal.rs) | Temporal metadata and active-time predicates |
| [Actor/scope policy](../../crates/orbz/src/security.rs) | Authority, owner, department, and entity-scope checks in library code |
| [Store](../../crates/orbz/src/lib.rs) | In-memory maps and accessible-node filtering |
| [Reflection](../../crates/orbz/src/reflection.rs) | Keyword-based candidate-fact extraction and confirmation metadata |
| [HTTP adapter](../../crates/host/src/orbz.rs) | Authenticated tenant handling and authoritative tenant-scoped SQL reads/writes |
| [SQL ledger](../../migrations/orbz-pg/001_agentic_memory_orbz.sql) | Memory and episodic persistence structures |

## Mounted surface

The host mounts `POST /api/v1/orbz/crystallize`, `/api/v1/orbz/query`, and `/api/v1/orbz/episodes`. The [handler request types](../../crates/host/src/orbz.rs) define their actual fields. The host resolves and locks an active human membership inside the same tenant transaction as each read/write. Ownership requires the Owner role and tenant-wide scope. Capability alone cannot confer ownership. Episode identity and role come from that verified membership, regardless of body claims.

## Limits of the current abstraction

Progressive rendering selects text/detail fields; it does not generate their summaries or impose exact token budgets. `estimated_tokens` approximates rendered byte length divided by four. No 63% token-saving benchmark was established in this audit.

Reflection is a set of deterministic text patterns. A candidate's confirmation flag is metadata; it is not by itself an executed approval transaction or an autonomous overnight learning worker.

Library scope checks and database RLS are separate boundaries. The HTTP host restricts department and entity memories to tenant-wide owners until persisted department/entity grants exist. The optional department input is only a narrowing filter. Public memory writers require a tenant-wide human Editor/Admin/Owner membership and sufficient capability; sovereign writes require an Enterprise Owner. Assigned-scope writers cannot create tenant-wide knowledge. The lower-level library policy remains a primitive, not a substitute for host authorization.

The query handler always reads PostgreSQL inside its tenant transaction, including on a warm host. Database failures return 503 instead of an empty success. Queries use original persisted validity intervals; current-time queries sample the database clock. A caller may select a historical validity instant. The HTTP host does not use the in-memory store as an authority or an unbounded cache. Node fields are bounded and scope tags use their stable SQL vocabulary. This does not establish a full autonomous reflection/invalidation workflow.

## Verification scope

`cargo test -p archava-orbz --lib` covers the library's declared cases. The permanent [host regression suite](../../crates/host/tests/production_hardening.rs) also covers forged department claims, sovereign writes, Entity persistence, revoked memberships, role claims, cold restart, historical validity and unavailable SQL. Dated command results belong to the release candidate evidence, not this source map.
