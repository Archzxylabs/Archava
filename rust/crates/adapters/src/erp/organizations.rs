//! The organizations SQL adapter — the Archxy 2 port of
//! `packages/organizations-pg/src/repository.ts` (the R04 organization +
//! affiliation mutation surface), executed strictly inside the shared RLS
//! transaction cohort.
//!
//! ## Statement order (the TS port)
//!
//! Every mutation arms the same ladder: membership row (FOR SHARE through
//! the `lock_membership` SECURITY DEFINER the SQL adapter reads), the
//! human/service on-behalf intersection, `requiredOrgPermission`, the
//! `commands` claim, then the arm's effect, the audit row and the receipt
//! binding. Refusals roll back through the transaction the host owns — the
//! same contract the R06 purchasing adapter established.
//!
//! The arms:
//! - `create_organization` inserts the active org owned by the actor.
//! - `update_organization` keeps absent/null cells (the SQL
//!   `COALESCE`/`CASE $::text IS NOT NULL` branches), refuses archived
//!   organizations and stale revisions.
//! - `archive_organization` archives AND deactivates the still-active
//!   affiliations, recording the reason in audit.
//! - `create_affiliation`/`update_affiliation` uphold the one-active-primary
//!   rule the database's partial unique index also enforces
//!   (`primary_conflict` — the TS check surfaces it first).
//! - `remove_affiliation` sets the affiliation `status='inactive'` — no
//!   DELETE anywhere, and the TS does the recorded-org gate BEFORE the
//!   archived gate here.
//!
//! ## Error vocabulary
//!
//! The TS `OrganizationError('forbidden'|'not_found'|'conflict'|'archived'|'primary_conflict')`
//! rides this crate's shared `PurchaseError`/`ErpError` carrier. The two
//! cohort-specific words lose their names — both map to conflict-class
//! refusals (409). The DOMAin-level message loss is the same flattening the
//! purchasing port records (refusal semantics, not word-for-word texts),
//! kept deliberately: a silent extra error surface would duplicate the
//! baseline's contract instead of porting it.

use sqlx::postgres::PgConnection;
use uuid::Uuid;

use archava_domain::erp::organizations::{
    replayed_receipt, OrgAccess, OrganizationCommandStatus, OrganizationData, OrganizationMutation,
    OrganizationReceipt, ORGANIZATION_PERMISSIONS,
};
use archava_domain::erp::purchasing::PrincipalKind;
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::purchase_database_error;
use super::PurchaseError;

/// The ledger label on database errors.
pub const ORGANIZATIONS_LEDGER: &str = "organizations";

/// The `archava_organizations` namespace — checked constants only.
pub const ORGANIZATIONS_NAMESPACE: &str = "archava_organizations";

fn refuses(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(ORGANIZATIONS_LEDGER, error)
}

/// A conflict-mapped `i32` conversion for the int4 audit columns (same rule
/// the purchasing mapper carries; the schema bounds every `revision`).
fn int4(value: i64) -> Result<i32, PurchaseError> {
    if value < 1 || value > i32::MAX as i64 {
        return Err(conflict());
    }
    Ok(value as i32)
}

fn conflict() -> PurchaseError {
    PurchaseError::Domain(ErpError::Conflict)
}

fn forbidden() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}

fn not_found() -> PurchaseError {
    PurchaseError::Domain(ErpError::NotFound)
}

