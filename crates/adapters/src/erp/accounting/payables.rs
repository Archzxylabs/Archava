//! Reviewed recording of actual supplier bank movements, never a transfer API.
use super::*;
use archava_domain::erp::accounting::{payables as rules, Line};

pub(super) async fn journal_authority(
    conn: &mut PgConnection,
    tenant: &str,
    access: &OrgAccess,
    journal: Uuid,
    posting: bool,
) -> Result<(), PurchaseError> {
    let linked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_accounting.payable_settlements WHERE tenant_id=$1 AND id=$2)")
        .bind(tenant).bind(journal).fetch_one(conn).await.map_err(db)?;
    if linked {
        require(access, "payables.read")?;
        require(access, "bank.read")?;
        if !posting {
            require(access, "payables.write")?;
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
    let (source, source_revision, original, row, code, payable, cash, amount, reason, effect) =
        match command {
            Command::PayablePaymentDraft {
                source_id,
                expected_document_revision,
                bank_row_id,
                code,
                payable_account,
                cash_account,
                amount_minor,
                reason,
            } => (
                id(source_id)?,
                *expected_document_revision,
                None,
                id(bank_row_id)?,
                code,
                payable_account,
                cash_account,
                amount_minor,
                reason,
                rules::Effect::Payment,
            ),
            Command::PayableReturnDraft {
                original_payment_id,
                expected_journal_revision,
                bank_row_id,
                code,
                payable_account,
                cash_account,
                amount_minor,
                reason,
            } => {
                let original = id(original_payment_id)?;
                let held: Option<(Uuid,i32)> = sqlx::query_as("SELECT s.source_id,s.document_revision FROM archava_accounting.payable_settlements s JOIN archava_accounting.journals j ON j.tenant_id=s.tenant_id AND j.id=s.id WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.id=$3 AND s.effect='payment' AND j.state='posted' AND j.revision=$4")
                .bind(tenant).bind(b.id).bind(original).bind(expected_journal_revision).fetch_optional(&mut *conn).await.map_err(db)?;
                let (source, revision) = held.ok_or(ErpError::Conflict)?;
                (
                    source,
                    revision,
                    Some(original),
                    id(bank_row_id)?,
                    code,
                    payable_account,
                    cash_account,
                    amount_minor,
                    reason,
                    rules::Effect::Return,
                )
            }
            _ => return Err(ErpError::Conflict.into()),
        };
    let date: Option<String> = sqlx::query_scalar("SELECT entry_date::text FROM archava_accounting.bank_rows WHERE tenant_id=$1 AND book_id=$2 AND id=$3")
        .bind(tenant).bind(b.id).bind(row).fetch_optional(&mut *conn).await.map_err(db)?;
    let legs = rules::legs(effect, amount).map_err(|_| ErpError::Conflict)?;
    let lines = legs
        .into_iter()
        .zip([payable, cash])
        .map(|(leg, account)| Line {
            account_id: account.account_id.clone(),
            expected_account_revision: account.expected_account_revision,
            debit_minor: leg.debit_minor,
            credit_minor: leg.credit_minor,
            memo: if effect == rules::Effect::Payment {
                "Reviewed supplier bank payment"
            } else {
                "Reviewed supplier bank return"
            }
            .into(),
        })
        .collect();
    let effect = if effect == rules::Effect::Payment {
        "payment"
    } else {
        "return"
    };
    // Bank-row identifiers stay outside the generic journal/audit projection.
    let draft = Command::JournalDraft {
        book_id: b.id.to_string(),
        code: code.clone(),
        date: date.ok_or(ErpError::NotFound)?,
        source_ref: format!("payable:{effect}:{}:{code}", original.unwrap_or(source)),
        memo: format!("Supplier {effect} for reviewed bill {source}"),
        lines,
        reason: reason.clone(),
    };
    let (_, journal, revision) = journal::execute(conn, tenant, p, b, &draft).await?;
    sqlx::query("INSERT INTO archava_accounting.payable_settlements(tenant_id,id,book_id,source_id,document_revision,bank_row_id,payable_account_id,cash_account_id,amount_minor,effect,original_payment_id,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(tenant).bind(journal).bind(b.id).bind(source).bind(source_revision).bind(row)
        .bind(id(&payable.account_id)?).bind(id(&cash.account_id)?)
        .bind(accounting::minor(amount).map_err(|_| ErpError::Conflict)?).bind(effect).bind(original).bind(&p.id)
        .execute(conn).await.map_err(db)?;
    Ok(("journal", journal, revision))
}

pub(super) async fn summary(
    conn: &mut PgConnection,
    tenant: &str,
    book: Uuid,
) -> Result<Value, PurchaseError> {
    sqlx::query_scalar("SELECT jsonb_build_object('billCount',count(*),'recognizedMinor',coalesce(sum((b->>'recognizedMinor')::numeric),0)::text,'postedPaymentMinor',coalesce(sum((b->>'postedPaymentMinor')::numeric),0)::text,'reservedPaymentMinor',coalesce(sum((b->>'reservedPaymentMinor')::numeric),0)::text,'outstandingMinor',coalesce(sum((b->>'outstandingMinor')::numeric),0)::text,'availableMinor',coalesce(sum((b->>'availableMinor')::numeric),0)::text,'allCapturedBills',true,'unpostedBillCount',count(*) FILTER(WHERE NOT (b->>'billPosted')::boolean),'termsIncluded',false,'legacyPayablesImported',false) FROM archava_accounting.purchase_sources s CROSS JOIN LATERAL archava_accounting.payable_balance(s.tenant_id,s.id) b WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect='bill'")
        .bind(tenant).bind(book).fetch_one(conn).await.map_err(db)
}

pub(super) fn bill_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'code',(SELECT d.vendor_reference FROM archava_purchasing.supplier_documents d WHERE d.tenant_id=s.tenant_id AND d.id=s.document_id),'sourceId',s.id,'documentId',s.document_id,'documentRevision',s.source_revision,'documentState',(SELECT d.state FROM archava_purchasing.supplier_documents d WHERE d.tenant_id=s.tenant_id AND d.id=s.document_id),'currentDocumentRevision',(SELECT d.revision FROM archava_purchasing.supplier_documents d WHERE d.tenant_id=s.tenant_id AND d.id=s.document_id),'supplierName',(SELECT d.supplier_name FROM archava_purchasing.supplier_documents d WHERE d.tenant_id=s.tenant_id AND d.id=s.document_id),'sourceDate',s.source_date,'payableAccountId',(SELECT m->>'accountId' FROM archava_accounting.purchase_origins o CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') m WHERE o.tenant_id=s.tenant_id AND o.id=s.origin_id AND m->>'role'='payable'),'balance',archava_accounting.payable_balance(s.tenant_id,s.id),'dueDate',NULL,'termsKnown',false,'revision',1)"
}
pub(super) fn settlement_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'sourceId',s.source_id,'documentRevision',s.document_revision,'bankRowId',s.bank_row_id,'payableAccountId',s.payable_account_id,'cashAccountId',s.cash_account_id,'amountMinor',s.amount_minor::text,'effect',s.effect,'originalPaymentId',s.original_payment_id,'code',(SELECT j.code FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'date',(SELECT j.entry_date FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'state',(SELECT j.state FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'revision',(SELECT j.revision FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id),'returnAvailableMinor',(s.amount_minor::numeric-coalesce((SELECT sum(x.amount_minor::numeric) FROM archava_accounting.payable_settlements x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.id WHERE x.tenant_id=s.tenant_id AND x.original_payment_id=s.id AND y.state IN ('draft','posted')),0))::text,'matchedMinor',coalesce((SELECT sum(a.amount_minor::numeric) FROM archava_accounting.bank_allocations a WHERE a.tenant_id=s.tenant_id AND a.journal_id=s.id AND a.bank_row_id=s.bank_row_id AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches x WHERE x.tenant_id=a.tenant_id AND x.allocation_id=a.id)),0)::text)"
}
