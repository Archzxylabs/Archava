//! Native financial books. Every public operation runs inside the caller's tenant transaction.
mod bank;
mod bank_periods;
mod clearing;
mod close;
mod inventory;
mod journal;
mod openings;
mod page;
mod payables;
mod purchases;
mod reads;
mod sources;
mod stock;
mod subledger;
mod tax;
mod terms;
use super::{
    commerce_kernel::load_locked_membership, company_trade, purchase_database_error, PurchaseError,
};
use archava_domain::erp::{
    accounting::{self, Command, Mutation, Receipt, SourceAuthority},
    organizations::OrgAccess,
    purchasing::PurchasePrincipal,
    ErpError,
};
pub use bank_periods::readiness as bank_readiness;
pub use close::close_readiness;
pub use inventory::inventory_control;
pub use page::Page;
pub use reads::{reports, workspace};
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
pub use subledger::{subledger_report, SubledgerPage, SubledgerSide};
use uuid::Uuid;

pub const TABLES: &[&str] = &[
    "memberships",
    "books",
    "accounts",
    "periods",
    "journals",
    "journal_lines",
    "commands",
    "audit",
    "invoice_policies",
    "invoice_sources",
    "journal_sources",
    "purchase_policies",
    "purchase_origins",
    "purchase_sources",
    "purchase_journal_sources",
    "stock_layer_origins",
    "stock_policies",
    "stock_sources",
    "stock_journal_sources",
    "bank_accounts",
    "bank_statements",
    "bank_rows",
    "bank_allocations",
    "bank_unmatches",
    "bank_policies",
    "bank_reconciliations",
    "payable_settlements",
    "clearing_settlements",
    "document_terms",
];
fn db(e: sqlx::Error) -> PurchaseError {
    purchase_database_error("accounting", e)
}
fn id(s: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(s).map_err(|_| ErpError::Conflict.into())
}
pub async fn access(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
) -> Result<OrgAccess, PurchaseError> {
    if p.kind.as_str() != "human" || p.on_behalf_of.is_some() {
        return Err(ErpError::Forbidden.into());
    }
    let m = load_locked_membership(
        conn,
        "archava_accounting",
        "accounting",
        tenant,
        &p.id,
        accounting::PERMISSIONS,
    )
    .await?
    .ok_or(ErpError::Forbidden)?;
    if !m.active || m.kind.as_str() != "human" || !m.permissions.iter().any(|v| v == "books.read") {
        return Err(ErpError::Forbidden.into());
    }
    Ok(OrgAccess {
        permissions: m.permissions,
        owner_id: p.id.clone(),
        scope: m.scope,
    })
}
fn require(a: &OrgAccess, permission: &str) -> Result<(), PurchaseError> {
    if a.holds(&["books.read", permission]) {
        Ok(())
    } else {
        Err(ErpError::Forbidden.into())
    }
}
struct Book {
    id: Uuid,
    company: company_trade::CompanyStamp,
    source: String,
    independent: bool,
}
async fn book(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    a: &OrgAccess,
    key: Uuid,
    lock: bool,
) -> Result<Book, PurchaseError> {
    let row=sqlx::query("SELECT company_id FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4)")
        .bind(tenant).bind(key).bind(a.scope.covers_all()).bind(&a.owner_id).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
    let company =
        company_trade::resolve(conn, tenant, p, row.try_get("company_id").map_err(db)?).await?;
    let q = if lock {
        "SELECT source_authority,independent_posting FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2 FOR UPDATE"
    } else {
        "SELECT source_authority,independent_posting FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2 FOR SHARE"
    };
    let row = sqlx::query(q)
        .bind(tenant)
        .bind(key)
        .fetch_one(conn)
        .await
        .map_err(db)?;
    Ok(Book {
        id: key,
        company,
        source: row.try_get("source_authority").map_err(db)?,
        independent: row.try_get("independent_posting").map_err(db)?,
    })
}
async fn book_for(
    conn: &mut PgConnection,
    tenant: &str,
    kind: &str,
    key: Uuid,
) -> Result<Uuid, PurchaseError> {
    let q = match kind {
        "account" => "SELECT book_id FROM archava_accounting.accounts WHERE tenant_id=$1 AND id=$2",
        "period" => "SELECT book_id FROM archava_accounting.periods WHERE tenant_id=$1 AND id=$2",
        "journal" => "SELECT book_id FROM archava_accounting.journals WHERE tenant_id=$1 AND id=$2",
        "bank_account" => {
            "SELECT book_id FROM archava_accounting.bank_accounts WHERE tenant_id=$1 AND id=$2"
        }
        "bank_statement" => {
            "SELECT book_id FROM archava_accounting.bank_statements WHERE tenant_id=$1 AND id=$2"
        }
        "bank_row" => {
            "SELECT book_id FROM archava_accounting.bank_rows WHERE tenant_id=$1 AND id=$2"
        }
        "bank_allocation" => {
            "SELECT book_id FROM archava_accounting.bank_allocations WHERE tenant_id=$1 AND id=$2"
        }
        "bank_unmatch" => {
            "SELECT book_id FROM archava_accounting.bank_unmatches WHERE tenant_id=$1 AND id=$2"
        }
        "bank_policy" => "SELECT book_id FROM archava_accounting.bank_policies WHERE tenant_id=$1 AND id=$2",
        "bank_reconciliation" => "SELECT book_id FROM archava_accounting.bank_reconciliations WHERE tenant_id=$1 AND id=$2",
        "payable_settlement" => "SELECT book_id FROM archava_accounting.payable_settlements WHERE tenant_id=$1 AND id=$2",
        "clearing_settlement" => "SELECT book_id FROM archava_accounting.clearing_settlements WHERE tenant_id=$1 AND id=$2",
        "document_terms" => "SELECT book_id FROM archava_accounting.document_terms WHERE tenant_id=$1 AND id=$2",
        "tax_code" => "SELECT book_id FROM archava_accounting.tax_codes WHERE tenant_id=$1 AND id=$2",
        "invoice_policy" => {
            "SELECT book_id FROM archava_accounting.invoice_policies WHERE tenant_id=$1 AND id=$2"
        }
        "purchase_source" => {
            "SELECT book_id FROM archava_accounting.purchase_sources WHERE tenant_id=$1 AND id=$2"
        }
        "purchase_policy" => {
            "SELECT book_id FROM archava_accounting.purchase_policies WHERE tenant_id=$1 AND id=$2"
        }
        "invoice_source" => {
            "SELECT book_id FROM archava_accounting.invoice_sources WHERE tenant_id=$1 AND id=$2"
        }
        "stock_policy" => {
            "SELECT book_id FROM archava_accounting.stock_policies WHERE tenant_id=$1 AND id=$2"
        }
        "stock_source" => {
            "SELECT book_id FROM archava_accounting.stock_sources WHERE tenant_id=$1 AND id=$2"
        }
        _ => return Err(ErpError::Conflict.into()),
    };
    sqlx::query_scalar(q)
        .bind(tenant)
        .bind(key)
        .fetch_optional(conn)
        .await
        .map_err(db)?
        .ok_or_else(|| ErpError::NotFound.into())
}
fn operation_permission(op: &str) -> Option<&'static str> {
    Some(match op {
        "invoice_terms.set" | "supplier_terms.set" => "terms.write",
        "bank_account.create" => "bank.configure",
        "bank_policy.set" => "bank.configure",
        "bank_reconciliation.confirm" => "bank.reconcile",
        "bank_statement.import" => "bank.import",
        "bank_allocation.record" | "bank_allocation.reverse" => "bank.reconcile",
        "payable_payment.draft" | "payable_return.draft" => "payables.write",
        "clearing_settlement.draft" => "clearing.write",
        "tax_code.create" | "tax_code.confirm" | "tax_code.retire" => "tax.manage",
        "book.create" | "invoice_policy.set" | "purchase_policy.set" | "stock_policy.set" => {
            "books.write"
        }
        "account.create" | "account.set_active" => "accounts.write",
        "period.open" | "period.close" => "periods.close",
        "period.reopen" => "periods.reopen",
        "journal.draft"
        | "journal.void"
        | "journal.reverse"
        | "invoice_source.draft"
        | "purchase_source.draft"
        | "stock_source.draft"
        | "opening_balance.draft" => "journals.write",
        "journal.post" => "journals.post",
        _ => return None,
    })
}
pub async fn status(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    a: &OrgAccess,
    request: Uuid,
) -> Result<Value, PurchaseError> {
    let held:Option<(String,Option<Value>)>=sqlx::query_as("SELECT actor_id,receipt FROM archava_accounting.commands WHERE tenant_id=$1 AND request_id=$2")
        .bind(tenant).bind(request).fetch_optional(&mut *conn).await.map_err(db)?;
    let Some((actor, Some(value))) = held else {
        return Ok(json!({"decision":"not_committed","requestId":request,"receipt":null}));
    };
    if actor != p.id {
        return Err(ErpError::Forbidden.into());
    }
    let mut r: Receipt = serde_json::from_value(value).map_err(|_| ErpError::Conflict)?;
    if r.operation.starts_with("bank_") {
        require(a, "bank.read")?;
    }
    require(
        a,
        operation_permission(&r.operation).ok_or(ErpError::Conflict)?,
    )?;
    if r.operation.starts_with("payable_") {
        require(a, "payables.read")?;
        require(a, "bank.read")?;
        require(a, "journals.write")?;
    }
    if r.operation == "clearing_settlement.draft" {
        require(a, "clearing.read")?;
        require(a, "bank.read")?;
        require(a, "journals.write")?;
    }
    if r.resource_kind == "tax_code" {
        require(a, "tax.read")?;
    }
    if r.resource_kind == "document_terms" {
        let snapshot = terms::read_authority(conn, tenant, a, id(&r.resource_id)?).await?;
        if r.document_terms.as_ref() != Some(&snapshot)
            || r.source_id.as_deref() != snapshot["sourceId"].as_str()
        {
            return Err(ErpError::Conflict.into());
        }
    }
    if r.resource_kind == "journal" {
        clearing::journal_authority(
            conn,
            tenant,
            a,
            id(&r.resource_id)?,
            r.operation == "journal.post",
        )
        .await?;
        payables::journal_authority(
            conn,
            tenant,
            a,
            id(&r.resource_id)?,
            r.operation == "journal.post",
        )
        .await?;
    }
    let b = book(conn, tenant, p, a, id(&r.book_id)?, false).await?;
    if id(&r.request_id)? != request || id(&r.company_id)? != b.company.id || r.revision < 1 {
        return Err(ErpError::Conflict.into());
    }
    if r.operation == "bank_allocation.reverse" {
        if r.resource_kind != "bank_unmatch" {
            return Err(ErpError::Conflict.into());
        }
        // Older committed receipts lack this optional reference. Recover it from
        // immutable evidence without rewriting the historical command or audit.
        let original: Uuid = sqlx::query_scalar("SELECT allocation_id FROM archava_accounting.bank_unmatches WHERE tenant_id=$1 AND book_id=$2 AND id=$3")
            .bind(tenant).bind(b.id).bind(id(&r.resource_id)?).fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::Conflict)?;
        let original = original.to_string();
        if r.original_bank_allocation_id
            .as_ref()
            .is_some_and(|v| v != &original)
        {
            return Err(ErpError::Conflict.into());
        }
        r.original_bank_allocation_id = Some(original);
    }
    r.replayed = true;
    Ok(json!({"decision":"committed","requestId":request,"receipt":r}))
}

