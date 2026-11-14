# AI data sharing

Owner settings select what the configured business-planning provider may receive.
This permission narrows each user's existing record access. It cannot create a
company membership, financial permission or right to execute a business action.
The [scoped native acceptance](../product/PRD_AI_NATIVE_BOUNDARY_ACCEPTANCE_2026-11-09.md)
binds its source, native package and observed tests. Its separate real-provider
run failed and remains active work; native transport evidence does not accept
live inference or the whole product.

## Owner journey

Open **Settings → AI data sharing**, review the displayed provider/model, and
choose business areas, categories and companies. Workspace records without a
company have their own checkbox. An empty company selection includes no
company-owned records. Accounting requires financial amounts and at least one
explicit company. Current messages must be selected for provider planning.

Preview displays the proposed categories and company names/codes. Confirmation
applies that exact versioned selection using a signed, ten-minute review. The
host rechecks current management grants, company access and provider identity.
Restoring configuration creates a new revision and requires a fresh review when
a sharing policy is involved. Historical policies are never merged into a newer
grant.

Use **Stop sharing** and confirm the review to revoke. Local planning and typed
business actions remain available without provider disclosure. A provider call
already in flight cannot be recalled; its response cannot authorize a further
lookup/model round after revocation. It cannot create a business goal after that
revocation is detected.

## Categories and boundaries

| Selection | Projected fields |
| --- | --- |
| Current messages | The current user message, with known credential patterns masked |
| Record references | Approved record IDs and revisions |
| Business facts | Allowed statuses, quantities, dates and currency metadata |
| Names | Allowed person, company, product and account names/codes |
| Contact details | Allowed contact fields, using the existing default contact mask |
| Notes | Allowed free-text notes, descriptions, reasons and source references |
| Financial amounts | Allowed prices, costs and exact string monetary units |

Unknown response fields and collections are withheld. Company records and their
source-organization/party/affiliation aliases retain company scope. Legitimate
counterparty information in a selected company's document follows that document's
scope. This does not promise semantic removal of every name or monetary phrase
inside arbitrary text. A current message or selected note can contain such text.

Raw prior messages, saved preference text, earlier goal objectives/summaries and
raw outcome payloads are withheld. All historical action inputs and record IDs
are also withheld: their derived text can outlive current record access. With
selected business-fact permission, earlier steps carry only registered tool names,
bounded indices and completion flags backed by matching approved mission receipts.
Overall state alone is not a verified completion. Fresh authorized reads supply
business values. The [historical row-scope finding](../product/PRD_PRIOR_CONTEXT_FINDING_2026-11-09.md)
and its regression describe this boundary. Complete safe historical memory and
broader partial-outcome acceptance remain separate work.

The provider sees only approved action contracts and typed lookups into its
emitted context. Routing sends the current sanitized message and permitted area
catalogue. Local authorization and review keep the full authorized snapshot;
the provider gets a separate projection. Session, sharing selection and authority
are rechecked before each round and after responses.

All serialized model input must fit 128 KiB. Per-source 32 KiB limits also apply.
Whole unprotected rows can be omitted with explicit coverage; oversized required
records refuse. A bounded page never establishes a complete business total.

## Provider changes and native operations

Approval binds the provider/model, endpoint, account credential identity,
reasoning profile, host identity key and projection version. Changing them makes
the old selection unusable for disclosure until reviewed again. Public settings
contain no API key or private endpoint path. This boundary does not verify the
provider's training, retention, residency or contractual terms.

The append-only Owner migration `004_ai_disclosure_boundary` and its startup
function advance the native compatibility profile. A previous binary must refuse
the upgraded database. Rollback uses an isolated pre-upgrade backup and its
matching binary; applied SQL is not reversed or edited.

See [Ava runtime](AGENT_RUNTIME.md), [Owner configuration](../product/OWNER_CONFIGURATION_PRD.md)
and [native installation](../../deploy/NATIVE_INSTALL.md). Whole-product,
real-provider, financial workflow and production acceptance are separate from
these implementation boundaries.
