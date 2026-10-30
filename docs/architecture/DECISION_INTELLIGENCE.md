# Decision component boundaries

> **Status: current source reference — reviewed 29 October 2026.** The [previous TypeScript implementation reference](../archive/history/subsystem-drafts-2026-10-29/architecture/DECISION_INTELLIGENCE.md) is historical.

The Rust decision library is under [crates/context/src/decision](../../crates/context/src/decision/mod.rs). It evaluates whether a provider response may replace a caller-supplied deterministic baseline. It is separate from command authorization, SQL permissions, and execution.

| Area | Source |
| --- | --- |
| Provider request/response seam | [provider.rs](../../crates/context/src/decision/provider.rs) |
| Task and answer vocabulary | [tasks.rs](../../crates/context/src/decision/tasks.rs) |
| Modes and per-task policy | [policy.rs](../../crates/context/src/decision/policy.rs) |
| Response validation, fallback, and effective answer | [orchestrator.rs](../../crates/context/src/decision/orchestrator.rs) |
| Turn-level composition | [turn.rs](../../crates/context/src/decision/turn.rs) |

`off` avoids a provider call; `shadow` evaluates the provider while retaining the baseline; `assist` can apply an acceptable answer under task-specific rules. Confidence is not permission. There is no single global 0.85 confidence threshold for every task.

The orchestrator uses a synchronous provider seam. Its elapsed-time judgment occurs after a call returns; it should not be documented as a transport that cancels an in-flight request at the deadline.

The host's current Cargo dependency graph does not establish that this library is the universal front door for every request. A provider implementation, live transport, host composition, and product acceptance need separate wiring and evidence. Library tests live in [context tests](../../crates/context/tests/context.rs).
