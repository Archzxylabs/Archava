//! Published company context for trade documents; no accounting engine is selected here.
use super::{companies, PurchaseError};
use archava_domain::erp::{purchasing::PurchasePrincipal, ErpError};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyStamp {
    pub id: Uuid,
    pub revision: i32,
    pub code: String,
    pub name: String,
    pub active: bool,
}
impl CompanyStamp {
    pub fn validate_new(&self, expected: i64) -> Result<(), PurchaseError> {
        if self.active && i64::from(self.revision) == expected {
            Ok(())
        } else {
            Err(ErpError::Conflict.into())
        }
    }
}
fn db(e: sqlx::Error) -> PurchaseError {
    super::purchase_database_error("organizations", e)
}
pub async fn resolve(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    id: Uuid,
) -> Result<CompanyStamp, PurchaseError> {
    resolve_locked(conn, tenant, principal, id, false).await
}
/// `resolve` for opening a book in a currency other than IDR. It takes the
/// company row lock that conflicts with the `FOR SHARE` every company trade
/// insert holds, at the same point in the lock order as `resolve`, so the book
/// guard (Accounting 015) never has to upgrade a shared lock inside its trigger.
pub async fn resolve_excluding_trade(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    id: Uuid,
) -> Result<CompanyStamp, PurchaseError> {
    resolve_locked(conn, tenant, principal, id, true).await
}
async fn resolve_locked(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    id: Uuid,
    excluding_trade: bool,
) -> Result<CompanyStamp, PurchaseError> {
    let access = companies::access(conn, tenant, principal).await?;
    if !access.holds(&["companies.read"]) {
        return Err(ErpError::Forbidden.into());
    }
    let statement = if excluding_trade {
        "SELECT id,revision,code,legal_name,active FROM archava_organizations.companies WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR NO KEY UPDATE"
    } else {
        "SELECT id,revision,code,legal_name,active FROM archava_organizations.companies WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR SHARE"
    };
    let row = sqlx::query(statement)
        .bind(tenant)
        .bind(id)
        .bind(access.scope.covers_all())
        .bind(&access.owner_id)
        .fetch_optional(conn)
        .await
        .map_err(db)?
        .ok_or(ErpError::NotFound)?;
    Ok(CompanyStamp {
        id: row.try_get("id").map_err(db)?,
        revision: row.try_get("revision").map_err(db)?,
        code: row.try_get("code").map_err(db)?,
        name: row.try_get("legal_name").map_err(db)?,
        active: row.try_get("active").map_err(db)?,
    })
}
#[derive(Clone, Copy)]
pub enum Record {
    Product,
    Sale,
    Purchase,
    Invoice,
    Offer,
    SupplierDocument,
}
/// Internal privacy classification of already bounded organization records.
/// These relations never grant the actor access to the corresponding company.
pub async fn company_sources(
    conn: &mut PgConnection,
    tenant: &str,
    organizations: &[Uuid],
) -> Result<Vec<(Uuid, Uuid)>, PurchaseError> {
    if organizations.len() > 1000 {
        return Err(ErpError::Conflict.into());
    }
    sqlx::query_as("SELECT source_organization_id,id FROM archava_organizations.companies WHERE tenant_id=$1 AND source_organization_id=ANY($2)").bind(tenant).bind(organizations).fetch_all(conn).await.map_err(db)
}
impl Record {
    fn query(self) -> &'static str {
        match self{
    Self::SupplierDocument=>"SELECT company_id FROM archava_purchasing.supplier_documents WHERE tenant_id=$1 AND id=$2",
    Self::Product=>"SELECT (to_jsonb(r)->>'company_id')::uuid FROM archava_commerce.products r WHERE tenant_id=$1 AND id=$2",
    Self::Sale=>"SELECT (to_jsonb(r)->>'company_id')::uuid FROM archava_commerce.orders r WHERE tenant_id=$1 AND id=$2",
    Self::Purchase=>"SELECT (to_jsonb(r)->>'company_id')::uuid FROM archava_purchasing.orders r WHERE tenant_id=$1 AND id=$2",
    Self::Invoice=>"SELECT (to_jsonb(r)->>'company_id')::uuid FROM archava_billing.invoices r WHERE tenant_id=$1 AND id=$2",
    Self::Offer=>"SELECT (to_jsonb(p)->>'company_id')::uuid FROM archava_purchasing.offers r JOIN archava_commerce.products p ON p.tenant_id=r.tenant_id AND p.id=r.product_id WHERE r.tenant_id=$1 AND r.id=$2",
}
    }
}
pub async fn record_company(
    conn: &mut PgConnection,
    tenant: &str,
    kind: Record,
    id: Uuid,
) -> Result<Option<Uuid>, PurchaseError> {
    sqlx::query_scalar(kind.query())
        .bind(tenant)
        .bind(id)
        .fetch_optional(conn)
        .await
        .map_err(db)?
        .ok_or_else(|| ErpError::NotFound.into())
}
pub async fn authorize(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    kind: Record,
    id: Uuid,
) -> Result<Option<Uuid>, PurchaseError> {
    let company = record_company(conn, tenant, kind, id).await?;
    if let Some(id) = company {
        resolve(conn, tenant, principal, id).await?;
    }
    Ok(company)
}
/// Old schema fixtures legitimately have no company column; other decode failures refuse.
pub(super) fn row_company(row: &sqlx::postgres::PgRow) -> Result<Option<Uuid>, PurchaseError> {
    match row.try_get("company_id") {
        Ok(id) => Ok(id),
        Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
        Err(e) => Err(db(e)),
    }
}
/// Billing read helpers receive an already verified human access frame.
pub async fn authorize_human(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    kind: Record,
    id: Uuid,
) -> Result<Option<Uuid>, PurchaseError> {
    authorize(conn, tenant, &PurchasePrincipal::human(actor), kind, id).await
}
