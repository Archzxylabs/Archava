//! Reviewed managed-company records share Organizations authority and command keys.
use super::{organizations::resolve_organizations_context, purchase_database_error, PurchaseError};
use archava_domain::erp::{
    companies::{CompanyCommand, CompanyMutation, CompanyReceipt},
    organizations::OrgAccess,
    purchasing::PurchasePrincipal,
    ErpError,
};
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
fn db(e: sqlx::Error) -> PurchaseError {
    purchase_database_error("organizations", e)
}
fn uuid(id: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(id).map_err(|_| ErpError::Conflict.into())
}
fn grant(access: &OrgAccess) -> Result<(), PurchaseError> {
    if access.holds(&["companies.read", "companies.write"]) {
        Ok(())
    } else {
        Err(ErpError::Forbidden.into())
    }
}
struct Company {
    id: Uuid,
    source: Uuid,
    source_revision: i32,
    revision: i32,
    active: bool,
}
async fn company(
    conn: &mut PgConnection,
    tenant: &str,
    access: &OrgAccess,
    id: Uuid,
    lock: bool,
) -> Result<Company, PurchaseError> {
    let query = if lock {
        "SELECT id,source_organization_id,source_revision,revision,active FROM archava_organizations.companies WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR UPDATE"
    } else {
        "SELECT id,source_organization_id,source_revision,revision,active FROM archava_organizations.companies WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4)"
    };
    let row = sqlx::query(query)
        .bind(tenant)
        .bind(id)
        .bind(access.scope.covers_all())
        .bind(&access.owner_id)
        .fetch_optional(conn)
        .await
        .map_err(db)?
        .ok_or(ErpError::NotFound)?;
    Ok(Company {
        id: row.try_get("id").map_err(db)?,
        source: row.try_get("source_organization_id").map_err(db)?,
        source_revision: row.try_get("source_revision").map_err(db)?,
        revision: row.try_get("revision").map_err(db)?,
        active: row.try_get("active").map_err(db)?,
    })
}
struct Source {
    name: String,
    legal_name: Option<String>,
    tax_id: Option<String>,
    revision: i32,
    active: bool,
}
async fn source(
    conn: &mut PgConnection,
    tenant: &str,
    access: &OrgAccess,
    id: Uuid,
) -> Result<Source, PurchaseError> {
    if !access.holds(&["organizations.read"]) {
        return Err(ErpError::Forbidden.into());
    }
    let row=sqlx::query("SELECT name,legal_name,tax_id,revision,state FROM archava_organizations.organizations WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR SHARE").bind(tenant).bind(id).bind(access.scope.covers_all()).bind(&access.owner_id).fetch_optional(conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
    Ok(Source {
        name: row.try_get("name").map_err(db)?,
        legal_name: row.try_get("legal_name").map_err(db)?,
        tax_id: row.try_get("tax_id").map_err(db)?,
        revision: row.try_get("revision").map_err(db)?,
        active: row.try_get::<String, _>("state").map_err(db)? == "active",
    })
}
async fn stored(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    request: Uuid,
) -> Result<Option<CompanyReceipt>, PurchaseError> {
    let held:Option<(String,Option<Value>)>=sqlx::query_as("SELECT actor_id,receipt FROM archava_organizations.commands WHERE tenant_id=$1 AND request_id=$2").bind(tenant).bind(request).fetch_optional(&mut *conn).await.map_err(db)?;
    let Some((actor, Some(value))) = held else {
        return Ok(None);
    };
    if !value["type"]
        .as_str()
        .is_some_and(|t| t.starts_with("company."))
    {
        return Ok(None);
    }
    if actor != principal.id || principal.kind.as_str() != "human" {
        return Err(ErpError::Forbidden.into());
    }
    grant(access)?;
    let mut receipt: CompanyReceipt =
        serde_json::from_value(value).map_err(|_| ErpError::Conflict)?;
    if ![
        "company.create_from_organization",
        "company.refresh_source",
        "company.set_active",
    ]
    .contains(&receipt.operation.as_str())
        || receipt.revision < 1
        || uuid(&receipt.request_id)? != request
    {
        return Err(ErpError::Conflict.into());
    }
    company(conn, tenant, access, uuid(&receipt.company_id)?, false).await?;
    receipt.replayed = true;
    Ok(Some(receipt))
}
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
    if principal.kind.as_str() == "human" && access.holds(&["companies.read", "companies.write"]) {
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
    mutation: &CompanyMutation,
) -> Result<CompanyReceipt, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(ErpError::Forbidden.into());
    }
    grant(access)?;
    let current = if let Some(id) = mutation.command.id() {
        Some(company(conn, tenant, access, uuid(id)?, false).await?)
    } else {
        None
    };
    // Immutable source lookup is unlocked; always acquire source before company.
    let reviewed = match &mutation.command {
        CompanyCommand::FromOrganization {
            organization_id, ..
        } => Some(source(conn, tenant, access, uuid(organization_id)?).await?),
        CompanyCommand::RefreshSource { .. } | CompanyCommand::SetActive { active: true, .. } => {
            Some(
                source(
                    conn,
                    tenant,
                    access,
                    current.as_ref().ok_or(ErpError::Conflict)?.source,
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
            || source
                .legal_name
                .as_ref()
                .is_none_or(|name| name.trim().is_empty())
            || mutation
                .command
                .expected_source()
                .is_some_and(|rev| rev != source.revision)
        {
            return Err(ErpError::Conflict.into());
        }
    }
    let (id, revision) = match &mutation.command {
        CompanyCommand::FromOrganization {
            code,
            organization_id,
            ..
        } => {
            let s = reviewed.as_ref().ok_or(ErpError::Conflict)?;
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_organizations.companies(tenant_id,id,owner_id,code,source_organization_id,source_revision,name,legal_name,tax_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(tenant).bind(id).bind(&principal.id).bind(code).bind(uuid(organization_id)?).bind(s.revision).bind(&s.name).bind(&s.legal_name).bind(&s.tax_id).execute(&mut *conn).await.map_err(db)?;
            (id, 1)
        }
        CompanyCommand::RefreshSource {
            id,
            expected_revision,
            ..
        } => {
            let c = company(conn, tenant, access, uuid(id)?, true).await?;
            let s = reviewed.as_ref().ok_or(ErpError::Conflict)?;
            if c.revision != *expected_revision || s.revision <= c.source_revision {
                return Err(ErpError::Conflict.into());
            }
            sqlx::query("UPDATE archava_organizations.companies SET name=$3,legal_name=$4,tax_id=$5,source_revision=$6,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(c.id).bind(&s.name).bind(&s.legal_name).bind(&s.tax_id).bind(s.revision).execute(&mut *conn).await.map_err(db)?;
            (c.id, c.revision + 1)
        }
        CompanyCommand::SetActive {
            id,
            expected_revision,
            active,
            ..
        } => {
            let c = company(conn, tenant, access, uuid(id)?, true).await?;
            if c.revision != *expected_revision
                || c.active == *active
                || (*active
                    && reviewed
                        .as_ref()
                        .is_none_or(|s| s.revision != c.source_revision))
            {
                return Err(ErpError::Conflict.into());
            }
            sqlx::query("UPDATE archava_organizations.companies SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(c.id).bind(active).execute(&mut *conn).await.map_err(db)?;
            (c.id, c.revision + 1)
        }
    };
    let receipt = CompanyReceipt {
        request_id: mutation.request_id.clone(),
        operation: mutation.command.operation().into(),
        company_id: id.to_string(),
        revision,
        replayed: false,
    };
    sqlx::query("INSERT INTO archava_organizations.company_audit(tenant_id,request_id,company_id,actor_id,operation,revision,reason,snapshot) SELECT tenant_id,$3,id,$4,$5,revision,$6,to_jsonb(c) FROM archava_organizations.companies c WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(id).bind(request).bind(&principal.id).bind(mutation.command.operation()).bind(mutation.command.reason()).execute(&mut *conn).await.map_err(db)?;
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
pub async fn access(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<OrgAccess, PurchaseError> {
    resolve_organizations_context(conn, tenant, principal).await
}
