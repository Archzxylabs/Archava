# Earlier platform contracts

These references predate the root Rust cutover in whole or in part. They remain useful contract/migration context; their original TypeScript package paths, pnpm commands and release-profile assumptions are historical. Use [the developer guide](../../DEVELOPER_GUIDE.md) and [deployment](../../../deploy/README.md) to operate the current workspace.

| Document | Purpose |
| --- | --- |
| [CRM_BACKUPS.md](CRM_BACKUPS.md) | Automatic local CRM backups — host operation |
| [CRM_COMMAND_RECOVERY.md](CRM_COMMAND_RECOVERY.md) | Read-only recovery of a reviewed CRM Save |
| [CRM_CONVERSATION.md](CRM_CONVERSATION.md) | Reusable CRM conversation — controlled read/draft slice |
| [CRM_GEMINI_PILOT.md](CRM_GEMINI_PILOT.md) | Gemini CRM pilot — partial M3 |
| [CRM_OVERVIEW.md](CRM_OVERVIEW.md) | Authorized CRM overview — shared local operation |
| [CRM_PIPELINE.md](CRM_PIPELINE.md) | Reusable sales pipeline — controlled local slice |
| [CRM_RUNTIME.md](CRM_RUNTIME.md) | Persistent CRM host and identity — controlled local operation |
| [CRM_WORKLIST.md](CRM_WORKLIST.md) | Follow-up worklist — local owner operation |
| [LIBRARY_CONSUMPTION.md](LIBRARY_CONSUMPTION.md) | Internal library consumption — M1/M2 and local host |
| [PROVIDER_RUNTIME.md](PROVIDER_RUNTIME.md) | Durable provider budget and diagnostic metadata |

Current replacements: [durable commands](../../architecture/DURABLE_ACT_OPERATIONS.md), [host integration](../../guides/INTEGRATION.md#host-integration), and [browser embedding](../../guides/INTEGRATION.md#browser-embedding). Old operating procedures are not acceptance evidence for the Rust runtime.
