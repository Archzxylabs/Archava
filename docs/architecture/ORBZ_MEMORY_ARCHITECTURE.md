# Orbz memory components

> **Status: current source reference — reviewed 29 October 2026.** The [earlier design draft](../archive/history/subsystem-drafts-2026-10-29/architecture/ORBZ_MEMORY_ARCHITECTURE.md) describes broader ambitions and is not a current capability guarantee.

## Ownership

| Component | Actual responsibility |
| --- | --- |
| [URI model](../../crates/orbz/src/uri.rs) | Parsing and rendering Orbz resource identifiers |
| [Progressive nodes](../../crates/orbz/src/progressive.rs) | Caller-provided summary/overview/detail fields and tier formatting |
| [Temporal coordinates](../../crates/orbz/src/temporal.rs) | Temporal metadata and active-time predicates |
| [Actor/scope policy](../../crates/orbz/src/security.rs) | Authority, owner, department, and entity-scope checks in library code |
| [Store](../../crates/orbz/src/lib.rs) | In-memory maps and accessible-node filtering |
| [Reflection](../../crates/orbz/src/reflection.rs) | Keyword-based candidate-fact extraction and confirmation metadata |
| [HTTP adapter](../../crates/host/src/orbz.rs) | Authenticated tenant handling, SQL writes, query/render responses, and database fallback |
| [SQL ledger](../../migrations/orbz-pg/001_agentic_memory_orbz.sql) | Memory and episodic persistence structures |

## Mounted surface

The host mounts `POST /api/v1/orbz/crystallize`, `/api/v1/orbz/query`, and `/api/v1/orbz/episodes`. The [handler request types](../../crates/host/src/orbz.rs) define their actual fields. The host derives ownership from membership rather than treating a high capability tier alone as ownership.

## Limits of the current abstraction

Progressive rendering selects text/detail fields; it does not generate their summaries or impose exact token budgets. `estimated_tokens` approximates rendered byte length divided by four. No 63% token-saving benchmark was established in this audit.

Reflection is a set of deterministic text patterns. A candidate's confirmation flag is metadata; it is not by itself an executed approval transaction or an autonomous overnight learning worker.

Library scope checks and database RLS are separate boundaries. The entity-scope branch currently checks a staff authority threshold, rather than proving membership of a particular entity. Avoid treating the policy type names as a complete entity authorization guarantee.

The query handler can populate its in-memory store from SQL when no accessible nodes were found, and ignores an unsuccessful database fallback read. That is not evidence of complete bi-temporal history recovery after a restart.

## Verification scope

`cargo test -p archava-orbz --lib` covers the library's declared cases. This page is a source map; tenant/department/entity negatives and persistence/restart behavior need their own fresh acceptance evidence.
