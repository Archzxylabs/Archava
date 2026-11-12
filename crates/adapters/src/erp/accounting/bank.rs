//! Manual evidence is immutable and independent of journal/payment execution.
use super::*;
use archava_domain::erp::accounting::{bank as rules, sources::AccountRef, AccountClass, CashRole};

async fn cash_account(
    conn: &mut PgConnection,
    tenant: &str,
    b: &Book,
    reference: &AccountRef,
) -> Result<(Uuid, String), PurchaseError> {
    let key = id(&reference.account_id)?;
    let row=sqlx::query("SELECT a.class,a.cash_role,a.active,a.revision,b.currency FROM archava_accounting.accounts a JOIN archava_accounting.books b ON b.tenant_id=a.tenant_id AND b.id=a.book_id WHERE a.tenant_id=$1 AND a.book_id=$2 AND a.id=$3 FOR SHARE OF a")
        .bind(tenant).bind(b.id).bind(key).fetch_optional(conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
    let class: AccountClass =
        serde_json::from_value(json!(row.try_get::<String, _>("class").map_err(db)?))
            .map_err(|_| ErpError::Conflict)?;
    let role: CashRole =
        serde_json::from_value(json!(row.try_get::<String, _>("cash_role").map_err(db)?))
            .map_err(|_| ErpError::Conflict)?;
    let currency: String = row.try_get("currency").map_err(db)?;
    if row.try_get::<i32, _>("revision").map_err(db)? != reference.expected_account_revision
        || !rules::valid_cash_binding(
            reference,
            SourceAuthority::Native,
            class,
            role,
            row.try_get("active").map_err(db)?,
            &currency,
        )
    {
        return Err(ErpError::Conflict.into());
    }
    Ok((key, currency))
}

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    b: &Book,
    command: &Command,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    match command {
        Command::BankAccountCreate {
            code,
            name,
            account,
            ..
        } => {
            let (account_id, _) = cash_account(conn, tenant, b, account).await?;
            let key = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_accounting.bank_accounts(tenant_id,id,book_id,account_id,account_revision,code,name) VALUES($1,$2,$3,$4,$5,$6,$7)")
                .bind(tenant).bind(key).bind(b.id).bind(account_id).bind(account.expected_account_revision).bind(code).bind(name).execute(conn).await.map_err(db)?;
            Ok(("bank_account", key, 1))
        }
        Command::BankStatementImport {
            bank_account_id,
            account,
            statement,
            ..
        } => {
            let bank = id(bank_account_id)?;
            let (account_id, currency) = cash_account(conn, tenant, b, account).await?;
            let bound: Option<Uuid> = sqlx::query_scalar("SELECT account_id FROM archava_accounting.bank_accounts WHERE tenant_id=$1 AND book_id=$2 AND id=$3")
                .bind(tenant).bind(b.id).bind(bank).fetch_optional(&mut *conn).await.map_err(db)?;
            if bound != Some(account_id) {
                return Err(ErpError::Conflict.into());
            }
            let totals =
                rules::statement_totals(statement, &currency).map_err(|_| ErpError::Conflict)?;
            let key = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_accounting.bank_statements(tenant_id,id,book_id,bank_account_id,account_revision,reference,from_date,to_date,currency,opening_minor,closing_minor,row_count,created_by) VALUES($1,$2,$3,$4,$5,$6,$7::text::date,$8::text::date,$9,$10,$11,$12,$13)")
                .bind(tenant).bind(key).bind(b.id).bind(bank).bind(account.expected_account_revision).bind(&statement.reference).bind(&statement.from_date).bind(&statement.to_date).bind(&currency)
                .bind(rules::signed_minor(&statement.opening_minor).map_err(|_| ErpError::Conflict)?).bind(rules::signed_minor(&statement.closing_minor).map_err(|_| ErpError::Conflict)?)
                .bind(i32::try_from(totals.row_count).map_err(|_| ErpError::Conflict)?).bind(&p.id).execute(&mut *conn).await.map_err(db)?;
            for (index, row) in statement.rows.iter().enumerate() {
                sqlx::query("INSERT INTO archava_accounting.bank_rows(tenant_id,id,book_id,bank_account_id,statement_id,ordinal,external_ref,entry_date,amount_minor,description) VALUES($1,$2,$3,$4,$5,$6,$7,$8::text::date,$9,$10)")
                    .bind(tenant).bind(Uuid::new_v4()).bind(b.id).bind(bank).bind(key).bind(i32::try_from(index+1).map_err(|_|ErpError::Conflict)?).bind(&row.external_ref).bind(&row.date)
                    .bind(rules::signed_minor(&row.amount_minor).map_err(|_|ErpError::Conflict)?).bind(&row.description).execute(&mut *conn).await.map_err(db)?;
            }
            Ok(("bank_statement", key, 1))
        }
        Command::BankAllocationRecord {
            bank_row_id,
            journal_id,
            journal_line_ordinal,
            expected_journal_revision,
            account,
            amount_minor,
            ..
        } => {
            let row = id(bank_row_id)?;
            let journal = id(journal_id)?;
            let (account_id, _) = cash_account(conn, tenant, b, account).await?;
            let bound: Option<Uuid> = sqlx::query_scalar("SELECT a.account_id FROM archava_accounting.bank_rows r JOIN archava_accounting.bank_accounts a ON a.tenant_id=r.tenant_id AND a.id=r.bank_account_id WHERE r.tenant_id=$1 AND r.book_id=$2 AND r.id=$3")
                .bind(tenant).bind(b.id).bind(row).fetch_optional(&mut *conn).await.map_err(db)?;
            if bound != Some(account_id) {
                return Err(ErpError::Conflict.into());
            }
            let cash: Option<String> = sqlx::query_scalar("SELECT archava_accounting.bank_cash_leg_remaining(j.tenant_id,j.id,l.ordinal)::text FROM archava_accounting.journals j JOIN archava_accounting.journal_lines l ON l.tenant_id=j.tenant_id AND l.journal_id=j.id WHERE j.tenant_id=$1 AND j.book_id=$2 AND j.id=$3 AND l.ordinal=$4 AND l.account_id=$5 AND j.state='posted' AND j.revision=$6")
                .bind(tenant).bind(b.id).bind(journal).bind(journal_line_ordinal).bind(account_id).bind(expected_journal_revision).fetch_optional(&mut *conn).await.map_err(db)?;
            let remaining: String =
                sqlx::query_scalar("SELECT archava_accounting.bank_row_remaining($1,$2)::text")
                    .bind(tenant)
                    .bind(row)
                    .fetch_one(&mut *conn)
                    .await
                    .map_err(db)?;
            rules::allocate(&remaining, &cash.ok_or(ErpError::Conflict)?, amount_minor)
                .map_err(|_| ErpError::Conflict)?;
            let key = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_accounting.bank_allocations(tenant_id,id,book_id,bank_row_id,journal_id,journal_line_ordinal,journal_revision,account_revision,amount_minor,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
                .bind(tenant).bind(key).bind(b.id).bind(row).bind(journal).bind(journal_line_ordinal).bind(expected_journal_revision).bind(account.expected_account_revision)
                .bind(accounting::minor(amount_minor).map_err(|_|ErpError::Conflict)?).bind(&p.id).execute(conn).await.map_err(db)?;
            Ok(("bank_allocation", key, 1))
        }
        Command::BankAllocationReverse {
            id: original,
            expected_revision,
            ..
        } => {
            let original = id(original)?;
            let key = Uuid::new_v4();
            let inserted=sqlx::query("INSERT INTO archava_accounting.bank_unmatches(tenant_id,id,book_id,allocation_id,created_by) SELECT $1,$2,$3,a.id,$5 FROM archava_accounting.bank_allocations a WHERE a.tenant_id=$1 AND a.book_id=$3 AND a.id=$4 AND a.revision=$6")
                .bind(tenant).bind(key).bind(b.id).bind(original).bind(&p.id).bind(expected_revision).execute(conn).await.map_err(db)?.rows_affected();
            if inserted != 1 {
                return Err(ErpError::Conflict.into());
            }
            Ok(("bank_unmatch", key, 1))
        }
        _ => Err(ErpError::Conflict.into()),
    }
}
