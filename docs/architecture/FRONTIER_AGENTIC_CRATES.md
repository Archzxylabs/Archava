# MCP, campaign, and RPA component boundaries

> **Status: current source reference — reviewed 29 October 2026.** The [former integration/specification draft](../archive/history/subsystem-drafts-2026-10-29/architecture/FRONTIER_AGENTIC_CRATES.md) is retained as history.

These are workspace components. The current host's dependency graph does not automatically expose all of them as production network services.

| Crate | Present in source | Boundary still requiring implementation or proof |
| --- | --- | --- |
| [mcp](../../crates/mcp/src/lib.rs) | JSON-RPC structures, tool/resource dispatch, external-tool registry | A deployed authenticated transport and real outbound MCP integration |
| [campaigns](../../crates/campaigns/src/runner.rs) | Trigger-to-plan conversion and registration of in-memory Cockpit missions | Continuous trigger ingestion, authenticated approval execution, real dispatch and delivery reconciliation |
| [rpa](../../crates/rpa/src/executor.rs) | Structured browser-action recipes and planning helpers | A real browser executor, runtime credential isolation, and complete redirect/navigation enforcement |

## MCP

`ArchavaMcpServer` handles methods such as `tools/list`, `tools/call`, `resources/list`, and `ping`. The library receives caller authority from its consumer; that is not a substitute for a transport deriving identity from an authenticated session.

[McpClientGateway::invoke_external](../../crates/mcp/src/client.rs) currently returns a formatted simulated-dispatch result. Registering a remote tool description does not establish a connection to Shopify, Google Drive, Slack, or another server.

## Evaluation

The `evals` crate was removed on 14 November 2026. It was never mounted; its cases were local string and payload heuristics, not runs against the host or a model, and its module comment described compliance certificates that nothing issued. Agent behaviour is evidenced by the host integration suites and the browser gate's agent journeys, including the live-provider run (`tools/e2e/run-agent-live.mjs`).

## Campaigns and RPA

A campaign plan can contain a proposed message and approval flag. The runner registers a mission and may launch its in-memory state. It does not itself prove that a message was sent, a purchase order committed, or a payment link settled.

RPA helpers return `RpaRecipe` action lists. Planning an allowlisted destination or masking a string does not demonstrate execution in a sandbox or successful interaction with a current banking/tax portal. This audit did not access such portals.

## Verification entry points

Library tests can be run with `cargo test -p archava-mcp -p archava-campaigns -p archava-rpa --lib`. A passing synthetic suite is scoped to those test cases; deployed integrations require separate evidence.
