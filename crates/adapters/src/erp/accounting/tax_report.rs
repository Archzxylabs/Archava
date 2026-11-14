//! The tax report of one native book for a period (TAX.md rule 9): what posted
//! sources say the output and input tax is, per tax code version, beside what
//! the ledger holds on each tax control account in the same period.
//!
//! A source is a captured invoice effect (issue, correction, void) or a
//! captured supplier bill or its void that carries a tax control account. It
//! counts once its journal is posted. A source journal is dated on the source
//! date and cannot be reversed, so the period is the source date and the
//! report ties to the ledger for the same range. The control movement is every
//! posted line on the account in the period, so a manual journal on a tax
//! account is a difference. Captured sources without a posted journal are
//! counted as pending; taxed documents that were never captured are counted
//! as outside the ledger. Neither is part of the totals. Nothing is written.
use super::*;

/// The standing sentence of the report (TAX.md rule 1): no compliance claim.
const NOTICE: &str = "Tax codes are entered by your company; Archava does not verify tax rates or rules. This report is not a tax return. It adds up the tax of posted invoice and supplier bill sources by the date they were posted in and compares it with the posted movement of each tax control account in the same period. A manual journal on a tax account appears as a difference. Captured sources that are not posted yet and taxed documents that were never captured for posting are listed separately and are not in the totals.";

// `taxed` is every captured source of the book that carries a tax control
// account and is dated in the period. The tax code version of an invoice
// source is the one stored on its invoice; of a bill, the one stored on the
// matched document version; of a void, the one of the bill it reverses.
const REPORT: &str = r#"
WITH taxed AS (
 SELECT 'output'::text side,i.tax_code_id code_id,s.tax_account_id account_id,s.amount_minor::numeric-s.tax_minor base,s.tax_minor::numeric tax,s.amount_minor<>0 monetary,
  EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') posted
 FROM archava_accounting.invoice_sources s JOIN archava_billing.invoices i ON i.tenant_id=s.tenant_id AND i.id=s.invoice_id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.tax_account_id IS NOT NULL AND s.source_date BETWEEN $3::text::date AND $4::text::date
 UNION ALL
 SELECT 'input',(v.snapshot->>'tax_code_id')::uuid,s.tax_account_id,s.goods_minor::numeric,s.tax_minor::numeric,s.goods_minor<>0,
  EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')
 FROM archava_accounting.purchase_sources s JOIN archava_purchasing.supplier_document_versions v ON v.tenant_id=s.tenant_id
  AND v.request_id=CASE WHEN s.effect='bill' THEN s.id ELSE (s.source_snapshot->'priorMatchedVersion'->>'request_id')::uuid END
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect IN ('bill','void') AND s.tax_account_id IS NOT NULL AND s.source_date BETWEEN $3::text::date AND $4::text::date
), codes AS (
 SELECT c.kind,c.code,c.version,jsonb_build_object('taxCodeId',c.id,'code',c.code,'version',c.version,'kind',c.kind,
  'rate',jsonb_build_object('numerator',c.rate_numerator,'denominator',c.rate_denominator),
  'base',jsonb_build_object('numerator',c.base_numerator,'denominator',c.base_denominator),
  'sourceCount',count(*),'taxableBaseMinor',sum(t.base)::text,'taxMinor',sum(t.tax)::text) entry
 FROM taxed t JOIN archava_accounting.tax_codes c ON c.tenant_id=$1 AND c.book_id=$2 AND c.id=t.code_id
 WHERE t.posted GROUP BY c.tenant_id,c.id
), attributed AS (
 SELECT account_id,sum(tax) tax FROM taxed WHERE posted GROUP BY account_id
), known AS (
 SELECT account_id FROM taxed UNION
 SELECT control_account_id FROM archava_accounting.tax_codes WHERE tenant_id=$1 AND book_id=$2 AND state IN ('confirmed','retired')
), movement AS (
 SELECT l.account_id,sum(l.debit_minor::numeric-l.credit_minor::numeric) net FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id
 WHERE j.tenant_id=$1 AND j.book_id=$2 AND j.state='posted' AND j.entry_date BETWEEN $3::text::date AND $4::text::date GROUP BY l.account_id
), controls AS (
 SELECT a.code,a.id,jsonb_build_object('accountId',a.id,'code',a.code,'name',a.name,'class',a.class,'kind',CASE WHEN a.class='liability' THEN 'output' ELSE 'input' END,
  'reportedTaxMinor',coalesce(r.tax,0)::text,
  'movementMinor',(CASE WHEN a.class='liability' THEN -coalesce(m.net,0) ELSE coalesce(m.net,0) END)::text,
  'differenceMinor',(CASE WHEN a.class='liability' THEN -coalesce(m.net,0) ELSE coalesce(m.net,0) END-coalesce(r.tax,0))::text) entry
 FROM known k JOIN archava_accounting.accounts a ON a.tenant_id=$1 AND a.book_id=$2 AND a.id=k.account_id
 LEFT JOIN attributed r ON r.account_id=a.id LEFT JOIN movement m ON m.account_id=a.id
), waiting AS (
 SELECT side,jsonb_build_object('sourceCount',count(*),'taxableBaseMinor',coalesce(sum(base),0)::text,'taxMinor',coalesce(sum(tax),0)::text) entry
 FROM taxed WHERE NOT posted AND monetary GROUP BY side
)
SELECT jsonb_build_object(
 'codes',coalesce((SELECT jsonb_agg(entry ORDER BY kind DESC,code,version) FROM codes),'[]'::jsonb),
 'outputTaxMinor',(SELECT coalesce(sum(tax),0)::text FROM taxed WHERE posted AND side='output'),
 'inputTaxMinor',(SELECT coalesce(sum(tax),0)::text FROM taxed WHERE posted AND side='input'),
 'controls',coalesce((SELECT jsonb_agg(entry ORDER BY code,id) FROM controls),'[]'::jsonb),
 'pendingOutput',(SELECT entry FROM waiting WHERE side='output'),
 'pendingInput',(SELECT entry FROM waiting WHERE side='input'))
