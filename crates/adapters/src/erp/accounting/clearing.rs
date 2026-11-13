//! Reviewed settlement of an already posted provider event into actual bank cash.
use super::*;
use archava_domain::erp::accounting::{clearing as rules, Line};

pub(super) async fn journal_authority(
    conn: &mut PgConnection,
    tenant: &str,
    a: &OrgAccess,
    journal: Uuid,
    posting: bool,
) -> Result<(), PurchaseError> {
    let linked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_accounting.clearing_settlements WHERE tenant_id=$1 AND id=$2)").bind(tenant).bind(journal).fetch_one(conn).await.map_err(db)?;
    if linked {
        require(a, "clearing.read")?;
        require(a, "bank.read")?;
        if !posting {
            require(a, "clearing.write")?;
        }
    }
    Ok(())
}
pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    let Command::ClearingSettlementDraft {
        source_id,
        bank_row_id,
        code,
        clearing_account,
        cash_account,
        fee_account,
        gross_minor,
        fee_minor,
        reason,
    } = command
    else {
        return Err(ErpError::Conflict.into());
    };
    let source = id(source_id)?;
    let row = id(bank_row_id)?;
    let effect:Option<String>=sqlx::query_scalar("SELECT effect FROM archava_accounting.invoice_sources WHERE tenant_id=$1 AND book_id=$2 AND id=$3 AND effect IN ('collection','refund') AND amount_minor<>0")
        .bind(tenant).bind(b.id).bind(source).fetch_optional(&mut *conn).await.map_err(db)?;
    let effect = match effect.as_deref() {
        Some("collection") => rules::Effect::Collection,
        Some("refund") => rules::Effect::Refund,
        _ => return Err(ErpError::Conflict.into()),
    };
    let date:Option<String>=sqlx::query_scalar("SELECT entry_date::text FROM archava_accounting.bank_rows WHERE tenant_id=$1 AND book_id=$2 AND id=$3")
        .bind(tenant).bind(b.id).bind(row).fetch_optional(&mut *conn).await.map_err(db)?;
    let legs = rules::legs(effect, gross_minor, fee_minor).map_err(|_| ErpError::Conflict)?;
    let cash = if effect == rules::Effect::Collection {
        &legs[1].debit_minor
    } else {
        &legs[1].credit_minor
    };
    let cash_minor = accounting::minor(cash).map_err(|_| ErpError::Conflict)?;
    let mut lines = Vec::new();
    for leg in legs {
        let account = match leg.role {
            rules::Role::Clearing => clearing_account,
            rules::Role::Cash => cash_account,
            rules::Role::Fee => fee_account.as_ref().ok_or(ErpError::Conflict)?,
        };
        lines.push(Line {
            account_id: account.account_id.clone(),
            expected_account_revision: account.expected_account_revision,
            debit_minor: leg.debit_minor,
            credit_minor: leg.credit_minor,
            memo: "Reviewed verified provider event to actual bank".into(),
        });
    }
    let draft = Command::JournalDraft {
        book_id: b.id.to_string(),
        code: code.clone(),
        date: date.ok_or(ErpError::NotFound)?,
        source_ref: format!("clearing:{source}:{code}"),
        memo: format!("Actual bank settlement of verified invoice event {source}"),
        lines,
        reason: reason.clone(),
    };
    let (_, journal, revision) = journal::execute(conn, tenant, p, b, &draft).await?;
    sqlx::query("INSERT INTO archava_accounting.clearing_settlements(tenant_id,id,book_id,source_id,bank_row_id,clearing_account_id,cash_account_id,fee_account_id,effect,gross_minor,fee_minor,cash_minor,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(tenant).bind(journal).bind(b.id).bind(source).bind(row).bind(id(&clearing_account.account_id)?).bind(id(&cash_account.account_id)?)
        .bind(fee_account.as_ref().map(|a|id(&a.account_id)).transpose()?).bind(if effect==rules::Effect::Collection{"collection"}else{"refund"})
        .bind(accounting::minor(gross_minor).map_err(|_|ErpError::Conflict)?).bind(accounting::minor(fee_minor).map_err(|_|ErpError::Conflict)?).bind(cash_minor).bind(&p.id).execute(conn).await.map_err(db)?;
    Ok(("journal", journal, revision))
}

pub(super) fn source_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'code',s.invoice_code||' · '||s.effect,'invoiceId',s.invoice_id,'invoiceCode',s.invoice_code,'effect',s.effect,'sourceDate',s.source_date,'sourceAmountMinor',s.amount_minor::text,'clearingAccountId',(SELECT m->>'accountId' FROM jsonb_array_elements(s.policy_snapshot->'mappings') m WHERE m->>'role'='payment_clearing'),'balance',archava_accounting.clearing_balance(s.tenant_id,s.id),'revision',1)"
}
pub(super) fn settlement_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'sourceId',s.source_id,'bankRowId',s.bank_row_id,'clearingAccountId',s.clearing_account_id,'cashAccountId',s.cash_account_id,'feeAccountId',s.fee_account_id,'effect',s.effect,'grossMinor',s.gross_minor::text,'feeMinor',s.fee_minor::text,'cashMinor',s.cash_minor::text,'code',(SELECT code FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'state',(SELECT state FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'date',(SELECT entry_date FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'revision',(SELECT revision FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id))"
}
