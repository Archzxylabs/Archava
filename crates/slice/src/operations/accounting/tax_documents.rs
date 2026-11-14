//! The documents behind the tax report of a period: one row per posted taxed
//! source with its document, the counterparty and the tax id the document
//! stored, the tax code version, the taxable amount, the tax and the tax
//! invoice reference when one was recorded. Paged, with a CSV download. It is
//! the company's own data: a draft export, not a tax return and not in any
//! authority's import format.
use super::*;
use archava_domain::erp::tax::Fraction;

fn fraction(value: &Value) -> Option<Fraction> {
    serde_json::from_value(value.clone()).ok()
}
/// The rate in exact digits, with the part of the price it applies to.
fn rule(row: &Value) -> String {
    let rate = fraction(&row["rate"]).map_or_else(|| "Unreadable rate".into(), Fraction::percent);
    match fraction(&row["base"]) {
        Some(base) if base.is_one() => rate,
        Some(base) => format!(
            "{rate} of {}/{} of the price",
            base.numerator, base.denominator
        ),
        None => format!("{rate} of an unreadable base"),
    }
}
/// What kind of source a row is, in the words of the screen.
fn source(row: &Value) -> &'static str {
    match (row["documentKind"].as_str(), row["effect"].as_str()) {
        (Some("invoice"), Some("issue")) => "Sales invoice",
        (Some("invoice"), Some("correction")) => "Sales invoice correction",
        (Some("invoice"), Some("void")) => "Sales invoice void",
        (Some("supplier_document"), Some("bill")) => "Supplier bill",
        (Some("supplier_document"), Some("void")) => "Supplier bill void",
        _ => "Unknown source",
    }
}
fn optional(row: &Value, key: &str) -> String {
    row[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map_or_else(|| "—".to_string(), str::to_owned)
}
fn reference(row: &Value) -> String {
    match (
        row["taxReference"].as_str(),
        row["taxTransactionCode"].as_str(),
    ) {
        (Some(reference), Some(code)) => format!("{reference} · code {code}"),
        (Some(reference), None) => reference.to_string(),
        _ => "—".to_string(),
    }
}
fn document_row(row: &Value, book: &Value) -> AnyView {
    view! {<tr data-testid="accounting-taxdocs-row">
        <td>{text(row,"date")}</td>
        <td>{source(row)}</td>
        <td>{text(row,"documentCode")}</td>
        <td>{text(row,"counterpartyName")}</td>
        <td data-testid="accounting-taxdocs-taxid">{optional(row,"counterpartyTaxId")}</td>
        <td>{format!("{} · version {} · {}",text(row,"taxCode"),row["taxCodeVersion"],rule(row))}</td>
        <td>{amount(&row["taxableBaseMinor"],book)}</td>
        <td>{amount(&row["taxMinor"],book)}</td>
        <td data-testid="accounting-taxdocs-reference">{reference(row)}</td>
    </tr>}
    .into_any()
}
fn table(rows: Vec<Value>, book: &Value) -> AnyView {
    if rows.is_empty() {
        return view! {<p class="hint" data-testid="accounting-taxdocs-empty">"No posted source carries tax in this period."</p>}.into_any();
    }
    let body = rows
        .iter()
        .map(|row| document_row(row, book))
        .collect_view();
    view! {<div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-taxdocs-table"><caption>"Posted taxed documents of the period, by posting date"</caption>
        <thead><tr><th>"Date"</th><th>"Source"</th><th>"Document"</th><th>"Counterparty"</th><th>"Counterparty tax id"</th><th>"Tax code"</th><th>"Taxable amount"</th><th>"Tax"</th><th>"Tax invoice reference"</th></tr></thead>
        <tbody>{body}</tbody></table></div>}
    .into_any()
}
/// One page of the listing as the server answered it.
fn listing(answer: &Value) -> AnyView {
    let book = answer["book"].clone();
    let rows = answer["documents"].as_array().cloned().unwrap_or_default();
    let more = answer["page"]["hasMore"] == true;
    view! {<div data-testid="accounting-taxdocs-result">
        <h4>{format!("Documents · {} · {} through {}",text(&book,"code"),text(answer,"from"),text(answer,"to"))}</h4>
        {table(rows,&book)}
        {more.then(||view!{<p class="hint" data-testid="accounting-taxdocs-more">"More documents follow. Use Next documents, or download the CSV for the whole period."</p>})}
        <p class="hint" data-testid="accounting-taxdocs-notice">{text(answer,"notice")}</p>
    </div>}
    .into_any()
}
/// The address of the CSV download for a book and a period.
fn csv_path(tenant: &str, book: &str, from: &str, to: &str) -> String {
    format!(
        "/erp/accounting/tax-report/documents.csv?tenant={}&bookId={book}&from={from}&to={to}",
        crate::contacts::query_encode(tenant)
    )
}
fn csv_link(path: String) -> AnyView {
    view! {<a class="btn secondary" data-testid="accounting-taxdocs-csv" href=path download>"Download CSV (draft export, not a tax return)"</a>}.into_any()
}

/// The documents listing under the tax report, for the dates of the report
/// form.
#[component]
pub(super) fn TaxDocuments(
    work: Workbench,
    from: RwSignal<String>,
    to: RwSignal<String>,
) -> impl IntoView {
    let answer = RwSignal::new(None::<Value>);
    let notice = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let sequence = RwSignal::new(0_u64);
    Effect::new(move |_| {
        work.data.track();
        from.track();
        to.track();
        answer.set_if_alive(None);
        notice.set_if_alive(String::new());
        sequence.update_if_alive(|v| *v += 1);
        busy.set_if_alive(false);
    });
    let period = move || {
        let book = text(&work.data.get()["configuredBook"], "id");
        let (start, end) = (from.get(), to.get());
        let date = archava_domain::erp::accommodation::parse_calendar_date;
        (!book.is_empty() && date(&start).is_some() && date(&end).is_some() && start <= end)
            .then_some((book, start, end))
    };
    let load = move |cursor: Option<String>| {
        let Some(held) = session::held() else { return };
        let Some((book, start, end)) = period() else {
            notice.set_if_alive("Select a company book and a valid date range.".into());
            return;
        };
        let company = work.company_key.get_untracked();
        sequence.update_if_alive(|v| *v += 1);
        let current = sequence.get_untracked();
        busy.set_if_alive(true);
        notice.set_if_alive(String::new());
        let mut path = format!(
            "/erp/accounting/tax-report/documents?tenant={}&bookId={book}&from={start}&to={end}&limit=100",
            crate::contacts::query_encode(&held.tenant)
        );
        if let Some(cursor) = cursor {
            path.push_str(&format!("&cursor={cursor}"));
        }
        spawn_local(async move {
            let response = session::api_fetch("GET", &path, None).await;
            if sequence.is_disposed()
                || sequence.get_untracked() != current
                || work.company_key.get_untracked() != company
            {
                return;
            }
            busy.set_if_alive(false);
            match response {
                Ok(v) if v.status == 200 => answer.set_if_alive(Some(v.body)),
                Ok(v) => {
                    answer.set_if_alive(None);
                    notice.set_if_alive(message(&v));
                }
                Err(_) => {
                    answer.set_if_alive(None);
                    notice.set_if_alive(
                        "The documents could not be loaded. Refresh and try again.".into(),
                    );
                }
            }
        });
    };
    let next = move || {
        answer.get().and_then(|a| {
            a["page"]["nextCursor"]
                .as_str()
                .filter(|_| a["page"]["hasMore"] == true)
                .map(str::to_owned)
        })
    };
    let allowed = move || work.permits("reports.read") && work.permits("tax.read");
    view! {<div data-testid="accounting-taxdocs">
        <p class="hint">"The documents behind the report: each posted taxed invoice source and supplier bill source of the period. The download is a draft export of data your company entered, not a tax return and not in any tax authority's import format."</p>
        <div class="ops-actions">
            <button type="button" class="btn secondary" data-testid="accounting-taxdocs-run" disabled=move||busy.get()||!allowed() on:click=move|_|load(None)>"List documents"</button>
            <button type="button" class="btn secondary" data-testid="accounting-taxdocs-next" disabled=move||busy.get()||next().is_none() on:click=move|_|load(next())>"Next documents"</button>
            {move||period().filter(|_|allowed()).zip(session::held()).map(|((book,start,end),held)|csv_link(csv_path(&held.tenant,&book,&start,&end)))}
        </div>
        {move||{let n=notice.get();(!n.is_empty()).then(||view!{<p class="refusal" data-testid="accounting-taxdocs-refusal">{n}</p>})}}
        {move||answer.get().map(|a|listing(&a))}
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    fn answer(more: bool) -> Value {
        json!({"book":{"id":"b","code":"BOOK-IDR","currency":"IDR","minorUnits":2},"from":"2026-11-01","to":"2026-11-30",
            "documents":[
                {"sourceId":"s1","date":"2026-11-03","side":"output","effect":"issue","documentKind":"invoice","documentCode":"INV-1",
                 "counterpartyName":"Toko \"Maju\", PT <b>","counterpartyTaxId":"02.345.678.9-012.000","companyTaxId":"01.234.567.8-999.000",
                 "taxCode":"VAT-OUT-12-B11-12","taxCodeVersion":1,"rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12},
                 "taxableBaseMinor":"100000000","taxMinor":"11000000","taxReference":"010.000-26.00000001","taxTransactionCode":"01"},
                {"sourceId":"s2","date":"2026-11-04","side":"output","effect":"correction","documentKind":"invoice","documentCode":"INV-1",
                 "counterpartyName":"Toko","counterpartyTaxId":null,"companyTaxId":null,
                 "taxCode":"VAT","taxCodeVersion":2,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
                 "taxableBaseMinor":"-1000000","taxMinor":"-110000","taxReference":"R-9","taxTransactionCode":null},
                {"sourceId":"s3","date":"2026-11-05","side":"input","effect":"bill","documentKind":"supplier_document","documentCode":"SUP/77",
                 "counterpartyName":"Pemasok","counterpartyTaxId":null,"companyTaxId":null,
                 "taxCode":"VAT-IN","taxCodeVersion":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
                 "taxableBaseMinor":"14000000","taxMinor":"1540100","taxReference":null,"taxTransactionCode":null}],
            "page":{"limit":100,"hasMore":more,"nextCursor":if more {json!("s3")} else {Value::Null}},
            "notice":"Draft export of data your company entered in Archava. It is not a tax return and is not in any tax authority's import format. Archava does not verify tax codes, rates, tax ids or tax invoice references."})
    }
    #[test]
    fn the_listing_shows_each_document_with_its_tax_id_code_amounts_and_reference() {
        let html = listing(&answer(false)).to_html();
        assert!(html.contains("Documents · BOOK-IDR · 2026-11-01 through 2026-11-30"));
        assert_eq!(
            html.matches("data-testid=\"accounting-taxdocs-row\"")
                .count(),
            3
        );
        let at = |text: &str| html.find(text).unwrap();
        let row = &html[at("2026-11-03")..at("2026-11-04")];
        assert!(row.contains(">Sales invoice<"), "{row}");
        assert!(row.contains(">INV-1<"));
        assert!(
            row.contains("Toko &quot;Maju&quot;, PT &lt;b&gt;")
                || row.contains("Toko \"Maju\", PT &lt;b&gt;"),
            "{row}"
        );
        assert!(row.contains(">02.345.678.9-012.000<"));
        assert!(row.contains("VAT-OUT-12-B11-12 · version 1 · 12% of 11/12 of the price"));
        assert!(
            row.contains(">IDR 1000000.00<") && row.contains(">IDR 110000.00<"),
            "{row}"
        );
        assert!(row.contains(">010.000-26.00000001 · code 01<"));
        let row = &html[at("2026-11-04")..at("2026-11-05")];
        assert!(row.contains(">Sales invoice correction<"));
        assert!(
            row.contains("data-testid=\"accounting-taxdocs-taxid\">—<"),
            "{row}"
        );
        assert!(
            row.contains(">IDR -10000.00<") && row.contains(">IDR -1100.00<"),
            "{row}"
        );
        assert!(row.contains(">R-9<"));
        let row = &html[at("2026-11-05")..];
        assert!(row.contains(">Supplier bill<") && row.contains(">SUP/77<"));
        assert!(row.contains("VAT-IN · version 1 · 11%"));
        assert!(row.contains(">IDR 15401.00<"));
        assert!(row.contains("data-testid=\"accounting-taxdocs-reference\">—<"));
        assert!(html.contains("not a tax return"));
        assert!(html.contains("not in any tax authority's import format"));
        assert!(!html.contains("accounting-taxdocs-more"));
        assert!(!html.contains("<b>"));
        for word in ["e-Faktur", "Coretax", "compliant", "official"] {
            assert!(!html.contains(word), "{word}");
        }
        assert!(listing(&answer(true))
            .to_html()
            .contains("accounting-taxdocs-more"));
    }
    #[test]
    fn an_empty_period_says_so_and_the_download_names_the_book_and_period() {
        let mut empty = answer(false);
        empty["documents"] = json!([]);
        let html = listing(&empty).to_html();
        assert!(html.contains("data-testid=\"accounting-taxdocs-empty\""));
        assert!(!html.contains("accounting-taxdocs-table"));
        let path = csv_path("acme retail", "0b0c", "2026-11-01", "2026-11-30");
        assert_eq!(
            path,
            "/erp/accounting/tax-report/documents.csv?tenant=acme%20retail&bookId=0b0c&from=2026-11-01&to=2026-11-30"
        );
        let link = csv_link(path).to_html();
        assert!(link.contains("data-testid=\"accounting-taxdocs-csv\""));
        assert!(link.contains("download"));
        assert!(link.contains("draft export, not a tax return"));
        assert_eq!(
            source(&json!({"documentKind":"invoice","effect":"void"})),
            "Sales invoice void"
        );
        assert_eq!(
            source(&json!({"documentKind":"supplier_document","effect":"void"})),
            "Supplier bill void"
        );
        assert_eq!(source(&json!({})), "Unknown source");
    }
}
