//! Reviewed immutable due conditions, separate from financial recognition.
use super::*;
use archava_domain::erp::accounting::terms as rules;

/// Original sources are separate collections because their UUID namespaces may
/// overlap. The version is the live document version, never the capture version.
pub(super) fn invoice_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'code',s.invoice_code,'documentKind','receivable','documentId',s.invoice_id,'documentRevision',(SELECT d.revision FROM archava_billing.invoices d WHERE d.tenant_id=s.tenant_id AND d.id=s.invoice_id),'sourceDate',s.source_date,'revision',1) || coalesce((SELECT jsonb_build_object('termsRevision',t.revision,'latestCondition',archava_accounting.terms_snapshot(t)->'condition','latestEffectiveFrom',t.effective_from,'latestRecordedAt',t.recorded_at) FROM archava_accounting.document_terms t WHERE t.tenant_id=s.tenant_id AND t.book_id=s.book_id AND t.document_kind='receivable' AND t.document_id=s.invoice_id ORDER BY t.revision DESC LIMIT 1),jsonb_build_object('termsRevision',0,'latestCondition',NULL,'latestEffectiveFrom',NULL,'latestRecordedAt',NULL))"
}
pub(super) fn supplier_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'code','Supplier bill · '||s.document_id::text,'documentKind','payable','documentId',s.document_id,'documentRevision',(SELECT d.revision FROM archava_purchasing.supplier_documents d WHERE d.tenant_id=s.tenant_id AND d.id=s.document_id),'sourceDate',s.source_date,'revision',1) || coalesce((SELECT jsonb_build_object('termsRevision',t.revision,'latestCondition',archava_accounting.terms_snapshot(t)->'condition','latestEffectiveFrom',t.effective_from,'latestRecordedAt',t.recorded_at) FROM archava_accounting.document_terms t WHERE t.tenant_id=s.tenant_id AND t.book_id=s.book_id AND t.document_kind='payable' AND t.document_id=s.document_id ORDER BY t.revision DESC LIMIT 1),jsonb_build_object('termsRevision',0,'latestCondition',NULL,'latestEffectiveFrom',NULL,'latestRecordedAt',NULL))"
}
pub(super) fn history_projection() -> &'static str {
    "archava_accounting.terms_snapshot(s) || jsonb_build_object('id',s.id,'bookId',s.book_id,'revision',s.revision,'code',s.document_kind||' · '||s.document_id::text||' · terms '||s.revision::text,'reviewedBy',s.created_by,'recordedAt',s.recorded_at,'reason',s.reason)"
}
pub(super) fn payable_due_projection(visible: bool) -> &'static str {
    if visible {
        "coalesce((SELECT jsonb_build_object('termsKnown',t.due_kind='due_on','dueDate',t.due_date,'dueTerms',archava_accounting.terms_snapshot(t)||jsonb_build_object('recordedAt',t.recorded_at)) FROM archava_accounting.document_terms t WHERE t.tenant_id=s.tenant_id AND t.book_id=s.book_id AND t.document_kind='payable' AND t.document_id=s.document_id AND t.effective_from<=(clock_timestamp() AT TIME ZONE 'UTC')::date ORDER BY t.revision DESC LIMIT 1),jsonb_build_object('termsKnown',false,'dueDate',NULL,'dueTerms',NULL))"
    } else {
        "jsonb_build_object('termsKnown',NULL,'dueDate',NULL,'dueTerms',NULL)"
    }
}

pub(super) async fn read_authority(
    conn: &mut PgConnection,
    tenant: &str,
    access: &OrgAccess,
    key: Uuid,
) -> Result<Value, PurchaseError> {
    require(access, "terms.read")?;
    let row:Option<(String,Value)>=sqlx::query_as("SELECT document_kind,archava_accounting.terms_snapshot(t) FROM archava_accounting.document_terms t WHERE tenant_id=$1 AND id=$2")
        .bind(tenant).bind(key).fetch_optional(conn).await.map_err(db)?;
    let (kind, snapshot) = row.ok_or(ErpError::NotFound)?;
    if kind == "payable" {
        require(access, "payables.read")?;
    }
    Ok(snapshot)
}

