//! The book's tax codes as the company entered them. Nothing is computed with
//! a code here (sales orders and their invoices use one, see `document_tax`):
//! the panel lists the stored versions, their state (entered, confirmed,
//! withdrawn or retired) and which apply on the date the server answered
//! for. Only a confirmed version applies.
use super::*;
use archava_domain::erp::tax::Fraction;
use archava_domain::erp::tax_templates::{self, TaxTemplate};

/// Shown wherever a tax code is listed (TAX.md rule 1).
const NOTICE: &str =
    "Entered by your company; Archava does not verify tax rates or rules. An entered version does not apply until it is confirmed. A company sales order can name a confirmed output code and a supplier claim a confirmed input code; the invoice or claim stores the tax, and its captured source posts that tax to the code's control account.";

fn fraction(value: &Value) -> Option<Fraction> {
    serde_json::from_value(value.clone()).ok()
}
/// The rate as a percentage in exact digits, never through floating point.
fn rate(row: &Value) -> String {
    fraction(&row["rate"]).map_or_else(|| "Unreadable rate".into(), Fraction::percent)
}
/// The part of the price the rate applies to; empty for the whole price.
fn base(row: &Value) -> String {
    match fraction(&row["base"]) {
        Some(base) if base.is_one() => "Whole price".into(),
        Some(base) => format!("{}/{} of the price", base.numerator, base.denominator),
        None => "Unreadable base".into(),
    }
}
fn kind(row: &Value) -> &'static str {
    match row["kind"].as_str() {
        Some("output") => "Output (sales)",
        Some("input") => "Input (purchases)",
        _ => "Unknown",
    }
}
fn range(row: &Value) -> String {
    let from = text(row, "effectiveFrom");
    match row["effectiveTo"].as_str() {
        Some(to) => format!("{from} through {to}"),
        None => format!("{from} onward"),
    }
}
/// The state in words that say whether the version can apply at all.
fn state(row: &Value) -> &'static str {
    match row["state"].as_str() {
        Some("entered") => "Entered — not confirmed, does not apply",
        Some("confirmed") => "Confirmed",
        Some("withdrawn") => "Withdrawn — never applied",
        Some("retired") => "Retired",
        _ => "Unknown",
    }
}
fn person(row: &Value, key: &str) -> String {
    row[key]
        .as_str()
        .filter(|name| !name.is_empty())
        .map_or_else(|| "Not yet".into(), str::to_owned)
}
fn account(row: &Value) -> String {
    let account = &row["controlAccount"];
    format!("{} · {}", text(account, "code"), text(account, "name"))
}

/// The panel for one workspace answer; nothing when the caller may not read
/// tax codes (the collection is then absent).
fn panel(data: &Value) -> Option<impl IntoView> {
    let rows = data["tax_codes"].as_array()?.clone();
    let on = rows
        .first()
        .map(|row| text(row, "appliesOn"))
        .unwrap_or_default();
    let more = data["collections"].as_array().is_some_and(|collections| {
        collections
            .iter()
            .any(|c| c["id"] == "tax_codes" && c["hasMore"] == true)
    });
    Some(
        view! {<section class="ops-detail" data-testid="accounting-tax-codes"><h3>"Tax codes"</h3>
            <p class="hint" data-testid="accounting-tax-notice">{NOTICE}</p>
            {if rows.is_empty() {
                view!{<p class="hint" data-testid="accounting-tax-empty">"This book has no tax codes. Use the action \"Enter a company tax code\" to add one, then \"Confirm a company tax code version\"."</p>}.into_any()
            } else {
                view!{<div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-tax-table"><caption>{format!("Tax code versions and whether each applies on {on}")}</caption>
                    <thead><tr><th>"Code"</th><th>"Kind"</th><th>"Rate"</th><th>"Applies to"</th><th>"Control account"</th><th>"Effective"</th><th>"State"</th><th>"Entered by"</th><th>"Confirmed by"</th><th>{format!("On {on}")}</th></tr></thead>
                    <tbody>{rows.into_iter().map(|row|view!{<tr data-testid="accounting-tax-row">
                        <td>{format!("{} · version {}",text(&row,"code"),row["version"])}</td>
                        <td>{kind(&row)}</td>
                        <td data-testid="accounting-tax-rate">{rate(&row)}</td>
                        <td data-testid="accounting-tax-base">{base(&row)}</td>
                        <td>{account(&row)}</td>
                        <td>{range(&row)}</td>
                        <td data-testid="accounting-tax-state">{state(&row)}</td>
                    <td>{person(&row,"createdBy")}</td>
                    <td data-testid="accounting-tax-reviewer">{person(&row,"reviewedBy")}</td>
                        <td>{if row["applies"]==true {"Applies"} else {"Does not apply"}}</td>
                    </tr>}).collect_view()}</tbody></table></div>}.into_any()
            }}
            {more.then(||view!{<p class="hint">"More tax codes exist than this page shows. Open the Tax codes collection to page through them."</p>})}
        </section>},
    )
}

