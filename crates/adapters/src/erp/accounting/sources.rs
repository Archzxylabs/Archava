use super::*;
use archava_domain::erp::accounting::{sources::InvoiceMapping, Line};

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    match command {
        Command::InvoicePolicySet {
            expected_revision,
            currency_catalogue,
            source_unit,
            mappings,
            ..
        } => {
            if *expected_revision == 0 && !b.company.active {
                return Err(ErpError::Conflict.into());
            }
            let key = if *expected_revision == 0 {
                let key = Uuid::new_v4();
                sqlx::query("INSERT INTO archava_accounting.invoice_policies(tenant_id,id,book_id,source_unit,currency_catalogue,mappings,revision) VALUES($1,$2,$3,$4,$5,$6,1)")
                    .bind(tenant).bind(key).bind(b.id).bind(source_unit).bind(currency_catalogue).bind(json!(mappings)).execute(&mut *conn).await.map_err(db)?;
                (key, 1)
            } else {
                sqlx::query_as::<_, (Uuid,i32)>("UPDATE archava_accounting.invoice_policies SET mappings=$4,revision=revision+1 WHERE tenant_id=$1 AND book_id=$2 AND revision=$3 RETURNING id,revision")
                    .bind(tenant).bind(b.id).bind(expected_revision).bind(json!(mappings)).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::Conflict)?
            };
            Ok(("invoice_policy", key.0, key.1))
        }
        Command::InvoiceSourceDraft {
            source_id,
            code,
            accounts,
            reason,
        } => {
            let source = id(source_id)?;
            let row = sqlx::query("SELECT amount_minor,effect,source_date::text,invoice_code,policy_snapshot FROM archava_accounting.invoice_sources WHERE tenant_id=$1 AND book_id=$2 AND id=$3")
                .bind(tenant).bind(b.id).bind(source).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
            let amount: i64 = row.try_get("amount_minor").map_err(db)?;
            if amount == 0 {
                return Err(ErpError::Conflict.into());
            }
            let effect: String = row.try_get("effect").map_err(db)?;
            let policy: Value = row.try_get("policy_snapshot").map_err(db)?;
            let mappings: Vec<InvoiceMapping> = serde_json::from_value(policy["mappings"].clone())
                .map_err(|_| ErpError::Conflict)?;
            let account_for = |role: &str| {
                mappings
                    .iter()
                    .find(|m| m.role.as_str() == role)
                    .map(|m| m.account_id.as_str())
                    .ok_or(ErpError::Conflict)
            };
            let legs = archava_domain::erp::accounting::sources::legs(&effect, amount)
                .map_err(|_| ErpError::Conflict)?;
            let make_line = |leg: archava_domain::erp::accounting::sources::InvoiceLeg| -> Result<Line, PurchaseError> {
                let key=account_for(leg.role.as_str())?;
                let reviewed=accounts.iter().find(|a|a.account_id==key).ok_or(ErpError::Conflict)?;
                Ok(Line{account_id:key.into(),expected_account_revision:reviewed.expected_account_revision,debit_minor:leg.debit_minor,credit_minor:leg.credit_minor,memo:format!("Source invoice {effect}")})
            };
            let draft = Command::JournalDraft {
                book_id: b.id.to_string(),
                code: code.clone(),
                date: row.try_get("source_date").map_err(db)?,
                source_ref: format!("billing:{source}:{code}"),
                memo: format!(
                    "{} · {effect}",
                    row.try_get::<String, _>("invoice_code").map_err(db)?
                ),
                lines: legs
                    .into_iter()
                    .map(make_line)
                    .collect::<Result<Vec<_>, _>>()?,
                reason: reason.clone(),
            };
            let (_, journal, revision) = journal::execute(conn, tenant, p, b, &draft).await?;
            sqlx::query("INSERT INTO archava_accounting.journal_sources(tenant_id,book_id,journal_id,source_id) VALUES($1,$2,$3,$4)")
                .bind(tenant).bind(b.id).bind(journal).bind(source).execute(&mut *conn).await.map_err(db)?;
            Ok(("journal", journal, revision))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
