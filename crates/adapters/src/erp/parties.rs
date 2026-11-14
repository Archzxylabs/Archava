//! Business-party registry, using the Organizations command namespace atomically.
use super::{organizations::resolve_organizations_context, purchase_database_error, PurchaseError};
use archava_domain::erp::{
    organizations::OrgAccess,
    parties::{PartyCommand, PartyMutation, PartyReceipt},
    purchasing::PurchasePrincipal,
    ErpError,
};
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;
fn db(error: sqlx::Error) -> PurchaseError {
    purchase_database_error("organizations", error)
}
fn uuid(id: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(id).map_err(|_| ErpError::Conflict.into())
}
fn grant(access: &OrgAccess) -> Result<(), PurchaseError> {
    if !access.holds(&["parties.read", "parties.write"]) {
        return Err(ErpError::Forbidden.into());
    }
    Ok(())
}
#[derive(Debug)]
struct Source {
    name: String,
    revision: i32,
    active: bool,
    /// The organization's tax id as entered; a contact has none.
    tax_id: Option<String>,
}
struct PartyRow {
    id: Uuid,
    owner_id: String,
    source_contact_id: Option<Uuid>,
    source_organization_id: Option<Uuid>,
    source_revision: i32,
    revision: i32,
    customer: bool,
    supplier: bool,
    tax_id: Option<String>,
}
/// Whether parties keep a tax id (Organizations 005). The tax id is written
/// only where the column exists, and read through jsonb, so an install whose
/// Organizations ledger predates it (an owned upgrade fixture) registers and
/// refreshes parties as it always did.
async fn keeps_tax_id(conn: &mut PgConnection) -> Result<bool, PurchaseError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='archava_organizations.parties'::regclass AND attname='tax_id' AND NOT attisdropped)")
        .fetch_one(conn)
        .await
        .map_err(db)
}
async fn party(
    conn: &mut PgConnection,
    tenant: &str,
    access: &OrgAccess,
    id: Uuid,
    lock: bool,
) -> Result<PartyRow, PurchaseError> {
    let sql = if lock {
        "SELECT id,owner_id,source_contact_id,source_organization_id,source_revision,revision,customer,supplier,to_jsonb(p)->>'tax_id' FROM archava_organizations.parties p WHERE tenant_id=$1 AND id=$2 FOR UPDATE"
    } else {
        "SELECT id,owner_id,source_contact_id,source_organization_id,source_revision,revision,customer,supplier,to_jsonb(p)->>'tax_id' FROM archava_organizations.parties p WHERE tenant_id=$1 AND id=$2"
    };
    let (
        id,
        owner_id,
        source_contact_id,
        source_organization_id,
        source_revision,
        revision,
        customer,
        supplier,
        tax_id,
    ) = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<Uuid>,
            Option<Uuid>,
            i32,
            i32,
            bool,
            bool,
            Option<String>,
        ),
    >(sql)
    .bind(tenant)
    .bind(id)
    .fetch_optional(conn)
    .await
    .map_err(db)?
    .ok_or(ErpError::NotFound)?;
    let row = PartyRow {
        id,
        owner_id,
        source_contact_id,
        source_organization_id,
        source_revision,
        revision,
        customer,
        supplier,
        tax_id,
    };
    if !access.sees(&row.owner_id) {
        return Err(ErpError::NotFound.into());
    }
    Ok(row)
}
async fn source(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    contact: Option<Uuid>,
    organization: Option<Uuid>,
) -> Result<Source, PurchaseError> {
    if let Some(id) = contact {
        let row = super::customer::resolve(conn, tenant, principal, id).await?;
        return Ok(Source {
            name: row.full_name,
            revision: row.revision,
            active: true,
            tax_id: None,
        });
    }
    if !access.holds(&["organizations.read"]) {
        return Err(ErpError::Forbidden.into());
    }
    let id = organization.ok_or(ErpError::Conflict)?;
    let row:Option<(String,i32,String,String,Option<String>)>=sqlx::query_as("SELECT name,revision,state,owner_id,tax_id FROM archava_organizations.organizations WHERE tenant_id=$1 AND id=$2 FOR SHARE").bind(tenant).bind(id).fetch_optional(conn).await.map_err(db)?;
    let (name, revision, state, owner, tax_id) = row.ok_or(ErpError::NotFound)?;
    if !access.sees(&owner) {
        return Err(ErpError::NotFound.into());
    }
    Ok(Source {
        name,
        revision,
        active: state == "active",
        tax_id,
    })
}
/// The host uses this scoped lookup solely to apply the CRM module gate.
pub async fn uses_contact(
    conn: &mut PgConnection,
    tenant: &str,
    access: &OrgAccess,
    command: &PartyCommand,
) -> Result<bool, PurchaseError> {
    grant(access)?;
    Ok(match command {
        PartyCommand::FromContact { .. } => true,
        PartyCommand::RefreshSource { id, .. } => party(conn, tenant, access, uuid(id)?, false)
            .await?
            .source_contact_id
            .is_some(),
        _ => false,
    })
}
async fn stored(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    request: Uuid,
) -> Result<Option<PartyReceipt>, PurchaseError> {
    let row:Option<(String,Option<Value>)>=sqlx::query_as("SELECT actor_id,receipt FROM archava_organizations.commands WHERE tenant_id=$1 AND request_id=$2").bind(tenant).bind(request).fetch_optional(&mut *conn).await.map_err(db)?;
    let Some((actor, receipt)) = row else {
        return Ok(None);
    };
    let Some(value) = receipt else {
        return Ok(None);
    };
    if !value["type"]
        .as_str()
        .is_some_and(|s| s.starts_with("party."))
    {
        return Ok(None);
    }
    if actor != principal.id || principal.kind.as_str() != "human" {
        return Err(ErpError::Forbidden.into());
    }
    grant(access)?;
    let mut receipt: PartyReceipt =
        serde_json::from_value(value).map_err(|_| ErpError::Conflict)?;
    if ![
        "party.create_from_contact",
        "party.create_from_organization",
        "party.set_roles",
        "party.refresh_source",
    ]
    .contains(&receipt.operation.as_str())
        || receipt.revision < 1
        || uuid(&receipt.request_id)? != request
    {
        return Err(ErpError::Conflict.into());
    }
    party(conn, tenant, access, uuid(&receipt.party_id)?, false).await?;
    receipt.replayed = true;
    Ok(Some(receipt))
}
/// A party receipt shares the existing request-ID namespace, never the org ID field.
pub async fn command_status(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    request: Uuid,
) -> Result<Option<Value>, PurchaseError> {
    if let Some(receipt) = stored(conn, tenant, principal, access, request).await? {
        return Ok(Some(
            json!({"decision":"committed","requestId":receipt.request_id,"receipt":receipt}),
        ));
    }
    if principal.kind.as_str() == "human" && access.holds(&["parties.read", "parties.write"]) {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_organizations.commands WHERE tenant_id=$1 AND request_id=$2)").bind(tenant).bind(request).fetch_one(conn).await.map_err(db)?;
        if !exists {
            return Ok(Some(
                json!({"decision":"not_committed","requestId":request,"receipt":Value::Null}),
            ));
        }
    }
    Ok(None)
}