fn parse_uuid_value(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

/// The committed instant every receipt stamps — `to_char` renders an
/// RFC-3339 UTC instant the receipt schema accepts (the TS surfaces the
/// driver's JS Date through `instant()`; the SQL text is this port's render
/// of the same column).
async fn committed_at(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map(|(text,)| text)
    .map_err(refuses)
}

// --- The context ladder ------------------------------------------------------

/// The membership ladder — ONE ledger for this module. Loads the actor's
/// own grant, refuses inactive/unknown/shape-foreign rows, intersects with
/// the human principal a service names, and hands the access envelope back.
pub async fn resolve_organizations_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<OrgAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        ORGANIZATIONS_NAMESPACE,
        ORGANIZATIONS_LEDGER,
        tenant,
        &principal.id,
        ORGANIZATION_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
        return Err(forbidden());
    }
    Ok(match principal.kind {
        PrincipalKind::Human => OrgAccess {
            permissions: own.permissions.clone(),
            owner_id: principal.id.clone(),
            scope: own.scope,
        },
        PrincipalKind::Service => {
            let on_behalf_of = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
            let human: MembershipRow = load_locked_membership(
                connection,
                ORGANIZATIONS_NAMESPACE,
                ORGANIZATIONS_LEDGER,
                tenant,
                on_behalf_of,
                ORGANIZATION_PERMISSIONS,
            )
            .await?
            .ok_or_else(forbidden)?;
            if human.kind != ErpMembershipKind::Human || !human.active {
                return Err(forbidden());
            }
            OrgAccess {
                permissions: own
                    .permissions
                    .iter()
                    .filter(|held| human.permissions.contains(*held))
                    .cloned()
                    .collect(),
                owner_id: on_behalf_of.to_string(),
                scope: match (own.scope, human.scope) {
                    (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                    (ErpScope::All, ErpScope::All) => ErpScope::All,
                },
            }
        }
    })
}

// --- The claim + audit + receipt pipeline ------------------------------------

async fn claim_organization_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed = sqlx::query(
        "INSERT INTO archava_organizations.commands(tenant_id,request_id,actor_id,fingerprint) \
         VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(claimed.rows_affected() == 1)
}

async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_organizations.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The organizations audit row — `(tenant, request, actor, operation,
/// organization_id, affiliation_id?, revision, reason?)`.
#[allow(clippy::too_many_arguments)]
async fn insert_organization_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    organization_id: Uuid,
    affiliation_id: Option<Uuid>,
    revision: i64,
    reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_organizations.audit(tenant_id,request_id,actor_id,operation,organization_id,affiliation_id,revision,reason) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(organization_id)
    .bind(affiliation_id)
    .bind(int4(revision)?)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// The organizational owner read recovery's scope re-check replays through.
async fn organization_owner(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<Option<String>, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT owner_id FROM archava_organizations.organizations WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map(|row| row.map(|(owner,)| owner))
    .map_err(refuses)
}

/// The mutation pipeline — the port of `mutate()`. The access ladder runs
/// FIRST in the caller's transaction; the human-only kind gate is both a
/// route and TS rule; every refusal rolls the WHOLE transaction back. The
/// host hands over the ALREADY STRICT-PARSED mutation (its boundary parse
/// answers 400), so the fingerprint and the effect read one document.
pub async fn record_organization_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    fingerprint: &str,
    mutation: &OrganizationMutation,
) -> Result<OrganizationReceipt, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    if !access.holds(&[mutation.data.kind().permission()]) {
        return Err(forbidden());
    }
    let request_id = parse_uuid_value(&mutation.request_id)?;
    let actor_id = principal.id.as_str();

    let claimed =
        claim_organization_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    if !claimed {
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_command_row(connection, tenant, request_id)
                .await?
                .ok_or_else(conflict)?;
        if row_fingerprint != fingerprint || row_actor != actor_id {
            return Err(conflict());
        }
        let receipt_value = row_receipt.ok_or_else(conflict)?;
        let stored = OrganizationReceipt::parse(&receipt_value).map_err(|_| conflict())?;
        // Recovery re-authorization: a SCOPED access replays only the
        // receipts whose recorded organization still belongs to it (the TS
        // `if (access.scope === 'assigned' && parsed.organizationId)` branch).
        let organization = parse_uuid_value(&stored.organization_id)?;
        if let Some(row_owner) = organization_owner(connection, tenant, organization).await? {
            if !access.sees(&row_owner) {
                return Err(forbidden());
            }
        }
        return Ok(replayed_receipt(&stored));
    }

    let now = committed_at(connection).await?;
    let (organization_id, affiliation_id, revision, reason) =
        execute_organization_arm(connection, tenant, actor_id, access, mutation).await?;
    let receipt = OrganizationReceipt {
        request_id: mutation.request_id.clone(),
        kind: mutation.data.kind().as_str().to_string(),
        organization_id: organization_id.to_string(),
        affiliation_id: affiliation_id.as_ref().map(Uuid::to_string),
        revision,
        committed_at: now,
        replayed: false,
    };
    insert_organization_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        mutation.data.kind().as_str(),
        organization_id,
        affiliation_id,
        revision,
        reason.as_deref(),
    )
    .await?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|_| conflict())?;
    sqlx::query(
        "UPDATE archava_organizations.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

// --- execute(): the six arms -------------------------------------------------

/// One arm's outcome: `(organization_id, affiliation_id, revision, reason)`
/// — everything the audit row and receipt record.
type ArmOutcome = (Uuid, Option<Uuid>, i64, Option<String>);

/// The FOR UPDATE read every non-create arm takes on the organization.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct OrgRow {
    id: Uuid,
    owner_id: String,
    state: String,
    revision: i64,
}

