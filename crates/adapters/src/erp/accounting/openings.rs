//! Declared opening balances: a draft journal on a policy-named control
//! account against equity, recorded so the control comparisons can explain it.
//! The database guard decides which account is the control account.
use super::*;
use archava_domain::erp::accounting::{openings as rules, Line};

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    let Command::OpeningBalanceDraft {
        control,
        code,
        date,
        control_account,
        offset_account,
        amount_minor,
        reason,
        ..
    } = command
    else {
        return Err(ErpError::Conflict.into());
    };
    let legs = rules::legs(*control, amount_minor).map_err(|_| ErpError::Conflict)?;
    let lines = legs
        .into_iter()
        .zip([control_account, offset_account])
        .map(|(leg, account)| Line {
            account_id: account.account_id.clone(),
            expected_account_revision: account.expected_account_revision,
            debit_minor: leg.debit_minor,
            credit_minor: leg.credit_minor,
            memo: "Declared opening balance".into(),
        })
        .collect();
    let control = control.as_str();
    let draft = Command::JournalDraft {
        book_id: b.id.to_string(),
        code: code.clone(),
        date: date.clone(),
        source_ref: format!("opening:{control}:{code}"),
        memo: format!("Declared opening {control} balance"),
        lines,
        reason: reason.clone(),
    };
    let (_, journal, revision) = journal::execute(conn, tenant, p, b, &draft).await?;
    sqlx::query("INSERT INTO archava_accounting.opening_balances(tenant_id,id,book_id,control,control_account_id,offset_account_id,amount_minor,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(tenant).bind(journal).bind(b.id).bind(control)
        .bind(id(&control_account.account_id)?).bind(id(&offset_account.account_id)?)
        .bind(accounting::minor(amount_minor).map_err(|_| ErpError::Conflict)?).bind(&p.id)
        .execute(conn).await.map_err(db)?;
    Ok(("journal", journal, revision))
}
