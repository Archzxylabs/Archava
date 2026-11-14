//! A company's own tax codes. `tax_code.create` stores a version as entered,
//! `tax_code.confirm` is the review after which it applies, and
//! `tax_code.retire` withdraws an entered version or closes the range of a
//! confirmed one. A company sales order names a confirmed output code and
//! the invoice made from it stores the version that applies (Commerce 012,
//! Billing 009), and its captured source posts the tax. The database guard (Accounting 016)
//! decides the control account, the reviewer and the date range.
use super::*;
use crate::erp::refused_by_code;

/// The workspace sentence for every place a tax code is shown (TAX.md rule 1).
pub(super) const NOTICE: &str = "Tax codes are entered by your company; Archava does not verify tax rates or rules. An entered version does not apply until a reviewer confirms it. A stored version never changes: retire it and enter a new version instead.";

/// One row of the `tax_codes` workspace collection. `$7` is the date the
/// caller asked about, or the UTC calendar date now when it named none.
pub(super) fn projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'code',s.code,'version',s.version,'kind',s.kind,\
     'rate',jsonb_build_object('numerator',s.rate_numerator,'denominator',s.rate_denominator),\
     'base',jsonb_build_object('numerator',s.base_numerator,'denominator',s.base_denominator),\
     'controlAccount',(SELECT jsonb_build_object('accountId',a.id,'expectedAccountRevision',s.control_account_revision,'code',a.code,'name',a.name,'class',a.class,'active',a.active,'revision',a.revision) FROM archava_accounting.accounts a WHERE a.tenant_id=s.tenant_id AND a.id=s.control_account_id),\
     'effectiveFrom',s.effective_from,'effectiveTo',s.effective_to,'state',s.state,'revision',s.revision,\
     'createdBy',s.created_by,'reviewedBy',s.reviewed_by,'createdAt',s.created_at,'confirmedAt',s.confirmed_at,\
     'withdrawnAt',s.withdrawn_at,'retiredAt',s.retired_at,\
     'appliesOn',coalesce($7::text::date,(now() AT TIME ZONE 'UTC')::date),\
     'applies',s.state IN ('confirmed','retired') AND coalesce($7::text::date,(now() AT TIME ZONE 'UTC')::date) BETWEEN s.effective_from AND coalesce(s.effective_to,DATE 'infinity'),\
     'enteredByCompany',true,'verifiedByArchava',false)"
}

/// The state a tax code version is in now, for the receipt of its command.
pub(super) async fn state(
    conn: &mut PgConnection,
    tenant: &str,
    key: Uuid,
) -> Result<String, PurchaseError> {
    sqlx::query_scalar(
        "SELECT state FROM archava_accounting.tax_codes WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(key)
    .fetch_one(conn)
    .await
    .map_err(db)
}
async fn held(
    conn: &mut PgConnection,
    tenant: &str,
    b: &Book,
    key: Uuid,
) -> Result<(i32, String, String), PurchaseError> {
    sqlx::query_as("SELECT revision,state,created_by FROM archava_accounting.tax_codes WHERE tenant_id=$1 AND id=$2 AND book_id=$3 FOR UPDATE")
        .bind(tenant).bind(key).bind(b.id).fetch_optional(conn).await.map_err(db)?
        .ok_or_else(|| ErpError::NotFound.into())
}

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    match command {
        Command::TaxCodeCreate {
            code,
            kind,
            rate,
            base,
            control_account,
            effective_from,
            ..
        } => {
            // The caller holds the book row FOR UPDATE, so the next version is
            // stable until this transaction ends; the guard checks it again.
            let version: i32 = sqlx::query_scalar("SELECT coalesce(max(version),0)+1 FROM archava_accounting.tax_codes WHERE tenant_id=$1 AND book_id=$2 AND code=$3")
                .bind(tenant).bind(b.id).bind(code).fetch_one(&mut *conn).await.map_err(db)?;
            let key = Uuid::new_v4();
            // Stored as entered on every book: it applies only once confirmed.
            sqlx::query("INSERT INTO archava_accounting.tax_codes(tenant_id,id,book_id,code,version,kind,rate_numerator,rate_denominator,base_numerator,base_denominator,control_account_id,control_account_revision,effective_from,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::text::date,$14)")
                .bind(tenant).bind(key).bind(b.id).bind(code).bind(version).bind(kind.as_str())
                .bind(rate.numerator).bind(rate.denominator).bind(base.numerator).bind(base.denominator)
                .bind(id(&control_account.account_id)?).bind(control_account.expected_account_revision)
                .bind(effective_from).bind(&p.id)
                .execute(conn).await.map_err(db)?;
            Ok(("tax_code", key, 1))
        }
        Command::TaxCodeConfirm {
            id: key,
            expected_revision,
            ..
        } => {
            let key = id(key)?;
            let (revision, state, creator) = held(conn, tenant, b, key).await?;
            if state != "entered" {
                return Err(refused_by_code("tax_code_not_awaiting_confirmation"));
            }
            if revision != *expected_revision {
                return Err(ErpError::Conflict.into());
            }
            // The journal-posting rule, named before the guard would refuse it.
            if b.independent && creator == p.id {
                return Err(refused_by_code("tax_code_independent_reviewer_required"));
            }
            // The guard checks the control account again as it is now.
            let revision: i32 = sqlx::query_scalar("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by=$3,confirmed_at=clock_timestamp(),revision=revision+1 WHERE tenant_id=$1 AND id=$2 RETURNING revision")
                .bind(tenant).bind(key).bind(&p.id).fetch_one(conn).await.map_err(db)?;
            Ok(("tax_code", key, revision))
        }
        Command::TaxCodeRetire {
            id: key,
            expected_revision,
            effective_to,
            ..
        } => {
            let key = id(key)?;
            let (revision, state, _) = held(conn, tenant, b, key).await?;
            if state == "withdrawn" || state == "retired" {
                return Err(refused_by_code("tax_code_already_closed"));
            }
            if revision != *expected_revision {
                return Err(ErpError::Conflict.into());
            }
            let revision: i32 = if state == "entered" {
                // It never applied, so it has no last day to store.
                sqlx::query_scalar("UPDATE archava_accounting.tax_codes SET state='withdrawn',withdrawn_at=clock_timestamp(),revision=revision+1 WHERE tenant_id=$1 AND id=$2 RETURNING revision")
                    .bind(tenant).bind(key).fetch_one(conn).await.map_err(db)?
            } else {
                let Some(last_day) = effective_to else {
                    return Err(refused_by_code("tax_code_last_day_required"));
                };
                sqlx::query_scalar("UPDATE archava_accounting.tax_codes SET state='retired',effective_to=$3::text::date,retired_at=clock_timestamp(),revision=revision+1 WHERE tenant_id=$1 AND id=$2 RETURNING revision")
                    .bind(tenant).bind(key).bind(last_day).fetch_one(conn).await.map_err(db)?
            };
            Ok(("tax_code", key, revision))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