/// The templates offered on a book kept in `currency`: those of every market
/// the catalogue lists for it. A book in another currency is offered none,
/// and its screen is what it was.
fn templates_for(currency: &str) -> Vec<&'static TaxTemplate> {
    tax_templates::markets_for_currency(currency)
        .into_iter()
        .flat_map(|market| market.templates.iter())
        .collect()
}
/// How a template is listed: its market, what its own numbers are and the
/// code text it suggests.
fn template_label(template: &TaxTemplate) -> String {
    let market = tax_templates::catalogue()
        .markets
        .iter()
        .find(|market| market.code == template.market)
        .map_or(template.market.as_str(), |market| market.name.as_str());
    format!(
        "{market} · {} · suggested code {}",
        template.label, template.code
    )
}
/// The form-prefill request for a template: the `tax_code.create` form with
/// the template's code text, kind, rate and base. Nothing is created: the
/// book, the control account, the first day and the reason stay the person's
/// own entries, and the code still needs its separate confirmation.
pub(in crate::operations) fn template_request(id: &str) -> Option<Value> {
    tax_templates::by_id(id).map(|template| {
        json!({"action":"tax_code.create","template":template.id,"values":template.form_values()})
    })
}
/// What the picker says once a template filled the form.
fn applied_line(template: &TaxTemplate) -> String {
    format!(
        "The form \"Enter a company tax code\" now holds the template \"{}\" (not verified). Choose your control account and the first day, check every value, then review and save it. It applies only after \"Confirm a company tax code version\".",
        template.label
    )
}
/// The picker for the given templates; `applied` is the template that filled
/// the form, if one did.
fn picker(
    templates: &[&'static TaxTemplate],
    chosen: RwSignal<String>,
    applied: Option<&'static TaxTemplate>,
    apply: Callback<()>,
    clear: Callback<()>,
) -> AnyView {
    let options = templates
        .iter()
        .map(|template| {
            let id = template.id.clone();
            let selected = template.id.clone();
            view! {<option value=id prop:selected=move||chosen.get()==selected>{template_label(template)}</option>}
        })
        .collect_view();
    view! {<details class="ops-disclosure" data-testid="accounting-tax-templates"><summary>"Start a tax code from a template"</summary><div class="ops-form">
        <p class="hint" data-testid="accounting-tax-template-notice">{tax_templates::catalogue().notice.clone()}</p>
        <label class="ops-field"><span>"Template"</span><select data-testid="accounting-tax-template" prop:value=move||chosen.get() on:change=move|e|chosen.set_if_alive(event_target_value(&e))>
            <option value="" prop:selected=move||chosen.get().is_empty()>"Choose a template…"</option>{options}</select></label>
        <button type="button" class="btn secondary" data-testid="accounting-tax-template-use" disabled=move||chosen.get().is_empty() on:click=move|_|apply.run(())>"Fill the tax code form"</button>
        {applied.map(|template|view!{<p class="hint" role="status" data-testid="accounting-tax-template-applied">{applied_line(template)}</p>
            <button type="button" class="btn secondary" data-testid="accounting-tax-template-clear" on:click=move|_|clear.run(())>"Clear the template from the form"</button>})}
    </div></details>}
    .into_any()
}

/// "Start from a template" under the tax codes: offered to a person who may
/// enter tax codes, on a book whose currency a market of the catalogue names.
/// Choosing one only pre-fills the reviewed form beside the records.
#[component]
fn TaxTemplates(work: Workbench) -> impl IntoView {
    let chosen = RwSignal::new(String::new());
    let apply = Callback::new(move |()| {
        if let Some(request) = template_request(&chosen.get_untracked()) {
            work.prefill.set_if_alive(request);
        }
    });
    let clear = Callback::new(move |()| {
        work.prefill.set_if_alive(Value::Null);
        chosen.set_if_alive(String::new());
    });
    move || {
        let data = work.data.get();
        let templates = templates_for(&text(&data["configuredBook"], "currency"));
        if templates.is_empty() || !data["tax_codes"].is_array() || !work.permits("tax.manage") {
            return ().into_any();
        }
        let applied = work.prefill.get()["template"]
            .as_str()
            .and_then(tax_templates::by_id);
        picker(&templates, chosen, applied, apply, clear)
    }
}

#[component]
pub(super) fn TaxCodes(work: Workbench) -> impl IntoView {
    view! { {move||panel(&work.data.get())} <TaxTemplates work/> }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    fn code(
        code: &str,
        rate: (i32, i32),
        base: (i32, i32),
        state: &str,
        reviewer: Value,
        to: Value,
        applies: bool,
    ) -> Value {
        json!({"id":code,"code":code,"version":1,"kind":"output","state":state,
            "rate":{"numerator":rate.0,"denominator":rate.1},"base":{"numerator":base.0,"denominator":base.1},
            "controlAccount":{"accountId":"vat","code":"VAT-OUT","name":"Output tax <payable>"},
            "effectiveFrom":"2026-01-01","effectiveTo":to,"createdBy":"alice","reviewedBy":reviewer,"appliesOn":"2026-11-14","applies":applies})
    }
    #[test]
    fn the_panel_lists_each_code_with_an_exact_rate_its_state_and_the_company_notice() {
        let data = json!({"tax_codes":[
            code("VAT-11",(11,100),(1,1),"retired",json!("bob"),json!("2026-12-31"),true),
            code("VAT-12",(12,100),(11,12),"entered",Value::Null,Value::Null,false),
            code("VAT-OLD",(10,100),(1,1),"withdrawn",Value::Null,Value::Null,false),
            code("VAT-NOW",(11,100),(1,1),"confirmed",json!("bob"),Value::Null,true),
        ],"collections":[{"id":"tax_codes","hasMore":false}]});
        let html = panel(&data).unwrap().to_html();
        assert!(html.contains("data-testid=\"accounting-tax-codes\""));
        assert!(
            html.contains("Entered by your company; Archava does not verify tax rates or rules.")
        );
        assert!(html.contains("does not apply until it is confirmed"));
        assert_eq!(
            html.matches("data-testid=\"accounting-tax-row\"").count(),
            4
        );
        let at = |name: &str| html.find(&format!("{name} · version 1")).unwrap();
        // 11/100 on the whole price, confirmed by a second person, then retired.
        let row = &html[at("VAT-11")..at("VAT-12")];
        assert!(row.contains(">11%<"), "{row}");
        assert!(row.contains("Whole price"));
        assert!(row.contains("2026-01-01 through 2026-12-31"));
        assert!(row.contains(">Retired<"));
        assert!(row.contains(">alice<") && row.contains(">bob<"));
        assert!(row.contains(">Applies<"));
        // 12/100 on eleven twelfths of the price, entered and not yet confirmed.
        let row = &html[at("VAT-12")..at("VAT-OLD")];
        assert!(row.contains(">12%<"), "{row}");
        assert!(row.contains("11/12 of the price"));
        assert!(row.contains("2026-01-01 onward"));
        assert!(row.contains("Entered — not confirmed, does not apply"));
        assert!(row.contains(">alice<") && row.contains(">Not yet<"));
        assert!(row.contains("Does not apply") && !row.contains(">Applies<"));
        let row = &html[at("VAT-OLD")..at("VAT-NOW")];
        assert!(row.contains("Withdrawn — never applied"));
        assert!(row.contains("Does not apply"));
        let row = &html[at("VAT-NOW")..];
        assert!(row.contains(">Confirmed<") && row.contains(">Applies<"));
        assert!(html.contains("Output (sales)"));
        assert!(html.contains("On 2026-11-14"));
        assert!(html.contains("Output tax &lt;payable&gt;"));
        assert!(!html.contains("0.11") && !html.contains("0.12"));
    }
    #[test]
    fn an_empty_book_says_so_and_a_reader_without_tax_access_sees_no_panel() {
        let html = panel(&json!({"tax_codes":[]})).unwrap().to_html();
        assert!(html.contains("data-testid=\"accounting-tax-empty\""));
        assert!(html.contains("Archava does not verify tax rates or rules"));
        assert!(!html.contains("accounting-tax-table"));
        assert!(panel(&json!({"accounts":[]})).is_none());
    }
    #[test]
    fn the_template_picker_lists_unverified_starting_points_with_the_notice() {
        let owner = leptos::reactive::owner::Owner::new();
        owner.with(|| {
            let templates = templates_for("IDR");
            assert_eq!(templates.len(), 4);
            assert!(templates_for("USD").is_empty());
            assert!(templates_for("").is_empty());
            let chosen = RwSignal::new(String::new());
            let nothing = Callback::new(|()| {});
            let html = picker(&templates, chosen, None, nothing, nothing).to_html();
            assert!(html.contains("data-testid=\"accounting-tax-templates\""));
            assert!(html.contains("Start a tax code from a template"));
            assert!(html.contains("Archava does not verify tax rates or rules"));
            assert!(html.contains("Check every value with your own tax adviser"));
            assert!(html.contains("only pre-fills the tax code form"));
            assert!(html.contains(
                "Indonesia · Output tax on sales, 11% on the full price · suggested code VAT-OUT-11"
            ));
            assert!(html.contains(
                "Indonesia · Input tax on purchases, 12% on 11/12 of the price · suggested code VAT-IN-12-B11-12"
            ));
            assert_eq!(html.matches("<option").count(), 5);
            assert!(!html.contains("accounting-tax-template-applied"));
            for word in ["compliant", "official", "Coretax", "e-Faktur", "required"] {
                assert!(!html.contains(word), "{word}");
            }
            // Once a template filled the form the picker says what is left to do.
            let applied = tax_templates::by_id("id-output-12-base-11-12");
            let html = picker(&templates, chosen, applied, nothing, nothing).to_html();
            assert!(html.contains("data-testid=\"accounting-tax-template-applied\""));
            assert!(html.contains("(not verified)"));
            assert!(html.contains("Choose your control account and the first day"));
            assert!(html.contains("accounting-tax-template-clear"));
        });
    }
    #[test]
    fn a_template_request_names_the_create_form_and_only_its_code_kind_rate_and_base() {
        let request = template_request("id-output-12-base-11-12").unwrap();
        assert_eq!(
            request,
            json!({"action":"tax_code.create","template":"id-output-12-base-11-12","values":{
                "code":"VAT-OUT-12-B11-12","kind":"output",
                "rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12}}})
        );
        assert!(template_request("no-such-template").is_none());
        assert!(template_request("").is_none());
    }
    #[test]
    fn rates_without_a_finite_percentage_stay_fractions() {
        assert_eq!(
            rate(&json!({"rate":{"numerator":43,"denominator":400}})),
            "10.75%"
        );
        assert_eq!(
            rate(&json!({"rate":{"numerator":1,"denominator":3}})),
            "1/3"
        );
        assert_eq!(rate(&json!({"rate":0.11})), "Unreadable rate");
        assert_eq!(
            base(&json!({"base":{"numerator":12,"denominator":12}})),
            "Whole price"
        );
        assert_eq!(state(&json!({"state":"draft"})), "Unknown");
    }
}
