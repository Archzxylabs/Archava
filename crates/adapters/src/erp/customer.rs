//! Current CRM contact visibility, shared by contact-linked business commands.
use super::PurchaseError;
use crate::erp::crm::{self, CrmActor, CrmPersistenceError, LockedContact};
use archava_core::tenant::TenantId;
use archava_domain::erp::{purchasing::PurchasePrincipal, ErpError};
use sqlx::PgConnection;
use uuid::Uuid;

fn error(error: CrmPersistenceError) -> PurchaseError {
    match error {
        CrmPersistenceError::Database { ledger, source } => {
            PurchaseError::Database { ledger, source }
        }
        CrmPersistenceError::UnknownResource => ErpError::NotFound.into(),
        CrmPersistenceError::TenantContext(crate::rls::TenantContextError::Database {
            ledger,
            source,
        }) => PurchaseError::Database { ledger, source },
        CrmPersistenceError::TenantContext(error) => PurchaseError::Database {
            ledger: "crm",
            source: sqlx::Error::Protocol(error.to_string()),
        },
        _ => ErpError::Forbidden.into(),
    }
}

/// Locks the current contact and membership, but leaves revision comparison to
/// the new-command branch so an authorized replay survives a later name change.
pub(super) async fn resolve(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    contact: Uuid,
) -> Result<LockedContact, PurchaseError> {
    let tenant = TenantId::new(tenant).map_err(|_| PurchaseError::from(ErpError::Conflict))?;
    let actor = if principal.kind.as_str() == "human" {
        CrmActor::Human(principal.id.clone())
    } else {
        CrmActor::Service {
            id: principal.id.clone(),
            on_behalf_of: principal.on_behalf_of.clone().ok_or(ErpError::Forbidden)?,
        }
    };
    let access = crm::authorize(connection, &tenant, &actor)
        .await
        .map_err(error)?;
    if !access.allows("contacts.read") {
        return Err(ErpError::Forbidden.into());
    }
    // SHARE blocks profile edits while remaining compatible with foreign-key
    // KEY SHARE locks from purchase orders. An exclusive contact lock would
    // invert the supplier/contact lock order during a profile refresh.
    type ContactSnapshot = (i32, String, Option<String>, Option<String>, String);
    let row = sqlx::query_as::<_, ContactSnapshot>(
        "SELECT revision,full_name,email,organization,owner_id FROM archava_crm.contacts \
         WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR SHARE",
    )
    .bind(tenant.as_str())
    .bind(contact)
    .bind(access.scope == crm::CrmScope::All)
    .bind(&access.owner_id)
    .fetch_optional(connection)
    .await
    .map_err(|source| PurchaseError::Database {
        ledger: "crm",
        source,
    })?
    .ok_or(ErpError::NotFound)?;
    Ok(LockedContact {
        revision: row.0,
        full_name: row.1,
        email: row.2,
        organization: row.3,
        owner_id: row.4,
    })
}
