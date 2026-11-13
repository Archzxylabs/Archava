//! Complete posted-source positions and independent control comparisons.
//! The caller holds the book's SHARE lock for the entire read transaction.
use super::*;
use archava_domain::erp::accounting::aging;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubledgerSide {
    Receivable,
    Payable,
}
impl SubledgerSide {
    fn name(self) -> &'static str {
        match self {
            Self::Receivable => "receivable",
            Self::Payable => "payable",
        }
    }
}
pub struct SubledgerPage<'a> {
    pub as_of: &'a str,
    pub side: SubledgerSide,
    pub cursor: Option<Uuid>,
    pub limit: usize,
}
impl SubledgerPage<'_> {
    pub fn valid(&self) -> bool {
        archava_domain::erp::accommodation::parse_calendar_date(self.as_of).is_some()
            && (1..=100).contains(&self.limit)
    }
}

// Amounts are independent original-source facts, not journal-line sums or
// today's mutable operating outstanding balance. Collections/refunds have
// signed source amounts; the receivable leg is their opposite.
const RECEIVABLE_POSITIONS: &str = r#"
WITH posted AS (
 SELECT s.invoice_id,sum(CASE WHEN s.effect IN ('collection','refund') THEN -s.amount_minor::numeric ELSE s.amount_minor::numeric END) balance
 FROM archava_accounting.invoice_sources s
 JOIN archava_accounting.journal_sources x ON x.tenant_id=s.tenant_id AND x.source_id=s.id
 JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND j.state='posted' AND j.entry_date<=$3::text::date
 GROUP BY s.invoice_id
)
SELECT jsonb_build_object('documentId',s.invoice_id,'sourceId',s.id,'code',s.invoice_code,'sourceDate',s.source_date,'balanceMinor',p.balance::text,
 'terms',(SELECT archava_accounting.terms_snapshot(t)||jsonb_build_object('recordedAt',t.recorded_at) FROM archava_accounting.document_terms t
 WHERE t.tenant_id=s.tenant_id AND t.book_id=s.book_id AND t.document_kind='receivable' AND t.document_id=s.invoice_id AND t.effective_from<=$3::text::date ORDER BY t.revision DESC LIMIT 1))
FROM archava_accounting.invoice_sources s JOIN posted p ON p.invoice_id=s.invoice_id
WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect='issue' AND s.source_date<=$3::text::date ORDER BY s.invoice_id
"#;
const PAYABLE_POSITIONS: &str = r#"
WITH events AS (
 SELECT CASE WHEN s.effect='bill' THEN s.id ELSE (s.source_snapshot->'priorMatchedVersion'->>'request_id')::uuid END source_id,s.goods_minor::numeric amount
 FROM archava_accounting.purchase_sources s
 JOIN archava_accounting.purchase_journal_sources x ON x.tenant_id=s.tenant_id AND x.source_id=s.id
 JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect IN ('bill','void') AND j.state='posted' AND j.entry_date<=$3::text::date
 UNION ALL
 SELECT s.source_id,CASE WHEN s.effect='payment' THEN -s.amount_minor::numeric ELSE s.amount_minor::numeric END
 FROM archava_accounting.payable_settlements s JOIN archava_accounting.journals j ON j.tenant_id=s.tenant_id AND j.id=s.id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND j.state='posted' AND j.entry_date<=$3::text::date
), positions AS (SELECT source_id,sum(amount) balance FROM events GROUP BY source_id)
SELECT jsonb_build_object('documentId',s.document_id,'sourceId',s.id,'code','Supplier bill · '||s.document_id::text,'sourceDate',s.source_date,'balanceMinor',p.balance::text,
 'terms',(SELECT archava_accounting.terms_snapshot(t)||jsonb_build_object('recordedAt',t.recorded_at) FROM archava_accounting.document_terms t
 WHERE t.tenant_id=s.tenant_id AND t.book_id=s.book_id AND t.document_kind='payable' AND t.document_id=s.document_id AND t.effective_from<=$3::text::date ORDER BY t.revision DESC LIMIT 1))
