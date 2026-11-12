use super::*;
use archava_domain::erp::accounting::Line;

async fn draft(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<Uuid, PurchaseError> {
    let (code, date, source_ref, memo, lines, reversal) = match command {
        Command::JournalDraft {
            code,
            date,
            source_ref,
            memo,
            lines,
            ..
        } => (
            code.clone(),
            date.clone(),
            source_ref.clone(),
            memo.clone(),
            lines.clone(),
            None,
        ),
        Command::JournalReverse {
            id: key,
            expected_revision,
            code,
            date,
            source_ref,
            ..
        } => {
            let original = id(key)?;
            let record=sqlx::query("SELECT code,state,revision,reversal_of FROM archava_accounting.journals WHERE tenant_id=$1 AND id=$2 AND book_id=$3 FOR UPDATE").bind(tenant).bind(original).bind(b.id).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
            if record.try_get::<String, _>("state").map_err(db)? != "posted"
                || record.try_get::<i32, _>("revision").map_err(db)? != *expected_revision
                || record
                    .try_get::<Option<Uuid>, _>("reversal_of")
                    .map_err(db)?
                    .is_some()
            {
                return Err(ErpError::Conflict.into());
            }
            let rows=sqlx::query("SELECT l.account_id,a.revision,l.debit_minor,l.credit_minor,l.memo FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE l.tenant_id=$1 AND l.journal_id=$2 ORDER BY l.ordinal").bind(tenant).bind(original).fetch_all(&mut *conn).await.map_err(db)?;
            let mut lines = Vec::new();
            for r in rows {
                lines.push(Line {
                    account_id: r.try_get::<Uuid, _>("account_id").map_err(db)?.to_string(),
                    expected_account_revision: r.try_get("revision").map_err(db)?,
                    debit_minor: r.try_get::<i64, _>("credit_minor").map_err(db)?.to_string(),
                    credit_minor: r.try_get::<i64, _>("debit_minor").map_err(db)?.to_string(),
                    memo: r.try_get("memo").map_err(db)?,
                });
            }
            (
                code.clone(),
                date.clone(),
                source_ref.clone(),
                format!(
                    "Reversal of {}",
                    record.try_get::<String, _>("code").map_err(db)?
                ),
                lines,
                Some(original),
            )
        }
        _ => return Err(ErpError::Conflict.into()),
    };
    if !accounting::balanced(&lines) {
        return Err(ErpError::Conflict.into());
    }
    let period:Option<Uuid>=sqlx::query_scalar("SELECT id FROM archava_accounting.periods WHERE tenant_id=$1 AND book_id=$2 AND state='open' AND start_date<=$3::text::date AND end_date>=$3::text::date FOR UPDATE").bind(tenant).bind(b.id).bind(&date).fetch_optional(&mut *conn).await.map_err(db)?;
    let key = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_accounting.journals(tenant_id,id,book_id,period_id,code,entry_date,source_ref,memo,created_by,reversal_of) VALUES($1,$2,$3,$4,$5,$6::text::date,$7,$8,$9,$10)")
        .bind(tenant).bind(key).bind(b.id).bind(period.ok_or(ErpError::Conflict)?).bind(code).bind(date).bind(source_ref).bind(memo).bind(&p.id).bind(reversal).execute(&mut *conn).await.map_err(db)?;
    for (index, line) in lines.iter().enumerate() {
        let affected=sqlx::query("INSERT INTO archava_accounting.journal_lines(tenant_id,journal_id,book_id,ordinal,account_id,account_revision,account_code,account_name,class,cash_role,debit_minor,credit_minor,memo) SELECT $1,$2,$3,$4,a.id,a.revision,a.code,a.name,a.class,a.cash_role,$7,$8,$9 FROM archava_accounting.accounts a WHERE tenant_id=$1 AND book_id=$3 AND id=$5 AND revision=$6 AND active")
            .bind(tenant).bind(key).bind(b.id).bind(i32::try_from(index+1).map_err(|_|ErpError::Conflict)?).bind(id(&line.account_id)?).bind(line.expected_account_revision).bind(accounting::minor(&line.debit_minor).map_err(|_|ErpError::Conflict)?).bind(accounting::minor(&line.credit_minor).map_err(|_|ErpError::Conflict)?).bind(&line.memo).execute(&mut *conn).await.map_err(db)?.rows_affected();
        if affected != 1 {
            return Err(ErpError::Conflict.into());
        }
    }
    Ok(key)
}
pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    match command {
        Command::JournalDraft { .. } | Command::JournalReverse { .. } => {
            Ok(("journal", draft(conn, tenant, p, b, command).await?, 1))
        }
        Command::JournalPost {
            id: key,
            expected_revision,
            ..
        }
        | Command::JournalVoid {
            id: key,
            expected_revision,
            ..
        } => {
            let key = id(key)?;
            let posting = matches!(command, Command::JournalPost { .. });
            if posting && b.independent {
                let creator:String=sqlx::query_scalar("SELECT created_by FROM archava_accounting.journals WHERE tenant_id=$1 AND id=$2 AND book_id=$3").bind(tenant).bind(key).bind(b.id).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
                if creator == p.id {
                    return Err(ErpError::Forbidden.into());
                }
            }
            let state = if posting { "posted" } else { "void" };
            let revision:Option<i32>=sqlx::query_scalar("UPDATE archava_accounting.journals SET state=$4,revision=revision+1,posted_by=CASE WHEN $4='posted' THEN $5 ELSE NULL END,posted_at=CASE WHEN $4='posted' THEN clock_timestamp() ELSE NULL END WHERE tenant_id=$1 AND id=$2 AND revision=$3 AND state='draft' RETURNING revision")
                .bind(tenant).bind(key).bind(expected_revision).bind(state).bind(&p.id).fetch_optional(conn).await.map_err(db)?;
            Ok(("journal", key, revision.ok_or(ErpError::Conflict)?))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
