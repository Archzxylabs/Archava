# Read-only recovery of a reviewed CRM Save

The CRM/PG/assistant artifacts are now **0.3.0**. After a human saves a reviewed
follow-up, a lost response followed by a reload can be recovered from the existing
transactional command receipt. No additional package, database migration,
provider call or machine write permission is introduced.

## User flow

Review the contact, message and due date, then click **Simpan follow-up yang
ditinjau**. The authenticated host records an opaque request UUID in this browser
tab before sending the existing command. A verified response clears that marker.
If the response is lost, the same mounted card can still retry the same UUID.

After reload or returning to the same workspace/account, the panel checks the
original UUID through a read endpoint. A committed receipt shows **Penyimpanan
sebelumnya terkonfirmasi** and clears the marker. **Cek hasil penyimpanan** repeats
only that read. It does not resubmit a command, recreate the draft or call Gemini.

If the receipt is not visible, the result remains **belum terkonfirmasi**. A
transaction may still be running, and absence is not proof of failure. A new
draft's Save stays disabled until the earlier outcome is verified. Read/search
and local conversation remain available. There is no expiry that silently clears
uncertainty or starts a second command.

## Reusable contracts

`@archava/crm/contracts` exposes strict `mutationReceiptSchema` and
`commandStatusSchema`. `CrmService.commandStatus` uses the optional repository
capability of the same name; existing adapters still work for their original
operations. An adapter without this capability refuses recovery as unavailable.

```http
GET /api/crm/:tenant/commands/:requestId
Cache-Control: no-store
```

```json
{
  "status": "committed",
  "requestId": "00000000-0000-4000-8000-000000000001",
  "receipt": {
    "kind": "followup",
    "id": "00000000-0000-4000-8000-000000000002",
    "revision": 1
  }
}
```

An unavailable own receipt returns `{ "status": "unconfirmed", "requestId":
"…" }`. That is an observation, not an execution state. A committed receipt
describes the original command's revision, not the resource's latest revision or
whether a follow-up message was sent. Contact/activity/opportunity receipts are
also supported by the backend; this panel opts in only for follow-up Save.

`@archava/crm-assistant/recovery` exports `FollowupRecoveryStore` and
`createTabFollowupRecovery`. The component accepts an optional `recovery` store
alongside `commandsBase`. Hosts can select their own storage; the local CRM host
uses `sessionStorage`, namespaced by verified account name and workspace. It
stores one UUID, with no draft, contact id, CRM content, token or credential.
Storage failures stop a new Save before its outbound command. Existing consumers
that omit recovery retain their original behavior.

## Authority and privacy

The backend reads the command in a tenant transaction, matching its original
human actor. Another human's UUID is indistinguishable from an unavailable own
receipt. A caller with access to both workspaces cannot use the wrong workspace
to retrieve a receipt. Invalid IDs, missing sessions and inactive memberships
are refused. Service principals and the model's tool allowlist cannot use this
recovery route.

Before exposing a receipt, the repository checks current `contacts.read`, the
receipt kind's read grant and the actual resource's current assigned scope.
Opportunity receipts additionally use their existing contact/opportunity scope
checks. Revoked permissions or reassignment can therefore prevent an older
receipt from being displayed. Write permission is not needed for this read.
Nothing returns a fingerprint, command body, raw SQL or draft text.

Commands, receipts and mutation audit already commit atomically. Recovery uses
SELECT queries within the existing membership transaction and creates no command,
task or mutation audit. Malformed or mismatched responses never count as success.
Destroying the panel aborts its read and prevents late replies from restoring a
signed-out or different workspace's UI.

## Boundaries still open

This handles human Save receipt recovery, not autonomous Act approval/execution.
The three existing Act operational tasks remain Pending; M3 stays partial.
Unsaved drafts and conversations remain transient. The UUID survives refresh in
the same tab, not cleared browser storage, closing the tab or another device.
There is no cross-device recovery inbox or operator reconciliation workflow yet.

A persistently unconfirmed outcome needs authoritative investigation before a
new intent is allowed; no automatic retry/forget is inferred. Clearing browser
storage is not proof that the first command failed. If a restored database lacks
a newer receipt, that absence likewise remains uncertain. Existing provider
budget, Free-tier synthetic-data boundary and server authority are unchanged.

See [verification evidence](CRM_COMMAND_RECOVERY_2026-10-09.md).
