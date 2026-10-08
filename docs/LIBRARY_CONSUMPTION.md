# Internal library consumption — M1/M2 and local host

ARCHZXY owns the internal library; Archava is the flagship product. The company
website remains an independent future consumer. This milestone distributes
artifacts internally without publishing a live service or public package.

Use Node **22.22.3** and pnpm **10.15.1**. Workspace development continues using
TypeScript source; internal distribution is compiled ESM JavaScript plus `.d.ts`
declarations. Class identity is preserved between root/subpath exports.

## Build and prove reuse

```sh
pnpm library:pack
pnpm reuse:verify
```

Artifacts and checks are generated under ignored `dist/`:

| Output                                   | Purpose                                                                   |
| ---------------------------------------- | ------------------------------------------------------------------------- |
| `dist/library/tarballs/*.tgz`            | Thirteen private internal artifacts; per-package versions in the manifest |
| `dist/library/artifacts.json`            | Artifact versions and tarball SHA-256                                     |
| `dist/library/consumer-proof.json`       | Installed implementation hashes and consumer results                      |
| `dist/reuse-demo/hospitality/index.html` | Offline hospitality browser embed                                         |
| `dist/reuse-demo/retail/index.html`      | Offline retail browser embed                                              |

The verifier copies each [independent consumer](../examples/consumers) into a
separate temporary directory outside the workspace, installs identical tarballs,
runs its own TypeScript/build commands, then exercises the public assistant,
SDK, knowledge and Chat APIs. No workspace path, `NODE_PATH`, bundler alias or
source import is required. Temporary installations are removed after verification.
Pinned public tools/Zod may be downloaded from npm; the fixture brain makes no
network calls. This is evidence for controlled internal artifacts, not a public
package release/support commitment.

Knowledge, branding and allowed page actions differ between the consumers;
core implementations do not. Tests refuse a cross-tenant turn and an unsafe
model-requested action. Browser builds include the shared Chat renderer. Serve
`dist/reuse-demo` through a local HTTP server to inspect both embeds.

## Public boundaries

| Entry                                                                                               | Environment                                                              |
| --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `@archava/config/pure`                                                                              | Browser-safe schemas, money vocabulary, thin registry/planning contracts |
| `@archava/config`, `@archava/config/client`, `@archava/config/pricing`, `@archava/config/templates` | Server loaders/resolver; filesystem access stays here                    |
| `@archava/core`, `@archava/sdk`, `@archava/assistant`, `@archava/chat`                              | Browser-compatible public APIs, also exercised under Node                |
| `@archava/acl`, `@archava/knowledge`, `@archava/adapters`, `@archava/decision`                      | Public dependencies included in the same artifact set                    |

Server catalog loaders require a host-owned catalog/path. Pricebook/template JSON
and credentials are outside the library artifact allowlist. Production hosts
supply authoritative catalogs, authentication and permission checks; browser
tenant ids and offline role fixtures do not authenticate a visitor.

## V1 compatibility and V2 deployment planning

`ClientConfigResolver.resolve()` is deprecated, delegates to `resolveLegacy()`
and preserves old demo defaults. Its `enabled` flags are not proof of connectivity.
Keep this compatibility path for existing reference callers.

New consumers use `ClientConfigResolver.plan(rawDeclaredConfig)` or the pure
`buildDeploymentPlan(config, hostTemplates, hostRegistry, hostEvidence)` contract.
The plan is version **2.0.0**; input client configs remain schema **1.0.0**. Unknown
input versions are refused. The quote API adds `deploymentPlan` while retaining
the existing `set` and `config` response fields.

- Template modules/integrations remain separate suggestions.
- Declared integrations retain their quoted complexity; absent integrations stay absent.
- `requested`, `supported`, `configured`, `connected` and `healthy` are distinct.
- Positive integration evidence comes from a trusted host and matches the tenant.
  `checkedAt` is a snapshot stamp; refresh health before live execution.
- Unknown/duplicated modules, dependency cycles, missing dependencies and explicit
  dependency opt-outs fail. Planned modules produce blockers rather than become available.
- A host explicitly registers implementation manifests. Metadata permissions never
  grant permissions. Dependency versions are pinned snapshots; semver range solving,
  migrations, rollback and a full installer remain later work.

For a legacy config already expanded by `resolve()`, recover the original declared
scope or have an operator reconcile it against the quote. It is impossible to
infer which enabled defaults were purchased just from the expanded V1 output.
Do not feed that expanded output into provisioning and assume it was reconciled.

`plan.ready` means this planning snapshot has no listed blocker. It never means
authorization, tenant authentication, provider release readiness, payment success
or permission to execute an action. The existing ActionPolicy/ActHost/executor
boundaries remain authoritative.

## Remaining work

The optional server-only `@archava/identity-pg` package is used by the
[persistent CRM host](CRM_RUNTIME.md). The host cold-installs compiled artifacts
and reuses the shared dashboard; core/CRM do not depend on this identity adapter.

The optional `@archava/crm-assistant` artifact adds `/contracts`, `/panel`, `/pilot`
and server-only `/server` exports using the existing turn pipeline. The artifact set contains
13 packages. Hospitality and professional-service CRM consumers cold-install
matching conversation implementations and exercise their authorized live data.
See [CRM conversation](CRM_CONVERSATION.md) and the [bounded Gemini pilot](CRM_GEMINI_PILOT.md).
The synthetic live evidence and explicit human Save do not establish general
model quality or machine-write readiness.
This proves local login/revocation and backup restoration, not public deployment.

The first persistent contacts + activities/follow-up CRM slice now has two
independent consumers, an API, a shared dashboard and permission-bound read/draft
tools. See [CRM module](CRM_MODULE.md). Broader CRM and ERP remain future work.
`@archava/crm` and `@archava/crm/dashboard` are browser-safe; `@archava/crm-pg`
is server-only and includes its owned migration. Runtime setup needs a host
identity adapter, membership provisioning and a stable fingerprint key.

Live Act still needs trusted identity, durable host attempts and conservative
provider recovery. The Python realtime template has no shared-core bridge yet.

The 7 October audit remains historical evidence. The Nx placeholder is archived
under `docs/history/nx.placeholder.json`; [task history](../delegation/README.md)
keeps completed rounds separate from the three Pending operational tasks.
