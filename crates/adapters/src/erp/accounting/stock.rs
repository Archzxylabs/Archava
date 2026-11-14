//! Original stock costs enter ordinary journals through their immutable source.
use super::*;
use archava_domain::erp::accounting::{
    stock_sources::{self, CostPortion, StockEffect},
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
        Command::StockPolicySet {
            expected_revision,
            currency_catalogue,
            source_unit,
            cogs_account,
            ..
        } => {
            if *expected_revision == 0 && !b.company.active {
                return Err(ErpError::Conflict.into());
            }
            let (key, revision) = if *expected_revision == 0 {
                let key = Uuid::new_v4();
                sqlx::query("INSERT INTO archava_accounting.stock_policies(tenant_id,id,book_id,currency_catalogue,source_unit,cogs_account_id,account_revision) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(key).bind(b.id).bind(currency_catalogue).bind(source_unit).bind(id(&cogs_account.account_id)?).bind(cogs_account.expected_account_revision).execute(&mut *conn).await.map_err(db)?;
                (key, 1)
            } else {
                // The unit is immutable: a command naming another unit is refused,
                // not accepted with the stored unit kept.
                sqlx::query_as::<_,(Uuid,i32)>("UPDATE archava_accounting.stock_policies SET cogs_account_id=$4,account_revision=$5,revision=revision+1 WHERE tenant_id=$1 AND book_id=$2 AND revision=$3 AND source_unit=$6 RETURNING id,revision").bind(tenant).bind(b.id).bind(expected_revision).bind(id(&cogs_account.account_id)?).bind(cogs_account.expected_account_revision).bind(source_unit).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::Conflict)?
            };
            Ok(("stock_policy", key, revision))
        }
        Command::StockSourceDraft {
            source_id,
            code,
            accounts,
            reason,
        } => {
            let source = id(source_id)?;
            let row=sqlx::query("SELECT effect,segments,source_date::text FROM archava_accounting.stock_sources WHERE tenant_id=$1 AND book_id=$2 AND id=$3").bind(tenant).bind(b.id).bind(source).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
            let effect: String = row.try_get("effect").map_err(db)?;
            let typed_effect: StockEffect =
                serde_json::from_value(json!(&effect)).map_err(|_| ErpError::Conflict)?;
            let segments: Value = row.try_get("segments").map_err(db)?;
            let portions:Vec<CostPortion>=segments.as_array().ok_or(ErpError::Conflict)?.iter().map(|v|serde_json::from_value(json!({"inventoryAccountId":v["inventoryAccountId"],"cogsAccountId":v["cogsAccountId"],"costMinor":v["costMinor"]})).map_err(|_|ErpError::Conflict)).collect::<Result<_,_>>()?;
            let legs =
                stock_sources::legs(typed_effect, &portions).map_err(|_| ErpError::Conflict)?;
            if legs.len() < 2 || legs.len() != accounts.len() {
                return Err(ErpError::Conflict.into());
            }
            // Independently derived SQL guards and the portable engine must agree.
            let stored=sqlx::query("SELECT account_id,debit_minor,credit_minor FROM archava_accounting.stock_legs($1,$2) ORDER BY account_id").bind(tenant).bind(source).fetch_all(&mut *conn).await.map_err(db)?;
            if stored.len() != legs.len() {
                return Err(ErpError::Conflict.into());
            }
            for (leg, stored) in legs.iter().zip(stored) {
                if leg.account_id
                    != stored
                        .try_get::<Uuid, _>("account_id")
                        .map_err(db)?
                        .to_string()
                    || leg.debit_minor
                        != stored
                            .try_get::<i64, _>("debit_minor")
                            .map_err(db)?
                            .to_string()
                    || leg.credit_minor
                        != stored
                            .try_get::<i64, _>("credit_minor")
                            .map_err(db)?
                            .to_string()
                {
                    return Err(ErpError::Conflict.into());
                }
            }
            let lines = legs
                .into_iter()
                .map(|leg| {
                    let reviewed = accounts
                        .iter()
                        .find(|a| a.account_id == leg.account_id)
                        .ok_or(ErpError::Conflict)?;
                    Ok(Line {
                        account_id: leg.account_id,
                        expected_account_revision: reviewed.expected_account_revision,
                        debit_minor: leg.debit_minor,
                        credit_minor: leg.credit_minor,
                        memo: format!("Stock {effect} original cost"),
                    })
                })
                .collect::<Result<Vec<_>, ErpError>>()?;
            let draft = Command::JournalDraft {
                book_id: b.id.to_string(),
                code: code.clone(),
                date: row.try_get("source_date").map_err(db)?,
                source_ref: format!("stock-cost:{source}:{code}"),
                memo: format!("Stock source {source} · {effect}"),
                lines,
                reason: reason.clone(),
            };
            let (_, journal, revision) = journal::execute(conn, tenant, p, b, &draft).await?;
            sqlx::query("INSERT INTO archava_accounting.stock_journal_sources(tenant_id,book_id,journal_id,source_id) VALUES($1,$2,$3,$4)").bind(tenant).bind(b.id).bind(journal).bind(source).execute(&mut *conn).await.map_err(db)?;
            Ok(("journal", journal, revision))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
