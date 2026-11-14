//! The tax report of a period: output and input tax of posted sources per
//! tax code version, beside the posted movement of each tax control account.
//! It is the company's own figures from its own codes, not a tax return.
use super::*;
use archava_domain::erp::tax::Fraction;

/// The one-line verdict for a report.
fn verdict(report: &Value) -> &'static str {
    let listed = report["controls"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty());
    if !listed {
        "This book has no confirmed tax code and no taxed source in this period."
    } else if report["postedSourceControlsAgree"] == true {
        "Posted sources explain the movement of every tax control account in this period."
    } else {
        "Some tax control accounts moved by an amount posted sources do not explain. Review the differences."
    }
}
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
fn kind(row: &Value) -> &'static str {
    match row["kind"].as_str() {
        Some("output") => "Output (sales)",
        Some("input") => "Input (purchases)",
        _ => "Unknown",
    }
}
/// An amount in the unit documents store (whole Rupiah for IDR, the minor
/// unit otherwise), which is not the ledger's minor unit for an IDR book.
fn document_amount(value: &Value, book: &Value) -> String {
    let unit = if book["currency"] == "IDR" {
        json!(0)
    } else {
        book["minorUnits"].clone()
    };
    amount(
        value,
        &json!({"currency":book["currency"],"minorUnits":unit}),
    )
}
fn codes(report: &Value, book: &Value) -> AnyView {
    let rows = report["codes"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return view! {<p class="hint" data-testid="accounting-taxreport-empty">"No posted source carries tax in this period."</p>}.into_any();
    }
    let book = book.clone();
    view! {<div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-taxreport-codes"><caption>"Posted tax by tax code version"</caption>
        <thead><tr><th>"Code"</th><th>"Kind"</th><th>"Rate"</th><th>"Posted sources"</th><th>"Taxable amount"</th><th>"Tax"</th></tr></thead>
        <tbody>{rows.into_iter().map(|row|view!{<tr data-testid="accounting-taxreport-code">
            <td>{format!("{} · version {}",text(&row,"code"),row["version"])}</td>
            <td>{kind(&row)}</td>
            <td>{rule(&row)}</td>
            <td>{row["sourceCount"].to_string()}</td>
            <td>{amount(&row["taxableBaseMinor"],&book)}</td>
            <td>{amount(&row["taxMinor"],&book)}</td>
        </tr>}).collect_view()}</tbody></table></div>}
    .into_any()
}
fn totals(report: &Value, book: &Value) -> AnyView {
    let totals = &report["totals"];
    view! {<dl class="operator-details" data-testid="accounting-taxreport-totals">
        <dt>"Output tax"</dt><dd data-testid="accounting-taxreport-output">{amount(&totals["outputTaxMinor"],book)}</dd>
        <dt>"Input tax"</dt><dd data-testid="accounting-taxreport-input">{amount(&totals["inputTaxMinor"],book)}</dd>
        <dt>"Output less input"</dt><dd data-testid="accounting-taxreport-difference">{amount(&totals["differenceMinor"],book)}</dd>
    </dl>}
    .into_any()
}
fn controls(report: &Value, book: &Value) -> AnyView {
    let rows = report["controls"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return ().into_any();
    }
    let book = book.clone();
    view! {<div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-taxreport-controls"><caption>"Tax control accounts in this period"</caption>
        <thead><tr><th>"Account"</th><th>"Kind"</th><th>"Tax of posted sources"</th><th>"Ledger movement"</th><th>"Difference"</th></tr></thead>
        <tbody>{rows.into_iter().map(|row|{
            let differs=row["differenceMinor"]!="0";
            view!{<tr data-testid="accounting-taxreport-control">
                <td>{format!("{} · {}",text(&row,"code"),text(&row,"name"))}</td>
                <td>{kind(&row)}</td>
                <td>{amount(&row["reportedTaxMinor"],&book)}</td>
                <td>{amount(&row["movementMinor"],&book)}</td>
                <td class=if differs {"refusal"} else {""} data-testid=if differs {"accounting-taxreport-control-differs"} else {"accounting-taxreport-control-agrees"}>{amount(&row["differenceMinor"],&book)}</td>
            </tr>}
        }).collect_view()}</tbody></table></div>}
    .into_any()
}
fn apart(report: &Value, book: &Value) -> AnyView {
    let pending = &report["pending"];
    let outside = &report["outsideLedger"];
    let waiting = |side: &Value| {
        format!(
            "{} (taxable {}, tax {})",
            side["sourceCount"],
            amount(&side["taxableBaseMinor"], book),
            amount(&side["taxMinor"], book)
        )
    };
    let never = |side: &Value| {
        format!(
            "{} (tax {})",
            side["count"],
            document_amount(&side["taxMinor"], book)
        )
    };
    view! {<p class="hint" data-testid="accounting-taxreport-pending">{format!("Captured but not posted, not in the totals: invoice sources {}; supplier bill sources {}.",waiting(&pending["output"]),waiting(&pending["input"]))}</p>
        <p class="hint" data-testid="accounting-taxreport-outside">{format!("Outside the ledger, not in the totals: taxed invoices issued without a posting policy {}; taxed supplier documents matched without one {}. These amounts are in the documents' own unit.",never(&outside["invoices"]),never(&outside["supplierDocuments"]))}</p>}
    .into_any()
}
/// The result of one report.
fn result(report: &Value) -> AnyView {
    let book = report["book"].clone();
    let agree = report["postedSourceControlsAgree"] == true;
    view! {<div data-testid="accounting-taxreport-result">
        <h4>{format!("{} · {} through {}",text(&book,"code"),text(report,"from"),text(report,"to"))}</h4>
        <p data-testid="accounting-taxreport-verdict" class=if agree {"hint"} else {"refusal"}>{verdict(report)}</p>
        {codes(report,&book)}
        {totals(report,&book)}
        {controls(report,&book)}
        {apart(report,&book)}
        <p class="hint" data-testid="accounting-taxreport-notice">{text(report,"notice")}</p>
    </div>}
    .into_any()
}