pub async fn record(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    fingerprint: &str,
    mutation: &PartyMutation,
) -> Result<PartyReceipt, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(ErpError::Forbidden.into());
    }
    grant(access)?;
    // Read immutable source identity without a row lock. Source locks precede the
    // eventual party update, avoiding a pair of SHARE-to-UPDATE lock upgrades.
    let current = if let Some(id) = mutation.command.id() {
        Some(party(conn, tenant, access, uuid(id)?, false).await?)
    } else {
        None
    };
    let reviewed = match &mutation.command {
        PartyCommand::FromContact { contact_id, .. } => Some(
            source(
                conn,
                tenant,
                principal,
                access,
                Some(uuid(contact_id)?),
                None,
            )
            .await?,
        ),
        PartyCommand::FromOrganization {
            organization_id, ..
        } => Some(
            source(
                conn,
                tenant,
                principal,
                access,
                None,
                Some(uuid(organization_id)?),
            )
            .await?,
        ),
        PartyCommand::RefreshSource { .. } => {
            let row = current.as_ref().ok_or(ErpError::Conflict)?;
            Some(
                source(
                    conn,
                    tenant,
                    principal,
                    access,
                    row.source_contact_id,
                    row.source_organization_id,
                )
                .await?,
            )
        }
        _ => None,
    };
    let request = uuid(&mutation.request_id)?;
    let claimed=sqlx::query("INSERT INTO archava_organizations.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(tenant).bind(request).bind(&principal.id).bind(fingerprint).execute(&mut *conn).await.map_err(db)?.rows_affected()==1;
    if !claimed {
        let held:Option<(String,String)>=sqlx::query_as("SELECT actor_id,fingerprint FROM archava_organizations.commands WHERE tenant_id=$1 AND request_id=$2").bind(tenant).bind(request).fetch_optional(&mut *conn).await.map_err(db)?;
        if held != Some((principal.id.clone(), fingerprint.into())) {
            return Err(ErpError::Conflict.into());
        }
        return stored(conn, tenant, principal, access, request)
            .await?
            .ok_or_else(|| ErpError::Conflict.into());
    }
    if let Some(source) = &reviewed {
        if !source.active
            || Some(source.revision) != mutation.command.expected_source()
            || source.name.trim().is_empty()
            || source.name.chars().count() > 160
        {
            return Err(ErpError::Conflict.into());
        }
    }
    let (id, revision) = match &mutation.command {
        PartyCommand::FromContact {
            customer, supplier, ..
        }
        | PartyCommand::FromOrganization {
            customer, supplier, ..
        } => {
            let source = reviewed.as_ref().ok_or(ErpError::Conflict)?;
            let id = Uuid::new_v4();
            let (contact, organization) = match &mutation.command {
                PartyCommand::FromContact { contact_id, .. } => (Some(uuid(contact_id)?), None),
                PartyCommand::FromOrganization {
                    organization_id, ..
                } => (None, Some(uuid(organization_id)?)),
                _ => unreachable!(),
            };
            let insert = if keeps_tax_id(conn).await? {
                sqlx::query("INSERT INTO archava_organizations.parties(tenant_id,id,owner_id,source_contact_id,source_organization_id,source_revision,name,customer,supplier,tax_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(tenant).bind(id).bind(&principal.id).bind(contact).bind(organization).bind(source.revision).bind(&source.name).bind(customer).bind(supplier).bind(&source.tax_id)
            } else {
                sqlx::query("INSERT INTO archava_organizations.parties(tenant_id,id,owner_id,source_contact_id,source_organization_id,source_revision,name,customer,supplier) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(tenant).bind(id).bind(&principal.id).bind(contact).bind(organization).bind(source.revision).bind(&source.name).bind(customer).bind(supplier)
            };
            insert.execute(&mut *conn).await.map_err(db)?;
            (id, 1)
        }
        PartyCommand::SetRoles {
            id,
            expected_revision,
            customer,
            supplier,
            ..
        } => {
            let row = party(conn, tenant, access, uuid(id)?, true).await?;
            if row.revision != *expected_revision
                || row.customer == *customer && row.supplier == *supplier
            {
                return Err(ErpError::Conflict.into());
            }
            sqlx::query("UPDATE archava_organizations.parties SET customer=$3,supplier=$4,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(row.id).bind(customer).bind(supplier).execute(&mut *conn).await.map_err(db)?;
            (row.id, row.revision + 1)
        }
        PartyCommand::RefreshSource {
            id,
            expected_revision,
            ..
        } => {
            let row = party(conn, tenant, access, uuid(id)?, true).await?;
            let source = reviewed.as_ref().ok_or(ErpError::Conflict)?;
            // A refresh needs a newer source. The one exception is a party
            // registered before parties kept a tax id: it may take the tax id
            // its organization already had at the reviewed revision.
            let keeps = keeps_tax_id(conn).await?;
            let adopts_tax_id = keeps
                && source.revision == row.source_revision
                && row.tax_id.is_none()
                && source.tax_id.is_some();
            if row.revision != *expected_revision
                || (source.revision <= row.source_revision && !adopts_tax_id)
            {
                return Err(ErpError::Conflict.into());
            }
            let update = if keeps {
                sqlx::query("UPDATE archava_organizations.parties SET name=$3,source_revision=$4,tax_id=$5,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(row.id).bind(&source.name).bind(source.revision).bind(&source.tax_id)
            } else {
                sqlx::query("UPDATE archava_organizations.parties SET name=$3,source_revision=$4,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(row.id).bind(&source.name).bind(source.revision)
            };
            update.execute(&mut *conn).await.map_err(db)?;
            (row.id, row.revision + 1)
        }
    };
    let receipt = PartyReceipt {
        request_id: mutation.request_id.clone(),
        operation: mutation.command.operation().into(),
        party_id: id.to_string(),
        revision,
        replayed: false,
    };
    sqlx::query("INSERT INTO archava_organizations.party_audit(tenant_id,request_id,party_id,actor_id,operation,revision,reason,snapshot) SELECT tenant_id,$3,id,$4,$5,revision,$6,to_jsonb(p) FROM archava_organizations.parties p WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(id).bind(request).bind(&principal.id).bind(mutation.command.operation()).bind(mutation.command.reason()).execute(&mut *conn).await.map_err(db)?;
    sqlx::query(
        "UPDATE archava_organizations.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request)
    .bind(serde_json::to_value(&receipt).map_err(|_| ErpError::Conflict)?)
    .execute(conn)
    .await
    .map_err(db)?;
    Ok(receipt)
}
/// Reuse the current membership resolver for a source-independent business master.
pub async fn access(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<OrgAccess, PurchaseError> {
    resolve_organizations_context(conn, tenant, principal).await
}

/// The published business master used by new transactions. Raw CRM/source grants
/// are deliberately independent of the explicit parties.read permission.
pub(super) struct TransactionParty {
    pub id: Uuid,
    pub name: String,
    pub revision: i32,
    pub customer: bool,
    pub supplier: bool,
}
pub(super) async fn resolve_transaction(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    id: Uuid,
) -> Result<TransactionParty, PurchaseError> {
    let access = resolve_organizations_context(conn, tenant, principal).await?;
    if !access.holds(&["parties.read"]) {
        return Err(ErpError::Forbidden.into());
    }
    let row:Option<(Uuid,String,i32,bool,bool)>=sqlx::query_as("SELECT id,name,revision,customer,supplier FROM archava_organizations.parties WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR SHARE")
        .bind(tenant).bind(id).bind(access.scope.covers_all()).bind(&access.owner_id).fetch_optional(conn).await.map_err(db)?;
    let (id, name, revision, customer, supplier) = row.ok_or(ErpError::NotFound)?;
    Ok(TransactionParty {
        id,
        name,
        revision,
        customer,
        supplier,
    })
}
