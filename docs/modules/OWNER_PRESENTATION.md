# Owner terminology in CRM

The current contact-record response exposes `pipelineLabels` and
`configurationRevision` beside the authorized CRM record. The host reads this
narrow presentation snapshot in the same tenant transaction as its module state.
Only a successful current CRM record read returns it. Read-only staff can see
business terminology without Owner management grants; foreign tenant, assigned
record and revoked membership refusals do not return presentation data.

The contact view uses configured labels for the current opportunity stage and
stage picker. Command values remain canonical stage IDs and use the same
transition, current-authority and revision rules. Labels render as escaped text.
Older responses without presentation fields fall back to canonical captions;
older clients can ignore these additive response fields.

Configuration changes do not rewrite opportunity/transition rows. History retains
canonical stage meaning. Reconstructing the exact custom caption displayed at a
historical transition would require its configuration-version association; this
change does not claim that additional historical-label proof.

Explicit initial `archava-ops bootstrap-owner` provisioning now includes the
mounted customer/supplier party permissions alongside organization/company
permissions. This fixes initial business-identity setup. Runtime Owner plans,
module toggles, compatible config restore and database upgrades never mint those
grants for an existing principal. Existing tenants still need explicit authorized
administration when their business roles lack them.

The invoice posting policy's revision is loaded from the selected native book
workspace. Reviewed agent proposals retain their own optimistic version when
that workspace metadata is absent; the server remains responsible for current
policy checks. Fixed catalogue choices use declared options/enums rather than
requiring manual entry of their internal values.

The [Owner integration suite](../../crates/host/tests/own.rs) covers staff access,
no Owner privilege elevation, older response decoding, assigned/foreign/revoked
refusals and unchanged contact facts. The [native install suite](../../crates/ops/tests/native_install.rs)
checks initial identity permissions. The native browser helper observes actual
rendering, disable/recovery/chat/manual/restore and immutable business digests;
packaged rehearsal also tests fresh native party creation after real CLI setup.

These are current implementation contracts. The preceding
[portable/Commerce candidate](../product/PRD_PORTABLE_COMMERCE_ACCEPTANCE_2026-11-08.md)
records the original label and installer findings; it does not accept these newer
changes. Current-source native/WASM/full-gate/browser proof remains required.
Complete OWN-V1, all consumer profiles and real-user acceptance remain separate.