#[component]
pub(super) fn TaxReport(work: Workbench) -> impl IntoView {
    let from = RwSignal::new(String::new());
    let to = RwSignal::new(String::new());
    let report = RwSignal::new(None::<Value>);
    let notice = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let sequence = RwSignal::new(0_u64);
    Effect::new(move |_| {
        let data = work.data.get();
        report.set_if_alive(None);
        notice.set_if_alive(String::new());
        sequence.update_if_alive(|v| *v += 1);
        busy.set_if_alive(false);
        // The dates start as the open period of the book, when it has one.
        let periods = data["periods"].as_array().cloned().unwrap_or_default();
        let chosen = periods
            .iter()
            .find(|row| row["state"] == "open")
            .or_else(|| periods.first());
        if let Some(row) = chosen {
            if from.get_untracked().is_empty() && to.get_untracked().is_empty() {
                from.set_if_alive(text(row, "startDate"));
                to.set_if_alive(text(row, "endDate"));
            }
        }
    });
    let load = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(held) = session::held() else { return };
        let book = text(&work.data.get_untracked()["configuredBook"], "id");
        let (start, end) = (from.get_untracked(), to.get_untracked());
        let date = archava_domain::erp::accommodation::parse_calendar_date;
        if book.is_empty() || date(&start).is_none() || date(&end).is_none() || start > end {
            report.set_if_alive(None);
            notice.set_if_alive("Select a company book and a valid date range.".into());
            return;
        }
        let company = work.company_key.get_untracked();
        sequence.update_if_alive(|v| *v += 1);
        let current = sequence.get_untracked();
        busy.set_if_alive(true);
        notice.set_if_alive(String::new());
        let path = format!(
            "/erp/accounting/tax-report?tenant={}&bookId={book}&from={start}&to={end}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if sequence.is_disposed()
                || sequence.get_untracked() != current
                || work.company_key.get_untracked() != company
            {
                return;
            }
            busy.set_if_alive(false);
            match answer {
                Ok(v) if v.status == 200 => report.set_if_alive(Some(v.body)),
                Ok(v) => {
                    report.set_if_alive(None);
                    notice.set_if_alive(message(&v));
                }
                Err(_) => {
                    report.set_if_alive(None);
                    notice.set_if_alive(
                        "The tax report could not be loaded. Refresh and try again.".into(),
                    );
                }
            }
        });
    };
    let changed = move |target: RwSignal<String>| {
        move |e: leptos::ev::Event| {
            target.set_if_alive(event_target_value(&e));
            report.set_if_alive(None);
        }
    };
    view! {<section class="ops-detail" data-testid="accounting-taxreport"><h3>"Tax report"</h3>
        <p class="hint">"Adds up the tax of posted invoice and supplier bill sources by tax code for the chosen dates and compares it with the movement of each tax control account. Tax codes are entered by your company; Archava does not verify tax rates or rules, and this is not a tax return."</p>
        <form class="ops-form" on:submit=load>
            <label class="ops-field"><span>"From"</span><input data-testid="accounting-taxreport-from" type="date" min="2000-01-01" max="2100-12-31" required prop:value=move||from.get() on:input=changed(from)/></label>
            <label class="ops-field"><span>"Through"</span><input data-testid="accounting-taxreport-to" type="date" min="2000-01-01" max="2100-12-31" required prop:value=move||to.get() on:input=changed(to)/></label>
            <button type="submit" class="btn secondary" data-testid="accounting-taxreport-run" disabled=move||busy.get()||!work.permits("reports.read")||!work.permits("tax.read")>"Show tax report"</button>
        </form>
        {move||{let n=notice.get();(!n.is_empty()).then(||view!{<p class="refusal" data-testid="accounting-taxreport-refusal">{n}</p>})}}
        {move||report.get().map(|r|result(&r))}
    </section>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    fn report(movement: &str, difference: &str, agree: bool) -> Value {
        json!({"book":{"id":"b","code":"BOOK-IDR","currency":"IDR","minorUnits":2},"from":"2026-11-01","to":"2026-11-30",
            "codes":[
                {"code":"VAT","version":1,"kind":"output","rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},"sourceCount":2,"taxableBaseMinor":"2000000","taxMinor":"220000"},
                {"code":"VAT-12","version":2,"kind":"output","rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12},"sourceCount":1,"taxableBaseMinor":"3000000","taxMinor":"330000"},
                {"code":"VAT-IN","version":1,"kind":"input","rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},"sourceCount":1,"taxableBaseMinor":"14000000","taxMinor":"1540100"}],
            "totals":{"outputTaxMinor":"550000","inputTaxMinor":"1540100","differenceMinor":"-990100"},
            "controls":[
                {"code":"VAT-IN","name":"Input tax","kind":"input","reportedTaxMinor":"1540100","movementMinor":"1540100","differenceMinor":"0"},
                {"code":"VAT-OUT","name":"Output tax <payable>","kind":"output","reportedTaxMinor":"550000","movementMinor":movement,"differenceMinor":difference}],
            "postedSourceControlsAgree":agree,
            "pending":{"output":{"sourceCount":1,"taxableBaseMinor":"3000000","taxMinor":"330000"},"input":{"sourceCount":0,"taxableBaseMinor":"0","taxMinor":"0"}},
            "outsideLedger":{"unit":"document","currency":"IDR","invoices":{"count":1,"taxMinor":"2200"},"supplierDocuments":{"count":0,"taxMinor":"0"}},
            "notice":"Tax codes are entered by your company; Archava does not verify tax rates or rules. This report is not a tax return."})
    }
    #[test]
    fn a_balanced_report_shows_codes_totals_and_control_accounts_without_a_difference() {
        let html = result(&report("550000", "0", true)).to_html();
        assert!(html.contains("BOOK-IDR · 2026-11-01 through 2026-11-30"));
        assert!(html.contains(
            "Posted sources explain the movement of every tax control account in this period."
        ));
        assert_eq!(
            html.matches("data-testid=\"accounting-taxreport-code\"")
                .count(),
            3
        );
        let at = |name: &str| html.find(name).unwrap();
        let first = &html[at("VAT · version 1")..at("VAT-12 · version 2")];
        for shown in [
            "Output (sales)",
            ">11%<",
            ">2<",
            "IDR 20000.00",
            "IDR 2200.00",
        ] {
            assert!(first.contains(shown), "{shown}: {first}");
        }
        let second = &html[at("VAT-12 · version 2")..at("VAT-IN · version 1")];
        assert!(
            second.contains("12% of 11/12 of the price"),
            "the base of a version is shown: {second}"
        );
        assert!(html.contains("Input (purchases)"));
        for (id, shown) in [
            ("accounting-taxreport-output", "IDR 5500.00"),
            ("accounting-taxreport-input", "IDR 15401.00"),
            ("accounting-taxreport-difference", "IDR -9901.00"),
        ] {
            assert!(
                html.contains(&format!("data-testid=\"{id}\">{shown}<")),
                "{id}: {html}"
            );
        }
        assert_eq!(
            html.matches("data-testid=\"accounting-taxreport-control-agrees\"")
                .count(),
            2
        );
        assert!(!html.contains("accounting-taxreport-control-differs"));
        // The account name is escaped, never markup.
        assert!(html.contains("Output tax &lt;payable&gt;"));
        assert!(html.contains("invoice sources 1 (taxable IDR 30000.00, tax IDR 3300.00); supplier bill sources 0 (taxable IDR 0.00, tax IDR 0.00)"));
        // Outside-ledger amounts are in whole Rupiah, the documents' unit.
        assert!(html.contains("taxed invoices issued without a posting policy 1 (tax IDR 2200); taxed supplier documents matched without one 0 (tax IDR 0)"));
        assert!(html.contains("Archava does not verify tax rates or rules"));
        assert!(html.contains("not a tax return"));
    }
    #[test]
    fn a_difference_is_marked_on_its_control_account_and_in_the_verdict() {
        let html = result(&report("550500", "500", false)).to_html();
        assert!(html.contains("Some tax control accounts moved by an amount posted sources do not explain. Review the differences."));
        assert!(html.contains("data-testid=\"accounting-taxreport-verdict\" class=\"refusal\""));
        assert_eq!(
            html.matches("data-testid=\"accounting-taxreport-control-differs\"")
                .count(),
            1
        );
        assert_eq!(
            html.matches("data-testid=\"accounting-taxreport-control-agrees\"")
                .count(),
            1
        );
        let differs = &html[html.find("accounting-taxreport-control-differs").unwrap()..];
        assert!(
            differs
                .starts_with("accounting-taxreport-control-differs\" class=\"refusal\">IDR 5.00<"),
            "{}",
            &differs[..80]
        );
    }
    #[test]
    fn a_book_without_tax_and_a_usd_document_unit_read_plainly() {
        let empty = json!({"book":{"code":"BOOK-USD","currency":"USD","minorUnits":2},"from":"2026-01-01","to":"2026-01-31","codes":[],
            "totals":{"outputTaxMinor":"0","inputTaxMinor":"0","differenceMinor":"0"},"controls":[],"postedSourceControlsAgree":true,
            "pending":{"output":{"sourceCount":0,"taxableBaseMinor":"0","taxMinor":"0"},"input":{"sourceCount":0,"taxableBaseMinor":"0","taxMinor":"0"}},
            "outsideLedger":{"invoices":{"count":0,"taxMinor":"0"},"supplierDocuments":{"count":1,"taxMinor":"15401"}},"notice":"n"});
        let html = result(&empty).to_html();
        assert!(html
            .contains("This book has no confirmed tax code and no taxed source in this period."));
        assert!(html.contains("data-testid=\"accounting-taxreport-empty\""));
        assert!(!html.contains("accounting-taxreport-controls"));
        assert!(html.contains("matched without one 1 (tax USD 154.01)"));
    }
}