pub(super) async fn execute(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    book: &Book,
    command: &Command,
    request: Uuid,
) -> Result<(&'static str, Uuid, i32), PurchaseError> {
    let (source, expected_document, expected_terms, effective, condition, reason, kind) =
        match command {
            Command::InvoiceTermsSet {
                source_id,
                expected_document_revision,
                expected_revision,
                effective_from,
                condition,
                reason,
            } => (
                source_id,
                *expected_document_revision,
                *expected_revision,
                effective_from,
                condition,
                reason,
                "receivable",
            ),
            Command::SupplierTermsSet {
                source_id,
                expected_document_revision,
                expected_revision,
                effective_from,
                condition,
                reason,
            } => (
                source_id,
                *expected_document_revision,
                *expected_revision,
                effective_from,
                condition,
                reason,
                "payable",
            ),
            _ => return Err(ErpError::Conflict.into()),
        };
    let source = id(source)?;
    // Separate fixed relations preserve ledger identity even if the same UUID
    // was legitimately used as a request key in both source namespaces.
    let query = if kind == "receivable" {
        "SELECT s.invoice_id,s.source_date::text,d.revision FROM archava_accounting.invoice_sources s JOIN archava_billing.invoices d ON d.tenant_id=s.tenant_id AND d.id=s.invoice_id WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.id=$3 AND s.effect='issue'"
    } else {
        "SELECT s.document_id,s.source_date::text,d.revision FROM archava_accounting.purchase_sources s JOIN archava_purchasing.supplier_documents d ON d.tenant_id=s.tenant_id AND d.id=s.document_id WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.id=$3 AND s.effect='bill'"
    };
    let original: Option<(Uuid, String, i32)> = sqlx::query_as(query)
        .bind(tenant)
        .bind(book.id)
        .bind(source)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db)?;
    let (document, source_date, document_revision) = original.ok_or(ErpError::Conflict)?;
    if document_revision != expected_document {
        return Err(ErpError::Conflict.into());
    }
    let previous=sqlx::query("SELECT revision,effective_from::text,due_kind,due_date::text FROM archava_accounting.document_terms WHERE tenant_id=$1 AND book_id=$2 AND document_kind=$3 AND document_id=$4 ORDER BY revision DESC LIMIT 1")
        .bind(tenant).bind(book.id).bind(kind).bind(document).fetch_optional(&mut *conn).await.map_err(db)?;
    let previous = previous
        .map(|row| -> Result<rules::Terms, PurchaseError> {
            let revision: i32 = row.try_get("revision").map_err(db)?;
            let condition = match row.try_get::<String, _>("due_kind").map_err(db)?.as_str() {
                "unknown" => rules::Condition::Unknown {},
                "due_on" => rules::Condition::DueOn {
                    date: row
                        .try_get::<Option<String>, _>("due_date")
                        .map_err(db)?
                        .ok_or(ErpError::Conflict)?,
                },
                _ => return Err(ErpError::Conflict.into()),
            };
            Ok(rules::Terms {
                revision: revision.try_into().map_err(|_| ErpError::Conflict)?,
                effective_from: row.try_get("effective_from").map_err(db)?,
                condition,
            })
        })
        .transpose()?;
    let next = rules::next(
        &source_date,
        previous.as_ref(),
        expected_terms.try_into().map_err(|_| ErpError::Conflict)?,
        effective,
        condition.clone(),
    )
    .map_err(|_| ErpError::Conflict)?;
    let revision: i32 = next.revision.try_into().map_err(|_| ErpError::Conflict)?;
    let key = Uuid::new_v4();
    let due_kind = if next.condition.due_date().is_some() {
        "due_on"
    } else {
        "unknown"
    };
    sqlx::query("INSERT INTO archava_accounting.document_terms(tenant_id,id,book_id,request_id,document_kind,source_id,document_id,document_revision,revision,effective_from,due_kind,due_date,created_by,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::text::date,$11,$12::text::date,$13,$14)")
        .bind(tenant).bind(key).bind(book.id).bind(request).bind(kind).bind(source).bind(document).bind(document_revision).bind(revision)
        .bind(&next.effective_from).bind(due_kind).bind(next.condition.due_date()).bind(&principal.id).bind(reason).execute(conn).await.map_err(db)?;
    Ok(("document_terms", key, revision))
}

pub(super) async fn snapshot(
    conn: &mut PgConnection,
    tenant: &str,
    key: Uuid,
) -> Result<Value, PurchaseError> {
    sqlx::query_scalar("SELECT archava_accounting.terms_snapshot(t) FROM archava_accounting.document_terms t WHERE tenant_id=$1 AND id=$2")
        .bind(tenant).bind(key).fetch_optional(conn).await.map_err(db)?.ok_or_else(||ErpError::NotFound.into())
}