pub async fn record(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    a: &OrgAccess,
    fingerprint: &str,
    m: &Mutation,
) -> Result<Receipt, PurchaseError> {
    require(a, m.command.permission())?;
    if matches!(
        m.command,
        Command::InvoiceTermsSet { .. } | Command::SupplierTermsSet { .. }
    ) {
        require(a, "terms.read")?;
        if matches!(m.command, Command::SupplierTermsSet { .. }) {
            require(a, "payables.read")?;
        }
    }
    let tax_command = matches!(
        m.command,
        Command::TaxCodeCreate { .. }
            | Command::TaxCodeConfirm { .. }
            | Command::TaxCodeRetire { .. }
    );
    if tax_command {
        require(a, "tax.read")?;
    }
    if matches!(m.command, Command::ClearingSettlementDraft { .. }) {
        require(a, "clearing.read")?;
        require(a, "bank.read")?;
        require(a, "journals.write")?;
    }
    if m.command.operation().starts_with("bank_") {
        require(a, "bank.read")?;
    }
    if matches!(
        m.command,
        Command::PayablePaymentDraft { .. } | Command::PayableReturnDraft { .. }
    ) {
        require(a, "payables.read")?;
        require(a, "bank.read")?;
        require(a, "journals.write")?;
    }
    if let Command::JournalPost { id: journal, .. }
    | Command::JournalVoid { id: journal, .. }
    | Command::JournalReverse { id: journal, .. } = &m.command
    {
        clearing::journal_authority(
            conn,
            tenant,
            a,
            id(journal)?,
            matches!(m.command, Command::JournalPost { .. }),
        )
        .await?;
        payables::journal_authority(
            conn,
            tenant,
            a,
            id(journal)?,
            matches!(m.command, Command::JournalPost { .. }),
        )
        .await?;
    }
    let request = id(&m.request_id)?;
    let claimed=sqlx::query("INSERT INTO archava_accounting.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(request).bind(&p.id).bind(fingerprint).execute(&mut *conn).await.map_err(db)?.rows_affected()==1;
    if !claimed {
        let held:(String,String)=sqlx::query_as("SELECT actor_id,fingerprint FROM archava_accounting.commands WHERE tenant_id=$1 AND request_id=$2").bind(tenant).bind(request).fetch_one(&mut *conn).await.map_err(db)?;
        if held != (p.id.clone(), fingerprint.into()) {
            return Err(ErpError::Conflict.into());
        }
        return serde_json::from_value(
            status(conn, tenant, p, a, request).await?["receipt"].clone(),
        )
        .map_err(|_| ErpError::Conflict.into());
    }
    let (kind, resource, book_id, company_id, revision) = if let Command::BookCreate {
        company_id,
        expected_company_revision,
        code,
        name,
        currency,
        source_authority,
        external_system,
        independent_posting,
        ..
    } = &m.command
    {
        // A non-IDR book excludes concurrent company trade inserts (Accounting
        // 015); an IDR book keeps the shared lock it always took.
        let company = if currency == "IDR" {
            company_trade::resolve(conn, tenant, p, id(company_id)?).await?
        } else {
            company_trade::resolve_excluding_trade(conn, tenant, p, id(company_id)?).await?
        };
        company.validate_new(i64::from(*expected_company_revision))?;
        let key = Uuid::new_v4();
        sqlx::query("INSERT INTO archava_accounting.books(tenant_id,id,company_id,company_revision,company_code,company_name,owner_id,code,name,currency,minor_units,currency_catalogue,source_authority,external_system,independent_posting) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'SIX-2026-09-17',$12,$13,$14)")
            .bind(tenant).bind(key).bind(company.id).bind(company.revision).bind(company.code).bind(company.name).bind(&p.id).bind(code).bind(name).bind(currency).bind(i16::from(accounting::minor_units(currency).ok_or(ErpError::Conflict)?)).bind(source_authority.as_str()).bind(external_system).bind(independent_posting).execute(&mut *conn).await.map_err(db)?;
        ("book", key, key, company.id, 1)
    } else {
        let key = match &m.command {
            Command::BankAccountCreate { book_id, .. }
            | Command::StockPolicySet { book_id, .. }
            | Command::PurchasePolicySet { book_id, .. }
            | Command::InvoicePolicySet { book_id, .. }
            | Command::AccountCreate { book_id, .. }
            | Command::PeriodOpen { book_id, .. }
            | Command::JournalDraft { book_id, .. }
            | Command::OpeningBalanceDraft { book_id, .. }
            | Command::TaxCodeCreate { book_id, .. } => id(book_id)?,
            Command::TaxCodeConfirm { id: key, .. } | Command::TaxCodeRetire { id: key, .. } => {
                book_for(conn, tenant, "tax_code", id(key)?).await?
            }
            Command::BankPolicySet {
                bank_account_id, ..
            }
            | Command::BankStatementImport {
                bank_account_id, ..
            } => book_for(conn, tenant, "bank_account", id(bank_account_id)?).await?,
            Command::BankAllocationRecord { bank_row_id, .. } => {
                book_for(conn, tenant, "bank_row", id(bank_row_id)?).await?
            }
            Command::BankAllocationReverse { id: key, .. } => {
                book_for(conn, tenant, "bank_allocation", id(key)?).await?
            }
            Command::BankReconciliationConfirm { bank_policy_id, .. } => {
                book_for(conn, tenant, "bank_policy", id(bank_policy_id)?).await?
            }
            Command::PurchaseSourceDraft { source_id, .. }
            | Command::PayablePaymentDraft { source_id, .. }
            | Command::SupplierTermsSet { source_id, .. } => {
                book_for(conn, tenant, "purchase_source", id(source_id)?).await?
            }
            Command::PayableReturnDraft {
                original_payment_id,
                ..
            } => book_for(conn, tenant, "payable_settlement", id(original_payment_id)?).await?,
            Command::StockSourceDraft { source_id, .. } => {
                book_for(conn, tenant, "stock_source", id(source_id)?).await?
            }
            Command::InvoiceSourceDraft { source_id, .. }
            | Command::ClearingSettlementDraft { source_id, .. }
            | Command::InvoiceTermsSet { source_id, .. } => {
                book_for(conn, tenant, "invoice_source", id(source_id)?).await?
            }
            Command::AccountSetActive { id: key, .. } => {
                book_for(conn, tenant, "account", id(key)?).await?
            }
            Command::PeriodClose { id: key, .. } | Command::PeriodReopen { id: key, .. } => {
                book_for(conn, tenant, "period", id(key)?).await?
            }
            Command::JournalPost { id: key, .. }
            | Command::JournalVoid { id: key, .. }
            | Command::JournalReverse { id: key, .. } => {
                book_for(conn, tenant, "journal", id(key)?).await?
            }
            _ => return Err(ErpError::Conflict.into()),
        };
        if matches!(
            m.command,
            Command::PurchasePolicySet { .. } | Command::StockPolicySet { .. }
        ) {
            // Plain scoped read before the company advisory lock. Taking a book
            // SHARE lock here would deadlock confirmation's company -> book order.
            let company: Uuid = sqlx::query_scalar("SELECT company_id FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4)")
                .bind(tenant).bind(key).bind(a.scope.covers_all()).bind(&a.owner_id)
                .fetch_optional(&mut *conn).await.map_err(db)?.ok_or(ErpError::NotFound)?;
            sqlx::query("SELECT archava_accounting.lock_purchase_company($1,$2)")
                .bind(tenant)
                .bind(company)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
        }
        let b = book(conn, tenant, p, a, key, true).await?;
        // An operating-company deactivation cannot strand its existing book:
        // authorized adjustments, settlement, reversal and close remain possible.
        // New book setup above still requires an active reviewed company.
        if b.source != SourceAuthority::Native.as_str() {
            // Decided here before the Accounting 016 guard would refuse it.
            if tax_command {
                return Err(super::refused_by_code("tax_code_native_book_required"));
            }
            return Err(ErpError::Conflict.into());
        }
        let (kind, resource, revision) = match &m.command {
            Command::TaxCodeCreate { .. }
            | Command::TaxCodeConfirm { .. }
            | Command::TaxCodeRetire { .. } => {
                tax::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::InvoiceTermsSet { .. } | Command::SupplierTermsSet { .. } => {
                terms::execute(conn, tenant, p, &b, &m.command, request).await?
            }
            Command::ClearingSettlementDraft { .. } => {
                clearing::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::PayablePaymentDraft { .. } | Command::PayableReturnDraft { .. } => {
                payables::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::OpeningBalanceDraft { .. } => {
                openings::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::BankPolicySet { .. } | Command::BankReconciliationConfirm { .. } => {
                bank_periods::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::BankAccountCreate { .. }
            | Command::BankStatementImport { .. }
            | Command::BankAllocationRecord { .. }
            | Command::BankAllocationReverse { .. } => {
                bank::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::StockPolicySet { .. } | Command::StockSourceDraft { .. } => {
                stock::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::PurchasePolicySet { .. } | Command::PurchaseSourceDraft { .. } => {
                purchases::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::InvoicePolicySet { .. } | Command::InvoiceSourceDraft { .. } => {
                sources::execute(conn, tenant, p, &b, &m.command).await?
            }
            Command::AccountCreate {
                code,
                name,
                class,
                cash_role,
                ..
            } => {
                let key = Uuid::new_v4();
                sqlx::query("INSERT INTO archava_accounting.accounts(tenant_id,id,book_id,code,name,class,cash_role) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(key).bind(b.id).bind(code).bind(name).bind(class.as_str()).bind(cash_role.as_str()).execute(&mut *conn).await.map_err(db)?;
                ("account", key, 1)
            }
            Command::AccountSetActive {
                id: key,
                expected_revision,
                active,
                ..
            } => {
                let key = id(key)?;
                let revision:Option<i32>=sqlx::query_scalar("UPDATE archava_accounting.accounts SET active=$4,revision=revision+1 WHERE tenant_id=$1 AND id=$2 AND revision=$3 AND active<>$4 RETURNING revision").bind(tenant).bind(key).bind(expected_revision).bind(active).fetch_optional(&mut *conn).await.map_err(db)?;
                ("account", key, revision.ok_or(ErpError::Conflict)?)
            }
            Command::PeriodOpen {
                code,
                start_date,
                end_date,
                ..
            } => {
                let key = Uuid::new_v4();
                sqlx::query("INSERT INTO archava_accounting.periods(tenant_id,id,book_id,code,start_date,end_date) VALUES($1,$2,$3,$4,$5::text::date,$6::text::date)").bind(tenant).bind(key).bind(b.id).bind(code).bind(start_date).bind(end_date).execute(&mut *conn).await.map_err(db)?;
                ("period", key, 1)
            }
            Command::PeriodClose {
                id: key,
                expected_revision,
                ..
            }
            | Command::PeriodReopen {
                id: key,
                expected_revision,
                ..
            } => {
                let key = id(key)?;
                let state = if matches!(m.command, Command::PeriodClose { .. }) {
                    "closed"
                } else {
                    "open"
                };
                let revision:Option<i32>=sqlx::query_scalar("UPDATE archava_accounting.periods SET state=$4,revision=revision+1 WHERE tenant_id=$1 AND id=$2 AND revision=$3 AND state<>$4 RETURNING revision").bind(tenant).bind(key).bind(expected_revision).bind(state).fetch_optional(&mut *conn).await.map_err(db)?;
                ("period", key, revision.ok_or(ErpError::Conflict)?)
            }
            _ => journal::execute(conn, tenant, p, &b, &m.command).await?,
        };
        (kind, resource, b.id, b.company.id, revision)
    };
    let receipt = Receipt {
        request_id: request.to_string(),
        operation: m.command.operation().into(),
        resource_kind: kind.into(),
        resource_id: resource.to_string(),
        book_id: book_id.to_string(),
        company_id: company_id.to_string(),
        revision,
        replayed: false,
        original_journal_id: match &m.command {
            Command::JournalReverse { id, .. } => Some(id.clone()),
            _ => None,
        },
        original_bank_allocation_id: match &m.command {
            Command::BankAllocationReverse { id, .. } => Some(id.clone()),
            _ => None,
        },
        source_id: match &m.command {
            Command::StockSourceDraft { source_id, .. }
            | Command::PurchaseSourceDraft { source_id, .. }
            | Command::InvoiceSourceDraft { source_id, .. }
            | Command::PayablePaymentDraft { source_id, .. }
            | Command::InvoiceTermsSet { source_id, .. }
            | Command::SupplierTermsSet { source_id, .. }
            | Command::ClearingSettlementDraft { source_id, .. } => Some(source_id.clone()),
            Command::PayableReturnDraft {..} => Some(sqlx::query_scalar::<_,Uuid>("SELECT source_id FROM archava_accounting.payable_settlements WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(resource).fetch_one(&mut *conn).await.map_err(db)?.to_string()),
            _ => None,
        },
        original_payable_payment_id: match &m.command {
            Command::PayableReturnDraft {original_payment_id, ..} => Some(original_payment_id.clone()),
            _ => None,
        },
        bank_row_id: match &m.command {
            Command::PayablePaymentDraft {bank_row_id, ..} | Command::PayableReturnDraft {bank_row_id, ..} | Command::ClearingSettlementDraft {bank_row_id, ..} => Some(bank_row_id.clone()),
            _ => None,
        },
        bank_check_hash: match &m.command {
            Command::BankReconciliationConfirm {
                expected_check_hash,
                ..
            } => Some(expected_check_hash.clone()),
            _ => None,
        },
        document_terms: if kind == "document_terms" {
            Some(terms::snapshot(conn, tenant, resource).await?)
        } else {
            None
        },
        tax_code_state: if kind == "tax_code" {
            Some(tax::state(conn, tenant, resource).await?)
        } else {
            None
        },
    };
    let snapshot = snapshot(conn, tenant, kind, resource).await?;
    sqlx::query("INSERT INTO archava_accounting.audit(tenant_id,request_id,actor_id,operation,resource_kind,resource_id,book_id,revision,reason,snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(tenant).bind(request).bind(&p.id).bind(m.command.operation()).bind(kind).bind(resource).bind(book_id).bind(revision).bind(m.command.reason()).bind(snapshot).execute(&mut *conn).await.map_err(db)?;
    sqlx::query(
        "UPDATE archava_accounting.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request)
    .bind(json!(receipt))
    .execute(conn)
    .await
    .map_err(db)?;
    Ok(receipt)
}
async fn snapshot(
    conn: &mut PgConnection,
    tenant: &str,
    kind: &str,
    key: Uuid,
) -> Result<Value, PurchaseError> {
    let q=match kind{
        "document_terms"=>"SELECT jsonb_build_object('record',to_jsonb(r),'documentTerms',archava_accounting.terms_snapshot(r)) FROM archava_accounting.document_terms r WHERE tenant_id=$1 AND id=$2",
        "clearing_settlement"=>"SELECT jsonb_build_object('record',to_jsonb(r)||jsonb_build_object('gross_minor',r.gross_minor::text,'fee_minor',r.fee_minor::text,'cash_minor',r.cash_minor::text)) FROM archava_accounting.clearing_settlements r WHERE tenant_id=$1 AND id=$2",
        "payable_settlement"=>"SELECT jsonb_build_object('record',to_jsonb(r)||jsonb_build_object('amount_minor',r.amount_minor::text)) FROM archava_accounting.payable_settlements r WHERE tenant_id=$1 AND id=$2",
        "bank_account"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.bank_accounts r WHERE tenant_id=$1 AND id=$2",
        "bank_statement"=>"SELECT archava_accounting.bank_statement_snapshot($1,$2)",
        "bank_row"=>"SELECT jsonb_build_object('record',to_jsonb(r)||jsonb_build_object('amount_minor',r.amount_minor::text)) FROM archava_accounting.bank_rows r WHERE tenant_id=$1 AND id=$2",
        "bank_allocation"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.bank_allocations r WHERE tenant_id=$1 AND id=$2",
        "bank_unmatch"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.bank_unmatches r WHERE tenant_id=$1 AND id=$2",
        "bank_policy"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.bank_policies r WHERE tenant_id=$1 AND id=$2",
        "bank_reconciliation"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.bank_reconciliations r WHERE tenant_id=$1 AND id=$2",
        "tax_code"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.tax_codes r WHERE tenant_id=$1 AND id=$2",
        "book"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.books r WHERE tenant_id=$1 AND id=$2",
        "account"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.accounts r WHERE tenant_id=$1 AND id=$2",
        "period"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.periods r WHERE tenant_id=$1 AND id=$2",
        "purchase_policy"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.purchase_policies r WHERE tenant_id=$1 AND id=$2",
        "purchase_origin"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.purchase_origins r WHERE tenant_id=$1 AND id=$2",
        "purchase_source"=>"SELECT jsonb_build_object('record',to_jsonb(r)||jsonb_build_object('goods_minor',r.goods_minor::text,'landed_fee_minor',r.landed_fee_minor::text)) FROM archava_accounting.purchase_sources r WHERE tenant_id=$1 AND id=$2",
        "stock_policy"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.stock_policies r WHERE tenant_id=$1 AND id=$2",
        "stock_source"=>"SELECT jsonb_build_object('record',to_jsonb(r)||jsonb_build_object('amount_minor',r.amount_minor::text)) FROM archava_accounting.stock_sources r WHERE tenant_id=$1 AND id=$2",
        "invoice_policy"=>"SELECT jsonb_build_object('record',to_jsonb(r)) FROM archava_accounting.invoice_policies r WHERE tenant_id=$1 AND id=$2",
        "invoice_source"=>"SELECT jsonb_build_object('record',jsonb_set(to_jsonb(r),'{amount_minor}',to_jsonb(r.amount_minor::text))) FROM archava_accounting.invoice_sources r WHERE tenant_id=$1 AND id=$2",
        "journal"=>"SELECT jsonb_build_object('record',to_jsonb(r),'lines',(SELECT jsonb_agg(to_jsonb(l) ORDER BY ordinal) FROM archava_accounting.journal_lines l WHERE l.tenant_id=r.tenant_id AND l.journal_id=r.id)) FROM archava_accounting.journals r WHERE tenant_id=$1 AND id=$2",
        _=>return Err(ErpError::Conflict.into()),
    };
    sqlx::query_scalar(q)
        .bind(tenant)
        .bind(key)
        .fetch_one(conn)
        .await
        .map_err(db)
}
