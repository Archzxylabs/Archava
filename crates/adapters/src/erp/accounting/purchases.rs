//! Native purchase sources share the book fence and ordinary review/post policy.
use super::*;
use archava_domain::erp::accounting::{
    purchase_sources::{self, PurchaseEffect, PurchaseMapping},
    Line,
};

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    match command {
        Command::PurchasePolicySet {
            expected_revision,
            currency_catalogue,
            source_unit,
            mappings,
            ..
        } => {
            if *expected_revision == 0 && !b.company.active {
                return Err(ErpError::Conflict.into());
            }
            let (key, revision) = if *expected_revision == 0 {
                let key = Uuid::new_v4();
                sqlx::query("INSERT INTO archava_accounting.purchase_policies(tenant_id,id,book_id,source_unit,currency_catalogue,mappings,revision) VALUES($1,$2,$3,$4,$5,$6,1)")
                    .bind(tenant).bind(key).bind(b.id).bind(source_unit).bind(currency_catalogue).bind(json!(mappings)).execute(&mut *conn).await.map_err(db)?;
                (key, 1)
            } else {
                sqlx::query_as::<_,(Uuid,i32)>("UPDATE archava_accounting.purchase_policies SET mappings=$4,revision=revision+1 WHERE tenant_id=$1 AND book_id=$2 AND revision=$3 RETURNING id,revision")
                    .bind(tenant).bind(b.id).bind(expected_revision).bind(json!(mappings)).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::Conflict)?
            };
            Ok(("purchase_policy", key, revision))
        }
        Command::PurchaseSourceDraft {
            source_id,
            code,
            accounts,
            reason,
        } => {
            let source = id(source_id)?;
            let row=sqlx::query("SELECT s.goods_minor,s.landed_fee_minor,s.effect,s.source_date::text,o.policy_snapshot FROM archava_accounting.purchase_sources s JOIN archava_accounting.purchase_origins o ON o.tenant_id=s.tenant_id AND o.id=s.origin_id AND o.book_id=s.book_id WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.id=$3")
                .bind(tenant).bind(b.id).bind(source).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
            let effect: PurchaseEffect =
                serde_json::from_value(json!(row.try_get::<String, _>("effect").map_err(db)?))
                    .map_err(|_| ErpError::Conflict)?;
            let legs = purchase_sources::legs(
                effect,
                row.try_get("goods_minor").map_err(db)?,
                row.try_get("landed_fee_minor").map_err(db)?,
            )
            .map_err(|_| ErpError::Conflict)?;
            if legs.is_empty() || legs.len() != accounts.len() {
                return Err(ErpError::Conflict.into());
            }
            let policy: Value = row.try_get("policy_snapshot").map_err(db)?;
            let mappings: Vec<PurchaseMapping> = serde_json::from_value(policy["mappings"].clone())
                .map_err(|_| ErpError::Conflict)?;
            let mut lines = Vec::new();
            for leg in legs {
                let mapping = mappings
                    .iter()
                    .find(|m| m.role == leg.role)
                    .ok_or(ErpError::Conflict)?;
                let reviewed = accounts
                    .iter()
                    .find(|a| a.account_id == mapping.account_id)
                    .ok_or(ErpError::Conflict)?;
                lines.push(Line {
                    account_id: mapping.account_id.clone(),
                    expected_account_revision: reviewed.expected_account_revision,
                    debit_minor: leg.debit_minor,
                    credit_minor: leg.credit_minor,
                    memo: format!("Purchase {} {}", effect.as_str(), leg.role.as_str()),
                });
            }
            let draft = Command::JournalDraft {
                book_id: b.id.to_string(),
                code: code.clone(),
                date: row.try_get("source_date").map_err(db)?,
                source_ref: format!("purchasing:{source}:{code}"),
                memo: format!("Purchase source {source} · {}", effect.as_str()),
                lines,
                reason: reason.clone(),
            };
            let (_, journal, revision) = journal::execute(conn, tenant, p, b, &draft).await?;
            sqlx::query("INSERT INTO archava_accounting.purchase_journal_sources(tenant_id,book_id,journal_id,source_id) VALUES($1,$2,$3,$4)").bind(tenant).bind(b.id).bind(journal).bind(source).execute(&mut *conn).await.map_err(db)?;
            Ok(("journal", journal, revision))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
