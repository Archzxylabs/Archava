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

// --- The documents behind the report ---------------------------------------

/// The sentence of the listing and the first line of its export.
pub const DOCUMENTS_NOTICE: &str = "Draft export of data your company entered in Archava. It is not a tax return and is not in any tax authority's import format. Archava does not verify tax codes, rates, tax ids or tax invoice references.";

/// The most documents one export holds. A longer period is exported in parts.
pub const DOCUMENTS_EXPORT_MAX: i64 = 20_000;

// One row per posted taxed source dated in the period: the same sources the
// report adds up, with the document each belongs to. An invoice source names
// its invoice, the customer and the tax ids the invoice stored at creation,
// and the tax invoice reference the invoice carries now. A bill source names
// the matched version of its supplier document (a void, the version it
// reverses). The invoice columns of Billing 011 are read through jsonb so a
// ledger that predates them reads "none".
const DOCUMENTS: &str = r#"
WITH docs AS (
 SELECT s.id,s.source_date,'output'::text side,s.effect,'invoice'::text document_kind,s.invoice_id document_id,s.invoice_code document_code,
  i.customer_ref counterparty_name,to_jsonb(i)->>'buyer_tax_id' counterparty_tax_id,to_jsonb(i)->>'seller_tax_id' company_tax_id,
  i.tax_code_id code_id,s.amount_minor::numeric-s.tax_minor base,s.tax_minor::numeric tax,
  to_jsonb(i)->>'tax_reference' tax_reference,to_jsonb(i)->>'tax_transaction_code' tax_transaction_code
 FROM archava_accounting.invoice_sources s JOIN archava_billing.invoices i ON i.tenant_id=s.tenant_id AND i.id=s.invoice_id
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.tax_account_id IS NOT NULL AND s.source_date BETWEEN $3::text::date AND $4::text::date
  AND EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')
 UNION ALL
 SELECT s.id,s.source_date,'input',s.effect,'supplier_document',s.document_id,v.snapshot->>'vendor_reference',
  v.snapshot->>'supplier_name',NULL,NULL,
  (v.snapshot->>'tax_code_id')::uuid,s.goods_minor::numeric,s.tax_minor::numeric,NULL,NULL
 FROM archava_accounting.purchase_sources s JOIN archava_purchasing.supplier_document_versions v ON v.tenant_id=s.tenant_id
  AND v.request_id=CASE WHEN s.effect='bill' THEN s.id ELSE (s.source_snapshot->'priorMatchedVersion'->>'request_id')::uuid END
 WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.effect IN ('bill','void') AND s.tax_account_id IS NOT NULL AND s.source_date BETWEEN $3::text::date AND $4::text::date
  AND EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')
)
SELECT jsonb_build_object('sourceId',d.id,'date',d.source_date,'side',d.side,'effect',d.effect,'documentKind',d.document_kind,'documentId',d.document_id,
 'documentCode',d.document_code,'counterpartyName',d.counterparty_name,'counterpartyTaxId',d.counterparty_tax_id,'companyTaxId',d.company_tax_id,
 'taxCode',c.code,'taxCodeVersion',c.version,
 'rate',jsonb_build_object('numerator',c.rate_numerator,'denominator',c.rate_denominator),
 'base',jsonb_build_object('numerator',c.base_numerator,'denominator',c.base_denominator),
 'taxableBaseMinor',d.base::text,'taxMinor',d.tax::text,'taxReference',d.tax_reference,'taxTransactionCode',d.tax_transaction_code)
FROM docs d JOIN archava_accounting.tax_codes c ON c.tenant_id=$1 AND c.book_id=$2 AND c.id=d.code_id
WHERE ($5::uuid IS NULL OR (d.source_date,d.id)>(SELECT x.source_date,x.id FROM docs x WHERE x.id=$5))
ORDER BY d.source_date,d.id LIMIT $6
"#;

/// A page of the documents listing: the period, an optional cursor (the
/// `sourceId` of the last row of the page before) and a page size.
pub struct TaxDocumentsPage<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub cursor: Option<Uuid>,
    pub limit: i64,
}
impl TaxDocumentsPage<'_> {
    pub fn valid(&self) -> bool {
        let date = archava_domain::erp::accommodation::parse_calendar_date;
        date(self.from).is_some()
            && date(self.to).is_some()
            && self.from <= self.to
            && (1..=100).contains(&self.limit)
    }
}

