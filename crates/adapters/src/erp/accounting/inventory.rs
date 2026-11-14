//! Inventory control comparison: what posted stock and purchase sources say
//! each inventory account should hold, against what the ledger holds.
//!
//! The expected balance is built only from captured original sources whose
//! journals are posted through the cutoff: receipts add goods and landed fee,
//! fulfillments remove their layer cost, restocks return it. The ledger
//! A posted declared opening balance on the inventory account is added to the
//! expected balance. The ledger balance is every posted line on the same
//! account. A difference is therefore anything posted to an inventory account
//! that neither a captured source nor a declared opening explains, such as a
//! manual adjustment. Nothing is written.
use super::*;

const CONTROLS: &str = r#"
WITH known AS (
 SELECT inventory_account_id account_id FROM archava_accounting.stock_layer_origins WHERE tenant_id=$1 AND book_id=$2 UNION
 SELECT (v->>'accountId')::uuid FROM archava_accounting.purchase_origins o CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') v WHERE o.tenant_id=$1 AND o.book_id=$2 AND v->>'role'='inventory' UNION
 SELECT (v->>'accountId')::uuid FROM archava_accounting.purchase_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v WHERE p.tenant_id=$1 AND p.book_id=$2 AND v->>'role'='inventory' UNION
 SELECT control_account_id FROM archava_accounting.opening_balances WHERE tenant_id=$1 AND book_id=$2 AND control='inventory'
), opening AS (
 SELECT o.control_account_id account_id,sum(CASE WHEN j.id=o.id THEN o.amount_minor::numeric ELSE -o.amount_minor::numeric END) balance
 FROM archava_accounting.opening_balances o JOIN archava_accounting.journals j ON j.tenant_id=o.tenant_id AND (j.id=o.id OR j.reversal_of=o.id)
 WHERE o.tenant_id=$1 AND o.book_id=$2 AND o.control='inventory' AND j.state='posted' AND j.entry_date<=$3::text::date GROUP BY o.control_account_id
), events AS (
 SELECT (v->>'accountId')::uuid account_id,s.goods_minor::numeric+s.landed_fee_minor::numeric amount
 FROM archava_accounting.purchase_sources s JOIN archava_accounting.purchase_origins o ON o.tenant_id=s.tenant_id AND o.book_id=s.book_id AND o.id=s.origin_id
 CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') v
 JOIN archava_accounting.purchase_journal_sources x ON x.tenant_id=s.tenant_id AND x.source_id=s.id
 JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect='receipt' AND v->>'role'='inventory' AND j.state='posted' AND j.entry_date<=$3::text::date
 UNION ALL
 SELECT (g->>'inventoryAccountId')::uuid,CASE WHEN s.effect='fulfillment' THEN -(g->>'costMinor')::numeric ELSE (g->>'costMinor')::numeric END
 FROM archava_accounting.stock_sources s CROSS JOIN LATERAL jsonb_array_elements(s.segments) g
 JOIN archava_accounting.stock_journal_sources x ON x.tenant_id=s.tenant_id AND x.source_id=s.id
 JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect<>'discard_return' AND j.state='posted' AND j.entry_date<=$3::text::date
), expected AS (
 SELECT account_id,sum(amount) balance FROM events GROUP BY account_id
), actual AS (
 SELECT l.account_id,sum(l.debit_minor::numeric-l.credit_minor::numeric) balance FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id
 WHERE j.tenant_id=$1 AND j.book_id=$2 AND j.state='posted' AND j.entry_date<=$3::text::date GROUP BY l.account_id
)
SELECT jsonb_build_object('accountId',a.id,'code',a.code,'name',a.name,'expectedMinor',(coalesce(e.balance,0)+coalesce(o.balance,0))::text,'openingMinor',coalesce(o.balance,0)::text,'actualMinor',coalesce(g.balance,0)::text,'differenceMinor',(coalesce(g.balance,0)-coalesce(e.balance,0)-coalesce(o.balance,0))::text)
FROM known k JOIN archava_accounting.accounts a ON a.tenant_id=$1 AND a.book_id=$2 AND a.id=k.account_id LEFT JOIN expected e ON e.account_id=a.id LEFT JOIN opening o ON o.account_id=a.id LEFT JOIN actual g ON g.account_id=a.id ORDER BY a.code,a.id
"#;

/// Captured receipts and stock cost sources dated through the cutoff that
/// carry money and have no posted journal yet.
const PENDING: &str = "SELECT \
 (SELECT count(*) FROM archava_accounting.purchase_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect='receipt' AND s.source_date<=$3::text::date AND (s.goods_minor<>0 OR s.landed_fee_minor<>0) AND NOT EXISTS(\
  SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted' AND j.entry_date<=$3::text::date))\
 +(SELECT count(*) FROM archava_accounting.stock_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.source_date<=$3::text::date AND s.amount_minor<>0 AND NOT EXISTS(\
  SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted' AND j.entry_date<=$3::text::date))";

/// The inventory control comparison of one native book through `as_of`.
pub async fn inventory_control(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    key: Uuid,
    as_of: &str,
) -> Result<Value, PurchaseError> {
    require(access, "reports.read")?;
    if archava_domain::erp::accommodation::parse_calendar_date(as_of).is_none() {
        return Err(ErpError::Conflict.into());
    }
    let book = book(conn, tenant, principal, access, key, false).await?;
    if book.source != SourceAuthority::Native.as_str() {
        return Err(ErpError::Conflict.into());
    }
    let header: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'companyId',company_id,'currency',currency,'minorUnits',minor_units,'code',code,'name',name) FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2")
        .bind(tenant)
        .bind(key)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    let controls: Vec<Value> = sqlx::query_scalar(CONTROLS)
        .bind(tenant)
        .bind(key)
        .bind(as_of)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)?;
    let pending: i64 = sqlx::query_scalar(PENDING)
        .bind(tenant)
        .bind(key)
        .bind(as_of)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    let agree = controls.iter().all(|row| row["differenceMinor"] == "0");
    Ok(json!({
        "book": header,
        "asOf": as_of,
        "observedAt": null,
        "controls": controls,
        "postedSourceControlsAgree": agree,
        "coverage": {
            "includesOnlyPostedEffectsThroughCutoff": true,
            "pendingMonetarySourceCount": pending,
            "legacyCoverage": "future_capture_only",
        },
        "notice": "Expected balances come from posted receipt, fulfillment and restock sources and posted declared opening balances through this date. Counted adjustments and manual journals on an inventory account are not captured sources and appear as differences. This compares ledger control accounts with source postings; it does not value the stock on hand.",
    }))
}