FROM archava_accounting.purchase_sources s JOIN positions p ON p.source_id=s.id
WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect='bill' AND s.source_date<=$3::text::date ORDER BY s.document_id
"#;
// Include every historically captured role account and the currently declared
// role, including an inactive old account. Generic entries remain differences.
const RECEIVABLE_CONTROLS: &str = r#"
WITH mappings AS (
 SELECT s.id source_id,(v->>'accountId')::uuid account_id FROM archava_accounting.invoice_sources s CROSS JOIN LATERAL jsonb_array_elements(s.policy_snapshot->'mappings') v
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND v->>'role'='receivable'
), known AS (
 SELECT account_id FROM mappings UNION
 SELECT (v->>'accountId')::uuid FROM archava_accounting.invoice_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v WHERE p.tenant_id=$1 AND p.book_id=$2 AND v->>'role'='receivable'
), expected AS (
 SELECT m.account_id,sum(CASE WHEN s.effect IN ('collection','refund') THEN -s.amount_minor::numeric ELSE s.amount_minor::numeric END) balance
 FROM mappings m JOIN archava_accounting.invoice_sources s ON s.id=m.source_id AND s.tenant_id=$1 AND s.book_id=$2
 JOIN archava_accounting.journal_sources x ON x.tenant_id=s.tenant_id AND x.source_id=s.id
 JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id
 WHERE j.state='posted' AND j.entry_date<=$3::text::date GROUP BY m.account_id
), actual AS (
 SELECT l.account_id,sum(l.debit_minor::numeric-l.credit_minor::numeric) balance FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id
 WHERE j.tenant_id=$1 AND j.book_id=$2 AND j.state='posted' AND j.entry_date<=$3::text::date GROUP BY l.account_id
)
SELECT jsonb_build_object('accountId',a.id,'code',a.code,'name',a.name,'expectedMinor',coalesce(e.balance,0)::text,'actualMinor',coalesce(g.balance,0)::text,'differenceMinor',(coalesce(g.balance,0)-coalesce(e.balance,0))::text)
FROM known k JOIN archava_accounting.accounts a ON a.tenant_id=$1 AND a.book_id=$2 AND a.id=k.account_id LEFT JOIN expected e ON e.account_id=a.id LEFT JOIN actual g ON g.account_id=a.id ORDER BY a.id
"#;
const PAYABLE_CONTROLS: &str = r#"
WITH mappings AS (
 SELECT o.id origin_id,(v->>'accountId')::uuid account_id FROM archava_accounting.purchase_origins o CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') v
 WHERE o.tenant_id=$1 AND o.book_id=$2 AND v->>'role'='payable'
), known AS (
 SELECT account_id FROM mappings UNION
 SELECT (v->>'accountId')::uuid FROM archava_accounting.purchase_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v WHERE p.tenant_id=$1 AND p.book_id=$2 AND v->>'role'='payable' UNION
 SELECT payable_account_id FROM archava_accounting.payable_settlements WHERE tenant_id=$1 AND book_id=$2
), events AS (
 SELECT m.account_id,s.goods_minor::numeric amount FROM mappings m JOIN archava_accounting.purchase_sources s ON s.origin_id=m.origin_id AND s.tenant_id=$1 AND s.book_id=$2
 JOIN archava_accounting.purchase_journal_sources x ON x.tenant_id=s.tenant_id AND x.source_id=s.id
 JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id
 WHERE s.effect IN ('bill','void') AND j.state='posted' AND j.entry_date<=$3::text::date
 UNION ALL
 SELECT s.payable_account_id,CASE WHEN s.effect='payment' THEN -s.amount_minor::numeric ELSE s.amount_minor::numeric END FROM archava_accounting.payable_settlements s JOIN archava_accounting.journals j ON j.tenant_id=s.tenant_id AND j.id=s.id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND j.state='posted' AND j.entry_date<=$3::text::date
), expected AS (SELECT account_id,sum(amount) balance FROM events GROUP BY account_id), actual AS (
 SELECT l.account_id,sum(l.credit_minor::numeric-l.debit_minor::numeric) balance FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id
 WHERE j.tenant_id=$1 AND j.book_id=$2 AND j.state='posted' AND j.entry_date<=$3::text::date GROUP BY l.account_id
)
SELECT jsonb_build_object('accountId',a.id,'code',a.code,'name',a.name,'expectedMinor',coalesce(e.balance,0)::text,'actualMinor',coalesce(g.balance,0)::text,'differenceMinor',(coalesce(g.balance,0)-coalesce(e.balance,0))::text)
FROM known k JOIN archava_accounting.accounts a ON a.tenant_id=$1 AND a.book_id=$2 AND a.id=k.account_id LEFT JOIN expected e ON e.account_id=a.id LEFT JOIN actual g ON g.account_id=a.id ORDER BY a.id
"#;
const RECEIVABLE_PENDING: &str = "SELECT count(*) FROM archava_accounting.invoice_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.source_date<=$3::text::date AND s.amount_minor<>0 AND NOT EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted' AND j.entry_date<=$3::text::date)";
const PAYABLE_PENDING: &str = "SELECT count(*) FROM archava_accounting.purchase_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.source_date<=$3::text::date AND s.effect IN ('bill','void') AND s.goods_minor<>0 AND NOT EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted' AND j.entry_date<=$3::text::date)";

