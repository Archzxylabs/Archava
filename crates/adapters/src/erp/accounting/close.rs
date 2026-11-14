//! What still stands between an open period and its close.
//!
//! Closing is decided by the period's database guards, which refuse with one
//! message at a time. This read states every condition those guards test, as
//! counts, so a reviewer can see the whole list before submitting
//! `period.close`. It writes nothing and is not a second authority: the guards
//! still decide, and a condition added there must be added here.
use super::*;

/// One row per period: the period, a count for each close guard, and the bank
/// policies in range with whether their check is ready and freshly reviewed.
const READINESS: &str = "SELECT jsonb_build_object(\
 'period',jsonb_build_object('id',r.id,'bookId',r.book_id,'code',r.code,'state',r.state,'revision',r.revision,'startDate',r.start_date::text,'endDate',r.end_date::text),\
 'draft_journals',(SELECT count(*) FROM archava_accounting.journals j WHERE j.tenant_id=r.tenant_id AND j.period_id=r.id AND j.state='draft'),\
 'unposted_invoice_sources',(SELECT count(*) FROM archava_accounting.invoice_sources s WHERE s.tenant_id=r.tenant_id AND s.book_id=r.book_id AND s.amount_minor<>0 AND s.source_date BETWEEN r.start_date AND r.end_date AND NOT EXISTS(\
  SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')),\
 'unposted_purchase_sources',(SELECT count(*) FROM archava_accounting.purchase_sources s WHERE s.tenant_id=r.tenant_id AND s.book_id=r.book_id AND (s.goods_minor<>0 OR s.landed_fee_minor<>0) AND s.source_date BETWEEN r.start_date AND r.end_date AND NOT EXISTS(\
  SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')),\
 'unposted_stock_sources',(SELECT count(*) FROM archava_accounting.stock_sources s WHERE s.tenant_id=r.tenant_id AND s.book_id=r.book_id AND s.amount_minor<>0 AND s.source_date BETWEEN r.start_date AND r.end_date AND NOT EXISTS(\
  SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')),\
 'unmatched_bank_rows',(SELECT count(*) FROM archava_accounting.bank_rows x WHERE x.tenant_id=r.tenant_id AND x.book_id=r.book_id AND x.entry_date BETWEEN r.start_date AND r.end_date AND archava_accounting.bank_row_remaining(x.tenant_id,x.id)<>0),\
 'banks_without_policy',(SELECT count(*) FROM archava_accounting.bank_accounts b WHERE b.tenant_id=r.tenant_id AND b.book_id=r.book_id AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_policies x WHERE x.tenant_id=b.tenant_id AND x.bank_account_id=b.id)),\
 'bankReviews',(SELECT coalesce(jsonb_agg(jsonb_build_object('bankPolicyId',p.id,'bankAccountId',p.bank_account_id,\
  'checkReady',coalesce(archava_accounting.bank_period_check(p.tenant_id,p.id,r.id)->>'ready','')='true',\
  'reviewed',EXISTS(SELECT 1 FROM archava_accounting.bank_reconciliations x WHERE x.tenant_id=p.tenant_id AND x.bank_policy_id=p.id AND x.period_id=r.id AND x.policy_revision=p.revision AND x.reviewed_period_revision=CASE WHEN r.state='closed' THEN r.revision-1 ELSE r.revision END AND x.check_snapshot=archava_accounting.bank_period_check(p.tenant_id,p.id,r.id))) ORDER BY p.id),'[]'::jsonb)\
  FROM archava_accounting.bank_policies p WHERE p.tenant_id=r.tenant_id AND p.book_id=r.book_id AND p.from_date<=r.end_date)\
 ) FROM archava_accounting.periods r WHERE r.tenant_id=$1 AND r.id=$2";

/// The counted guards, in the order the report lists them, with the sentence
/// shown for each.
const GUARDS: &[(&str, &str)] = &[
    (
        "draft_journals",
        "Post or void the draft journals dated in this period.",
    ),
    (
        "unposted_invoice_sources",
        "Post the invoice sources dated in this period.",
    ),
    (
        "unposted_purchase_sources",
        "Post the receipt, supplier claim and correction sources dated in this period.",
    ),
    (
        "unposted_stock_sources",
        "Post the stock cost sources dated in this period.",
    ),
    (
        "unmatched_bank_rows",
        "Match the imported statement amounts dated in this period.",
    ),
    (
        "banks_without_policy",
        "Set a reconciliation opening basis for every registered bank.",
    ),
];

/// The close conditions of one period of a native book, for a principal who
/// may read the book's reports.
pub async fn close_readiness(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    period: Uuid,
) -> Result<Value, PurchaseError> {
    require(access, "reports.read")?;
    let key = book_for(conn, tenant, "period", period).await?;
    let book = book(conn, tenant, principal, access, key, false).await?;
    if book.source != SourceAuthority::Native.as_str() {
        return Err(ErpError::Conflict.into());
    }
    let counted: Option<Value> = sqlx::query_scalar(READINESS)
        .bind(tenant)
        .bind(period)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db)?;
    let counted = counted.ok_or(ErpError::NotFound)?;
    let mut blockers: Vec<Value> = GUARDS
        .iter()
        .filter_map(|(reason, message)| {
            let count = counted[*reason].as_i64().unwrap_or(0);
            (count > 0).then(|| json!({"reason": reason, "count": count, "message": message}))
        })
        .collect();
    let reviews = counted["bankReviews"].clone();
    let unreviewed = reviews
        .as_array()
        .into_iter()
        .flatten()
        .filter(|review| review["checkReady"] != true || review["reviewed"] != true)
        .count();
    if unreviewed > 0 {
        blockers.push(json!({
            "reason": "bank_reviews_incomplete",
            "count": unreviewed,
            "message": "Complete bank coverage, cash controls and a fresh reviewed reconciliation for every bank.",
        }));
    }
    let open = counted["period"]["state"] == "open";
    let ready = open && blockers.is_empty();
    Ok(json!({
        "period": counted["period"],
        "ready": ready,
        "canClose": ready && access.holds(&["periods.close"]),
        "blockers": blockers,
        "bankReviews": reviews,
    }))
}