"#;

// Taxed documents of the book that no bridge captured, by the UTC date they
// were issued or matched on (the calendar a source would have been dated in).
// An invoice counts while it is issued or settled, with the tax of its
// corrections; a supplier document counts while its current version is
// matched. Amounts are the documents' own stored integers, in the document
// unit: no posting policy captured them, so no policy states a unit factor.
const OUTSIDE: &str = r#"
SELECT jsonb_build_object(
 'invoices',(SELECT jsonb_build_object('count',count(*),'taxMinor',coalesce(sum(i.tax_minor::numeric+i.correction_tax_minor),0)::text)
  FROM archava_billing.invoices i JOIN archava_accounting.tax_codes c ON c.tenant_id=i.tenant_id AND c.id=i.tax_code_id
  WHERE i.tenant_id=$1 AND c.book_id=$2 AND i.status IN ('issued','settled') AND (i.issued_at AT TIME ZONE 'UTC')::date BETWEEN $3::text::date AND $4::text::date
   AND NOT EXISTS(SELECT 1 FROM archava_accounting.invoice_sources s WHERE s.tenant_id=i.tenant_id AND s.invoice_id=i.id AND s.effect='issue')),
 'supplierDocuments',(SELECT jsonb_build_object('count',count(*),'taxMinor',coalesce(sum(d.tax_minor::numeric),0)::text)
  FROM archava_purchasing.supplier_documents d JOIN archava_accounting.tax_codes c ON c.tenant_id=d.tenant_id AND c.id=d.tax_code_id
  WHERE d.tenant_id=$1 AND c.book_id=$2 AND d.state='matched' AND (d.updated_at AT TIME ZONE 'UTC')::date BETWEEN $3::text::date AND $4::text::date
   AND NOT EXISTS(SELECT 1 FROM archava_accounting.purchase_sources s JOIN archava_purchasing.supplier_document_versions v ON v.tenant_id=s.tenant_id AND v.request_id=s.id
    WHERE s.tenant_id=d.tenant_id AND s.effect='bill' AND v.document_id=d.id AND v.revision=d.revision)))
"#;

fn amount(v: &Value) -> Result<i128, PurchaseError> {
    v.as_str()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ErpError::Conflict.into())
}

/// The tax report of one native book for the source dates `from..=to`.
pub async fn tax_report(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    key: Uuid,
    from: &str,
    to: &str,
) -> Result<Value, PurchaseError> {
    require(access, "tax.read")?;
    require(access, "reports.read")?;
    let date = archava_domain::erp::accommodation::parse_calendar_date;
    if date(from).is_none() || date(to).is_none() || from > to {
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
    let report: Value = sqlx::query_scalar(REPORT)
        .bind(tenant)
        .bind(key)
        .bind(from)
        .bind(to)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    let mut outside: Value = sqlx::query_scalar(OUTSIDE)
        .bind(tenant)
        .bind(key)
        .bind(from)
        .bind(to)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    let output = amount(&report["outputTaxMinor"])?;
    let input = amount(&report["inputTaxMinor"])?;
    let difference = output.checked_sub(input).ok_or(ErpError::Conflict)?;
    let agree = report["controls"]
        .as_array()
        .is_some_and(|rows| rows.iter().all(|row| row["differenceMinor"] == "0"));
    let none = || json!({"sourceCount":0,"taxableBaseMinor":"0","taxMinor":"0"});
    let pending = |key: &str| {
        if report[key].is_null() {
            none()
        } else {
            report[key].clone()
        }
    };
    outside["unit"] = json!("document");
    outside["currency"] = header["currency"].clone();
    outside["unitNote"] = json!("Amounts of documents outside the ledger are in the unit the documents store: whole Rupiah for IDR, the currency's minor unit otherwise. They are not in ledger minor units.");
    Ok(json!({
        "book": header,
        "from": from,
        "to": to,
        "observedAt": null,
        "periodDate": "source_date",
        "codes": report["codes"],
        "totals": {
            "outputTaxMinor": output.to_string(),
            "inputTaxMinor": input.to_string(),
            "differenceMinor": difference.to_string(),
        },
        "controls": report["controls"],
        "postedSourceControlsAgree": agree,
        "pending": {"output": pending("pendingOutput"), "input": pending("pendingInput")},
        "outsideLedger": outside,
        "enteredByCompany": true,
        "verifiedByArchava": false,
        "notice": NOTICE,
    }))
}