async fn load_organization(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<OrgRow, PurchaseError> {
    let (owner_id, state, revision_i32): (String, String, i32) = sqlx::query_as(
        "SELECT owner_id,state,revision FROM archava_organizations.organizations \
         WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?
    .ok_or_else(not_found)?;
    Ok(OrgRow {
        id,
        owner_id,
        state,
        revision: i64::from(revision_i32),
    })
}

/// The scoped-access owner rule the non-create arms run before their
/// archived/revision gates, in the TS order.
fn scope_gate(access: &OrgAccess, row: &OrgRow) -> Result<(), PurchaseError> {
    if !access.sees(&row.owner_id) {
        return Err(forbidden());
    }
    Ok(())
}

/// The archived gate — the TS `'archived'` error class, a conflict-class
/// refusal here (see the module header).
fn archived_gate(row: &OrgRow) -> Result<(), PurchaseError> {
    if row.state == "archived" {
        return Err(conflict());
    }
    Ok(())
}

/// The revision gate — number(row.revision) !== d.revision.
fn revision_gate(row: &OrgRow, expected: i64) -> Result<(), PurchaseError> {
    if row.revision != expected {
        return Err(conflict());
    }
    Ok(())
}

/// The one-active-primary pre-check — the TS `primary_conflict` error the
/// database's partial unique index would also eventually raise.
async fn primary_conflict_gate(
    connection: &mut PgConnection,
    tenant: &str,
    organization_id: Uuid,
    is_primary: bool,
    status_is_active: bool,
    exclude: Option<Uuid>,
) -> Result<(), PurchaseError> {
    if !(is_primary && status_is_active) {
        return Ok(());
    }
    let owner = match exclude {
        None => sqlx::query_as::<_, (Uuid,)>(
            "SELECT id FROM archava_organizations.affiliations WHERE tenant_id=$1 \
                 AND organization_id=$2 AND is_primary=true AND status='active'",
        )
        .bind(tenant)
        .bind(organization_id),
        Some(id) => sqlx::query_as::<_, (Uuid,)>(
            "SELECT id FROM archava_organizations.affiliations WHERE tenant_id=$1 \
                 AND organization_id=$2 AND is_primary=true AND status='active' AND id<>$3",
        )
        .bind(tenant)
        .bind(organization_id)
        .bind(id),
    }
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?;
    if owner.is_some() {
        return Err(conflict());
    }
    Ok(())
}

/// The six executed arms, in the TS branch order.
async fn execute_organization_arm(
    connection: &mut PgConnection,
    tenant: &str,
    actor_id: &str,
    access: &OrgAccess,
    mutation: &OrganizationMutation,
) -> Result<ArmOutcome, PurchaseError> {
    match &mutation.data {
        OrganizationData::CreateOrganization {
            name,
            legal_name,
            tax_id,
            website,
            industry,
            billing_address,
        } => {
            let id = Uuid::new_v4();
            // The TS binds `?? null` for every optional cell — absent and
            // null collapse on the INSERT.
            sqlx::query(
                "INSERT INTO archava_organizations.organizations(\
                    tenant_id,id,name,legal_name,tax_id,website,industry,billing_address,state,owner_id,revision,created_at,updated_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,'active',$9,1,\
                    clock_timestamp(),clock_timestamp())",
            )
            .bind(tenant)
            .bind(id)
            .bind(name)
            .bind(legal_name.clone().flatten())
            .bind(tax_id.clone().flatten())
            .bind(website.clone().flatten())
            .bind(industry.clone().flatten())
            .bind(billing_address.clone().flatten())
            .bind(actor_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((id, None, 1, None))
        }
        OrganizationData::UpdateOrganization {
            id,
            revision,
            name,
            legal_name,
            tax_id,
            website,
            industry,
            billing_address,
        } => {
            let id = parse_uuid_value(id)?;
            let row = load_organization(connection, tenant, id).await?;
            scope_gate(access, &row)?;
            archived_gate(&row)?;
            revision_gate(&row, *revision)?;
            let next_revision = revision + 1;
            // `name = COALESCE($3, name)` — the optional name only replaces
            // when present (absent binds NULL twice over, so the COALESCE
            // branch holds the old cell for both absent and null calls).
            sqlx::query(
                "UPDATE archava_organizations.organizations SET \
                    name = COALESCE($3, name), \
                    legal_name = CASE WHEN $4::text IS NOT NULL THEN $4 ELSE legal_name END, \
                    tax_id = CASE WHEN $5::text IS NOT NULL THEN $5 ELSE tax_id END, \
                    website = CASE WHEN $6::text IS NOT NULL THEN $6 ELSE website END, \
                    industry = CASE WHEN $7::text IS NOT NULL THEN $7 ELSE industry END, \
                    billing_address = CASE WHEN $8::text IS NOT NULL THEN $8 ELSE billing_address END, \
                    revision = $9, updated_at = clock_timestamp() \
                  WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(id)
            .bind(name.clone())
            .bind(legal_name.clone().flatten())
            .bind(tax_id.clone().flatten())
            .bind(website.clone().flatten())
            .bind(industry.clone().flatten())
            .bind(billing_address.clone().flatten())
            .bind(int4(next_revision)?)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((id, None, next_revision, None))
        }
        OrganizationData::ArchiveOrganization {
            id,
            revision,
            reason,
        } => {
            let id = parse_uuid_value(id)?;
            let row = load_organization(connection, tenant, id).await?;
            scope_gate(access, &row)?;
            archived_gate(&row)?;
            revision_gate(&row, *revision)?;
            let next_revision = revision + 1;
            sqlx::query(
                "UPDATE archava_organizations.organizations SET \
                    state='archived', revision=$3, updated_at=clock_timestamp() \
                  WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(id)
            .bind(int4(next_revision)?)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            // The still-active affiliations deactivate with the archive.
            sqlx::query(
                "UPDATE archava_organizations.affiliations SET status='inactive', \
                    updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND organization_id=$2 AND status='active'",
            )
            .bind(tenant)
            .bind(id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let reason_text = reason.clone();
            Ok((id, None, next_revision, Some(reason_text)))
        }
        OrganizationData::CreateAffiliation {
            organization_id,
            contact_id,
            role,
            title,
            department,
            is_primary,
        } => {
            let organization_id = parse_uuid_value(organization_id)?;
            let contact_id = parse_uuid_value(contact_id)?;
            let row = load_organization(connection, tenant, organization_id).await?;
            scope_gate(access, &row)?;
            archived_gate(&row)?;
            let is_primary = is_primary.unwrap_or(false);
            primary_conflict_gate(connection, tenant, organization_id, is_primary, true, None)
                .await?;
            let affiliation_id = Uuid::new_v4();
            let next_revision = row.revision + 1;
            sqlx::query(
                "INSERT INTO archava_organizations.affiliations(\
                    tenant_id,id,organization_id,contact_id,role,title,department,is_primary,status,created_at,updated_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,'active',\
                    clock_timestamp(),clock_timestamp())",
            )
            .bind(tenant)
            .bind(affiliation_id)
            .bind(organization_id)
            .bind(contact_id)
            .bind(role)
            .bind(title.clone().flatten())
            .bind(department.clone().flatten())
            .bind(is_primary)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            sqlx::query(
                "UPDATE archava_organizations.organizations SET revision=$3, \
                    updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(organization_id)
            .bind(int4(next_revision)?)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((organization_id, Some(affiliation_id), next_revision, None))
        }
        OrganizationData::UpdateAffiliation {
            id,
            organization_id,
            role,
            title,
            department,
            is_primary,
            status,
        } => {
            let id = parse_uuid_value(id)?;
            let organization_id = parse_uuid_value(organization_id)?;
            let row = load_organization(connection, tenant, organization_id).await?;
            scope_gate(access, &row)?;
            archived_gate(&row)?;
            missing_affiliation(connection, tenant, id, organization_id).await?;
            // The TS: `d.isPrimary && (d.status === undefined || d.status === 'active')`
            // — a primary-inactive update does NOT trip the conflict.
            let status_is_active = matches!(status.as_deref(), None | Some("active"));
            primary_conflict_gate(
                connection,
                tenant,
                organization_id,
                is_primary.as_ref().map(|held| *held).unwrap_or(false),
                status_is_active,
                Some(id),
            )
            .await?;
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_organizations.affiliations SET \
                    role = COALESCE($4, role), \
                    title = CASE WHEN $5::text IS NOT NULL THEN $5 ELSE title END, \
                    department = CASE WHEN $6::text IS NOT NULL THEN $6 ELSE department END, \
                    is_primary = COALESCE($7, is_primary), \
                    status = COALESCE($8, status), \
                    updated_at = clock_timestamp() \
                  WHERE tenant_id=$1 AND id=$2 AND organization_id=$3",
            )
            .bind(tenant)
            .bind(id)
            .bind(organization_id)
            .bind(role.clone())
            .bind(title.clone().flatten())
            .bind(department.clone().flatten())
            .bind(*is_primary)
            // CASE with the TS's $8: COALESCE keeps the present value.
            .bind(status.clone())
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            sqlx::query(
                "UPDATE archava_organizations.organizations SET revision=$3, \
                    updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(organization_id)
            .bind(int4(next_revision)?)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((organization_id, Some(id), next_revision, None))
        }
        OrganizationData::RemoveAffiliation {
            id,
            organization_id,
            reason,
        } => {
            // remove_affiliation: the recorded-org gate runs, the archived
            // gate does NOT (the TS branch has no state check here).
            let id = parse_uuid_value(id)?;
            let organization_id = parse_uuid_value(organization_id)?;
            let row = load_organization(connection, tenant, organization_id).await?;
            scope_gate(access, &row)?;
            missing_affiliation(connection, tenant, id, organization_id).await?;
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_organizations.affiliations SET status='inactive', \
                    updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2 AND organization_id=$3",
            )
            .bind(tenant)
            .bind(id)
            .bind(organization_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            sqlx::query(
                "UPDATE archava_organizations.organizations SET revision=$3, \
                    updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(organization_id)
            .bind(int4(next_revision)?)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((
                organization_id,
                Some(id),
                next_revision,
                Some(reason.clone()),
            ))
        }
    }
}

/// A missing affiliation row — the TS not_found loaded eagerly to match
/// the statement order.
async fn missing_affiliation(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
    organization_id: Uuid,
) -> Result<(), PurchaseError> {
    let row = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM archava_organizations.affiliations WHERE tenant_id=$1 AND id=$2 \
         AND organization_id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .bind(organization_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?;
    if row.is_none() {
        return Err(not_found());
    }
    Ok(())
}

/// `commandStatus()` — the discovered receipt replays only while the
/// CURRENT access still authorizes the recorded organization.
pub async fn organization_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    request_id: Uuid,
) -> Result<OrganizationCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    let Some((_, _, row_receipt)) = fetch_command_row(connection, tenant, request_id).await? else {
        return Ok(OrganizationCommandStatus::NotCommitted);
    };
    let receipt_value = row_receipt.ok_or_else(conflict)?;
    let stored = OrganizationReceipt::parse(&receipt_value).map_err(|_| conflict())?;
    let organization = parse_uuid_value(&stored.organization_id)?;
    if let Some(row_owner) = organization_owner(connection, tenant, organization).await? {
        if !access.sees(&row_owner) {
            return Err(forbidden());
        }
    }
    Ok(OrganizationCommandStatus::Committed { receipt: stored })
}
