use super::*;
use archava_domain::erp::accounting::{
    sources::{InvoiceLegAccount, InvoiceMapping},
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
                // The unit is immutable: a command naming another unit is refused,
                // not accepted with the stored unit kept.
                sqlx::query_as::<_, (Uuid,i32)>("UPDATE archava_accounting.invoice_policies SET mappings=$4,revision=revision+1 WHERE tenant_id=$1 AND book_id=$2 AND revision=$3 AND source_unit=$5 RETURNING id,revision")
                    .bind(tenant).bind(b.id).bind(expected_revision).bind(json!(mappings)).bind(source_unit).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::Conflict)?
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
            let row = sqlx::query("SELECT amount_minor,tax_minor,tax_account_id,effect,source_date::text,invoice_code,policy_snapshot FROM archava_accounting.invoice_sources WHERE tenant_id=$1 AND book_id=$2 AND id=$3")
                .bind(tenant).bind(b.id).bind(source).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
            let amount: i64 = row.try_get("amount_minor").map_err(db)?;
            if amount == 0 {
                return Err(ErpError::Conflict.into());
            }
            let tax: i64 = row.try_get("tax_minor").map_err(db)?;
            let tax_account: Option<String> = row
                .try_get::<Option<Uuid>, _>("tax_account_id")
                .map_err(db)?
                .map(|key| key.to_string());
            let effect: String = row.try_get("effect").map_err(db)?;
            let policy: Value = row.try_get("policy_snapshot").map_err(db)?;
            let mappings: Vec<InvoiceMapping> = serde_json::from_value(policy["mappings"].clone())
                .map_err(|_| ErpError::Conflict)?;
            // A source that carries tax is refused with its own reason when the
            // reviewed accounts are not exactly its receivable, revenue and tax
            // control accounts; the journal-source guard holds the same rule.
            let unreviewed = || -> PurchaseError {
                if tax == 0 {
                    ErpError::Conflict.into()
                } else {
                    crate::erp::refused_by_code("invoice_source_tax_accounts_required")
                }
            };
            let account_for = |account: InvoiceLegAccount| match account {
                InvoiceLegAccount::Policy(role) => mappings
                    .iter()
                    .find(|m| m.role == role)
                    .map(|m| m.account_id.as_str())
                    .ok_or_else(|| PurchaseError::from(ErpError::Conflict)),
                InvoiceLegAccount::TaxControl => tax_account
                    .as_deref()
                    .ok_or_else(|| PurchaseError::from(ErpError::Conflict)),
            };
            let legs = archava_domain::erp::accounting::sources::taxed_legs(&effect, amount, tax)
                .map_err(|_| ErpError::Conflict)?;
            // Exactly the accounts of the legs: two without tax, three with it.
            if accounts.len() != legs.len() {
                return Err(unreviewed());
            }
            let make_line = |leg: archava_domain::erp::accounting::sources::TaxedInvoiceLeg| -> Result<Line, PurchaseError> {
                let key=account_for(leg.account)?;
                let reviewed=accounts.iter().find(|a|a.account_id==key).ok_or_else(unreviewed)?;
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