/// The book header and the rows of the period after `cursor`, at most
/// `limit` of them. The grants and the book rules are the report's.
async fn documents(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    key: Uuid,
    page: &TaxDocumentsPage<'_>,
    limit: i64,
) -> Result<(Value, Vec<Value>), PurchaseError> {
    require(access, "tax.read")?;
    require(access, "reports.read")?;
    let date = archava_domain::erp::accommodation::parse_calendar_date;
    if date(page.from).is_none() || date(page.to).is_none() || page.from > page.to {
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
    let rows: Vec<Value> = sqlx::query_scalar(DOCUMENTS)
        .bind(tenant)
        .bind(key)
        .bind(page.from)
        .bind(page.to)
        .bind(page.cursor)
        .bind(limit)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)?;
    Ok((header, rows))
}

/// The posted taxed sources of a period, one row per source with its
/// document, paged in the order of source date and source id.
pub async fn tax_report_documents(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    key: Uuid,
    page: TaxDocumentsPage<'_>,
) -> Result<Value, PurchaseError> {
    if !page.valid() {
        return Err(ErpError::Conflict.into());
    }
    let (header, mut rows) =
        documents(conn, tenant, principal, access, key, &page, page.limit + 1).await?;
    let more = rows.len() as i64 > page.limit;
    rows.truncate(page.limit as usize);
    let next = if more {
        rows.last()
            .map(|row| row["sourceId"].clone())
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok(json!({
        "book": header,
        "from": page.from,
        "to": page.to,
        "observedAt": null,
        "periodDate": "source_date",
        "documents": rows,
        "page": {"limit": page.limit, "hasMore": more, "nextCursor": next},
        "enteredByCompany": true,
        "verifiedByArchava": false,
        "notice": DOCUMENTS_NOTICE,
    }))
}

/// A CSV export of the documents listing.
pub struct TaxDocumentsExport {
    /// A file name made of the book code and the period.
    pub filename: String,
    /// The export, UTF-8 with CRLF line ends (RFC 4180).
    pub body: String,
}

/// One CSV field (RFC 4180): quoted when it holds a comma, a quote or a line
/// break, with quotes doubled.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
/// A text cell a person typed. A spreadsheet reads a cell that starts with
/// `=`, `+`, `-` or `@` as a formula, so such text gets a leading apostrophe.
fn csv_text(value: &Value) -> String {
    let text = value.as_str().unwrap_or_default();
    if text.starts_with(['=', '+', '-', '@']) {
        csv_field(&format!("'{text}"))
    } else {
        csv_field(text)
    }
}
/// Ledger minor units as an exact decimal amount of the book's currency
/// (`1100000` with two minor units is `11000.00`); never through a float.
fn csv_amount(minor: &Value, minor_units: u32) -> Result<String, PurchaseError> {
    let value = amount(minor)?;
    let scale = 10_i128.checked_pow(minor_units).ok_or(ErpError::Conflict)?;
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let scale = scale.unsigned_abs();
    Ok(if minor_units == 0 {
        format!("{sign}{magnitude}")
    } else {
        format!(
            "{sign}{}.{:0width$}",
            magnitude / scale,
            magnitude % scale,
            width = minor_units as usize
        )
    })
}
fn csv_fraction(value: &Value) -> String {
    format!("{}/{}", value["numerator"], value["denominator"])
}

const CSV_HEADER: [&str; 17] = [
    "Side",
    "Document type",
    "Effect",
    "Document code",
    "Date",
    "Counterparty",
    "Counterparty tax id",
    "Company tax id",
    "Tax code",
    "Tax code version",
    "Rate",
    "Base",
    "Taxable base",
    "Tax",
    "Currency",
    "Tax invoice reference",
    "Transaction code",
];

/// Every row of the documents listing for the period as CSV. The first line
/// says what the file is and is not; the second names the columns. Amounts are
/// exact decimals of the book's currency. A period with more than
/// [`DOCUMENTS_EXPORT_MAX`] documents is refused: export it in parts.
pub async fn tax_report_documents_csv(
    conn: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &OrgAccess,
    key: Uuid,
    from: &str,
    to: &str,
) -> Result<TaxDocumentsExport, PurchaseError> {
    let page = TaxDocumentsPage {
        from,
        to,
        cursor: None,
        limit: 1,
    };
    let (header, rows) = documents(
        conn,
        tenant,
        principal,
        access,
        key,
        &page,
        DOCUMENTS_EXPORT_MAX + 1,
    )
    .await?;
    if rows.len() as i64 > DOCUMENTS_EXPORT_MAX {
        return Err(ErpError::Conflict.into());
    }
    let minor_units = header["minorUnits"]
        .as_u64()
        .and_then(|units| u32::try_from(units).ok())
        .filter(|units| *units <= 6)
        .ok_or(ErpError::Conflict)?;
    let currency = header["currency"].as_str().unwrap_or_default();
    let mut body = String::new();
    body.push_str(&csv_field(DOCUMENTS_NOTICE));
    body.push_str("\r\n");
    body.push_str(&CSV_HEADER.join(","));
    body.push_str("\r\n");
    for row in &rows {
        let cells = [
            csv_text(&row["side"]),
            csv_text(&row["documentKind"]),
            csv_text(&row["effect"]),
            csv_text(&row["documentCode"]),
            csv_text(&row["date"]),
            csv_text(&row["counterpartyName"]),
            csv_text(&row["counterpartyTaxId"]),
            csv_text(&row["companyTaxId"]),
            csv_text(&row["taxCode"]),
            row["taxCodeVersion"].to_string(),
            csv_fraction(&row["rate"]),
            csv_fraction(&row["base"]),
            csv_amount(&row["taxableBaseMinor"], minor_units)?,
            csv_amount(&row["taxMinor"], minor_units)?,
            csv_field(currency),
            csv_text(&row["taxReference"]),
            csv_text(&row["taxTransactionCode"]),
        ];
        body.push_str(&cells.join(","));
        body.push_str("\r\n");
    }
    let code: String = header["code"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(TaxDocumentsExport {
        filename: format!("tax-documents-{code}-{from}-to-{to}.csv"),
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_only_when_needed_and_quotes_are_doubled() {
        assert_eq!(csv_field("PT Maju"), "PT Maju");
        assert_eq!(csv_field("Maju, PT"), "\"Maju, PT\"");
        assert_eq!(csv_field("The \"Best\" Shop"), "\"The \"\"Best\"\" Shop\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_field(""), "");
        assert_eq!(csv_text(&Value::Null), "");
        assert_eq!(csv_text(&json!("=SUM(A1)")), "'=SUM(A1)");
        assert_eq!(csv_text(&json!("-1,5")), "\"'-1,5\"");
        assert_eq!(csv_text(&json!("@home")), "'@home");
        assert_eq!(csv_text(&json!("01.234")), "01.234");
    }

    #[test]
    fn amounts_are_exact_decimals_of_the_book_currency() {
        assert_eq!(csv_amount(&json!("1100000"), 2).unwrap(), "11000.00");
        assert_eq!(csv_amount(&json!("-5"), 2).unwrap(), "-0.05");
        assert_eq!(csv_amount(&json!("0"), 2).unwrap(), "0.00");
        assert_eq!(csv_amount(&json!("1500"), 0).unwrap(), "1500");
        assert_eq!(csv_amount(&json!("-1250"), 3).unwrap(), "-1.250");
        assert_eq!(
            csv_amount(&json!("9000000000000000"), 2).unwrap(),
            "90000000000000.00"
        );
        assert!(csv_amount(&json!(11), 2).is_err());
        assert_eq!(
            csv_fraction(&json!({"numerator":11,"denominator":12})),
            "11/12"
        );
        assert!(DOCUMENTS_NOTICE.contains("not a tax return"));
        assert!(DOCUMENTS_NOTICE.contains("not in any tax authority's import format"));
        assert_eq!(CSV_HEADER.len(), 17);
    }
}