pub async fn subledger_report(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    a: &OrgAccess,
    key: Uuid,
    page: SubledgerPage<'_>,
) -> Result<Value, PurchaseError> {
    require(a, "reports.read")?;
    require(a, "terms.read")?;
    if matches!(page.side, SubledgerSide::Payable) {
        require(a, "payables.read")?;
    }
    if !page.valid() {
        return Err(ErpError::Conflict.into());
    }
    let b = book(conn, tenant, p, a, key, false).await?;
    if b.source != "native" {
        return Err(ErpError::Conflict.into());
    }
    let header:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'companyId',company_id,'currency',currency,'minorUnits',minor_units,'code',code,'name',name) FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(key).fetch_one(&mut *conn).await.map_err(db)?;
    let (positions_sql, controls_sql) = match page.side {
        SubledgerSide::Receivable => (RECEIVABLE_POSITIONS, RECEIVABLE_CONTROLS),
        SubledgerSide::Payable => (PAYABLE_POSITIONS, PAYABLE_CONTROLS),
    };
    let mut rows: Vec<Value> = sqlx::query_scalar(positions_sql)
        .bind(tenant)
        .bind(key)
        .bind(page.as_of)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)?;
    let inputs = rows
        .iter()
        .map(|r| {
            Ok(aging::Position {
                document_id: r["documentId"].as_str().ok_or(ErpError::Conflict)?.into(),
                source_date: r["sourceDate"].as_str().ok_or(ErpError::Conflict)?.into(),
                due_date: r["terms"]["condition"]["date"].as_str().map(str::to_owned),
                balance_minor: r["balanceMinor"].as_str().ok_or(ErpError::Conflict)?.into(),
            })
        })
        .collect::<Result<Vec<_>, PurchaseError>>()?;
    let aged = aging::report(page.as_of, &inputs).map_err(|_| ErpError::Conflict)?;
    for (row, position) in rows.iter_mut().zip(&aged.positions) {
        row["bucket"] = json!(position.bucket);
        row["daysPastDue"] = json!(position.days_past_due);
    }
    let count = rows.len();
    let controls: Vec<Value> = sqlx::query_scalar(controls_sql)
        .bind(tenant)
        .bind(key)
        .bind(page.as_of)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)?;
    let expected = controls.iter().try_fold(0_i128, |sum, row| {
        let amount = row["expectedMinor"]
            .as_str()
            .and_then(|v| v.parse::<i128>().ok())
            .ok_or(ErpError::Conflict)?;
        sum.checked_add(amount).ok_or(ErpError::Conflict)
    })?;
    let position_total = aged
        .totals
        .net_outstanding_minor
        .parse::<i128>()
        .map_err(|_| ErpError::Conflict)?;
    let position_difference = expected
        .checked_sub(position_total)
        .ok_or(ErpError::Conflict)?;
    let agree = position_difference == 0 && controls.iter().all(|r| r["differenceMinor"] == "0");
    let pending_sql = match page.side {
        SubledgerSide::Receivable => RECEIVABLE_PENDING,
        SubledgerSide::Payable => PAYABLE_PENDING,
    };
    let pending: i64 = sqlx::query_scalar(pending_sql)
        .bind(tenant)
        .bind(key)
        .bind(page.as_of)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    let cursor = page.cursor.map(|id| id.to_string());
    rows.retain(|row| {
        cursor
            .as_ref()
            .is_none_or(|c| row["documentId"].as_str().is_some_and(|id| id > c.as_str()))
    });
    let more = rows.len() > page.limit;
    rows.truncate(page.limit);
    let next = if more {
        rows.last()
            .map(|r| r["documentId"].clone())
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok(
        json!({"book":header,"side":page.side.name(),"asOf":page.as_of,"observedAt":null,"positions":rows,"totals":aged.totals,"controls":controls,"postedSourceControlsAgree":agree,"positionControlDifferenceMinor":position_difference.to_string(),
        "coverage":{"wholeAuthorizedBookTotals":true,"qualifyingDocumentCount":count,"draftsIncluded":false,"includesOnlyPostedEffectsThroughCutoff":true,"pendingMonetarySourceCount":pending,"legacyCoverage":"future_capture_only","termsKnowledge":"current_immutable_history_selected_by_business_effective_date"},
        "page":{"limit":page.limit,"hasMore":more,"nextCursor":next},
        "notice":"Totals cover every qualifying original document in this book. Details are paged. Unknown due dates stay separate; credits are not netted into overdue buckets. Controls compare independent original-source amounts to posted general-ledger balances, including historical and currently declared control accounts. Generic openings and adjustments remain differences. This does not certify legacy imports, statutory accounts or complete financial close."}),
    )
}
