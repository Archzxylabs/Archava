//! Tax on a sales order and on the invoice made from it (TAX.md stage B).
//! The order names a tax code and a price basis; the invoice stores the
//! version it was taxed with. Every amount is the server's integer, shown in
//! the document's currency; the only arithmetic here is the preview of an
//! invoice that does not exist yet, with the domain's own rule.
use super::*;
use archava_domain::erp::tax::{describe, DocumentTax, Fraction, TaxRule, DOCUMENT_NOTICE};

fn fraction(value: &Value) -> Option<Fraction> {
    serde_json::from_value(value.clone()).ok()
}
/// `VAT 11%`, or `VAT 12% of 11/12 of the price`, from a row that carries
/// `code`, `rate` and `base`.
fn code_label(row: &Value) -> String {
    match (fraction(&row["rate"]), fraction(&row["base"])) {
        (Some(rate), Some(base)) => describe(&text(row, "code"), rate, base),
        _ => format!("{} (unreadable rate)", text(row, "code")),
    }
}
fn basis(price_includes_tax: bool) -> &'static str {
    if price_includes_tax {
        "included in the prices"
    } else {
        "added to the prices"
    }
}

/// The net, tax and total of a taxed invoice, after its corrections, with the
/// tax code version the invoice stored. Renders nothing for an untaxed invoice.
#[component]
pub fn InvoiceTax(invoice: Value) -> impl IntoView {
    let snapshot = invoice["tax"].clone();
    snapshot.is_object().then(|| {
        let currency = currency_of(&invoice);
        let total = integer(&invoice, "gross") + integer(&invoice, "correction");
        let tax = integer(&invoice, "taxMinor") + integer(&invoice, "correctionTaxMinor");
        let label = format!(
            "Tax · {} · {} · version {}",
            code_label(&snapshot),
            basis(snapshot["priceIncludesTax"] == true),
            integer(&snapshot, "version")
        );
        view! {<dl class="ops-totals" data-testid="invoice-tax">
            <dt>"Net"</dt><dd data-testid="invoice-tax-net">{currency_amount(total-tax,&currency)}</dd>
            <dt>{label}</dt><dd data-testid="invoice-tax-amount">{currency_amount(tax,&currency)}</dd>
            <dt>"Total"</dt><dd data-testid="invoice-tax-total">{currency_amount(total,&currency)}</dd>
        </dl><p class="hint" data-testid="invoice-tax-notice">{DOCUMENT_NOTICE}" The invoice keeps the rate it was created with; corrections are split with the same rate."</p>}
    })
}

/// What an invoice made now from `order` would carry, for the invoice-from-
/// order form: the order's tax code as the applicable codes of its company
/// list it today. The server computes the stored amounts when the invoice is
/// created; without an applicable version it refuses the invoice.
#[component]
pub fn OrderTaxPreview(order: Value, codes: Vec<Value>) -> impl IntoView {
    let code = order["taxCode"].as_str().map(str::to_owned);
    code.map(|code| {
        let currency = currency_of(&order);
        let inclusive = order["priceIncludesTax"] == true;
        let listed = codes.into_iter().find(|row| row["code"] == code.as_str());
        let taxed: Option<(String, DocumentTax)> = listed.and_then(|row| {
            let rule = TaxRule::new(fraction(&row["rate"])?, fraction(&row["base"])?).ok()?;
            Some((
                code_label(&row),
                rule.document(integer(&order, "total"), inclusive).ok()?,
            ))
        });
        match taxed {
            Some((label, amounts)) => view! {<dl class="ops-totals" data-testid="invoice-order-tax">
                <dt>"Net"</dt><dd>{currency_amount(amounts.net,&currency)}</dd>
                <dt>{format!("Tax · {label} · {}",basis(inclusive))}</dt><dd>{currency_amount(amounts.tax,&currency)}</dd>
                <dt>"Total"</dt><dd>{currency_amount(amounts.gross,&currency)}</dd>
            </dl><p class="hint" data-testid="invoice-order-tax-notice">{DOCUMENT_NOTICE}" Calculated with the version that applies today; the invoice stores the version that applies on the day it is created."</p>}.into_any(),
            None => view! {<p class="refusal" data-testid="invoice-order-tax-missing">{format!("This order names tax code {code}, and no confirmed version of it applies today. The invoice is refused until a version that applies is entered and confirmed in Accounting. ")}{DOCUMENT_NOTICE}</p>}.into_any(),
        }
    })
}

/// The tax code and price basis an existing sales order names.
#[component]
pub fn OrderTax(order: Value) -> impl IntoView {
    order["taxCode"].as_str().map(|code| {
        let wording = if order["priceIncludesTax"] == true {
            "prices include the tax"
        } else {
            "tax is added to the prices on the invoice"
        };
        view! {<p class="hint" data-testid="order-tax">{format!("Tax code {code} · {wording} · the rate is taken when the order is invoiced. ")}{DOCUMENT_NOTICE}</p>}
    })
}

/// The tax fields of the new sales order form. Shown for a selected company
/// whose book has confirmed output codes that apply today; an empty choice
/// creates the order without tax, as before.
#[component]
pub fn OrderTaxFields(
    work: Workbench,
    code: RwSignal<String>,
    price_includes_tax: RwSignal<bool>,
) -> impl IntoView {
    let codes = Signal::derive(move || work.rows("taxCodes"));
    view! {<Show when=move||work.company.get().is_some()&&!codes.get().is_empty()>
        <label class="ops-field"><span>"Tax code (optional)"</span><select data-testid="order-tax-code" prop:value=move||code.get() on:change=move|e|code.set_if_alive(event_target_value(&e))>
            <option value="" prop:selected=move||code.get().is_empty()>"No tax"</option>
            {move||codes.get().into_iter().map(|row|{let id=text(&row,"code");let selected=id.clone();view!{<option value=id prop:selected=move||code.get()==selected>{code_label(&row)}</option>}}).collect::<Vec<_>>()}
        </select></label>
        <label class="ops-check" hidden=move||code.get().is_empty()><input type="checkbox" data-testid="order-price-includes-tax" prop:checked=move||price_includes_tax.get() on:change=move|e|price_includes_tax.set_if_alive(event_target_checked(&e))/><span>"Product prices already include this tax"</span></label>
        <p class="hint" data-testid="order-tax-notice">{DOCUMENT_NOTICE}" Left unticked, the tax is added to the prices when the order is invoiced. The invoice takes the version of the code that applies on its own date."</p>
    </Show>}
}

/// Adds the chosen tax code and price basis to a company order command.
pub fn order_tax_fields(command: &mut Value, code: &str, price_includes_tax: bool) {
    if !code.is_empty() && command["operation"] == "order.create_for_company" {
        command["taxCode"] = json!(code);
        command["priceIncludesTax"] = json!(price_includes_tax);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    fn snapshot(rate: (i64, i64), base: (i64, i64), inclusive: bool) -> Value {
        json!({"codeId":"c","code":"VAT","version":2,"kind":"output",
            "rate":{"numerator":rate.0,"denominator":rate.1},"base":{"numerator":base.0,"denominator":base.1},
            "priceIncludesTax":inclusive,"enteredByCompany":true,"verifiedByArchava":false})
    }
    fn loaded(data: Value, company: Option<Value>) -> Workbench {
        let work = Workbench::with_company(Domain::Commerce, RwSignal::new(company));
        work.data.set(data);
        work.ready.set(true);
        work
    }

    #[test]
    fn a_taxed_invoice_shows_net_tax_and_total_in_its_currency_with_the_company_notice() {
        Owner::new().with(|| {
            let idr = json!({"currency":"IDR","gross":33_300,"correction":0,"taxMinor":3_300,"correctionTaxMinor":0,"netMinor":30_000,
                "tax":snapshot((11,100),(1,1),false)});
            let html = view! {<InvoiceTax invoice=idr/>}.to_html();
            assert!(html.contains("data-testid=\"invoice-tax\""), "{html}");
            assert!(html.contains(">Rp 30.000<"), "{html}");
            assert!(html.contains(">Rp 3.300<"), "{html}");
            assert!(html.contains(">Rp 33.300<"), "{html}");
            assert!(html.contains("Tax · VAT 11% · added to the prices · version 2"), "{html}");
            assert!(html.contains("Tax codes are entered by the company; Archava does not verify tax rates or rules."), "{html}");
            assert!(!html.contains("0.11"), "{html}");

            // Minor units, a base fraction, an inclusive basis and a correction.
            let usd = json!({"currency":"USD","gross":1_109,"correction":-109,"taxMinor":110,"correctionTaxMinor":-11,"netMinor":999,
                "tax":snapshot((12,100),(11,12),true)});
            let html = view! {<InvoiceTax invoice=usd/>}.to_html();
            assert!(html.contains(">USD 9.01<"), "{html}");
            assert!(html.contains(">USD 0.99<"), "{html}");
            assert!(html.contains(">USD 10.00<"), "{html}");
            assert!(html.contains("Tax · VAT 12% of 11/12 of the price · included in the prices · version 2"), "{html}");
        });
    }
    /// What an absent optional view renders: the hydration placeholder, no
    /// element and no text.
    const EMPTY: &str = "<!>";
    #[test]
    fn an_untaxed_invoice_renders_no_tax_block() {
        Owner::new().with(|| {
            for invoice in [
                json!({"currency":"IDR","gross":30_000,"correction":0,"taxMinor":0,"correctionTaxMinor":0,"netMinor":30_000,"tax":null}),
                // A read from before the tax fields existed.
                json!({"currency":"IDR","gross":30_000,"correction":0}),
            ] {
                assert_eq!(view! {<InvoiceTax invoice/>}.to_html(), EMPTY);
            }
            let plain = json!({"id":"o","total":30_000,"currency":"IDR"});
            assert_eq!(view! {<OrderTax order=plain.clone()/>}.to_html(), EMPTY);
            assert_eq!(
                view! {<OrderTaxPreview order=plain codes=vec![]/>}.to_html(),
                EMPTY
            );
        });
    }
    #[test]
    fn the_invoice_preview_of_a_taxed_order_uses_the_code_that_applies_today_or_says_none_does() {
        Owner::new().with(|| {
            let codes = vec![json!({"code":"VAT","version":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1}})];
            let exclusive = json!({"id":"o","total":30_000,"currency":"IDR","taxCode":"VAT","priceIncludesTax":false});
            let html = view! {<OrderTaxPreview order=exclusive.clone() codes=codes.clone()/>}.to_html();
            assert!(html.contains(">Rp 30.000<") && html.contains(">Rp 3.300<") && html.contains(">Rp 33.300<"), "{html}");
            assert!(html.contains("Tax · VAT 11% · added to the prices"), "{html}");
            assert!(html.contains("Archava does not verify tax rates or rules."), "{html}");
            let inclusive = json!({"id":"o","total":999,"currency":"USD","taxCode":"VAT","priceIncludesTax":true});
            let html = view! {<OrderTaxPreview order=inclusive codes=codes/>}.to_html();
            assert!(html.contains(">USD 9.00<") && html.contains(">USD 0.99<") && html.contains(">USD 9.99<"), "{html}");
            assert!(html.contains("included in the prices"), "{html}");
            let html = view! {<OrderTaxPreview order=exclusive.clone() codes=vec![]/>}.to_html();
            assert!(html.contains("data-testid=\"invoice-order-tax-missing\""), "{html}");
            assert!(html.contains("no confirmed version of it applies today"), "{html}");
            let html = view! {<OrderTax order=exclusive/>}.to_html();
            assert!(html.contains("Tax code VAT · tax is added to the prices on the invoice"), "{html}");
            assert!(html.contains("Archava does not verify tax rates or rules."), "{html}");
        });
    }
    #[test]
    fn the_order_form_offers_the_companys_applicable_output_codes_and_a_price_basis() {
        Owner::new().with(|| {
            let code = RwSignal::new(String::new());
            let inclusive = RwSignal::new(false);
            let company = json!({"id":"c","revision":1,"active":true});
            let data = json!({"taxCodes":[
                {"code":"VAT","version":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1}},
                {"code":"VAT-DPP","version":3,"rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12}}]});
            let work = loaded(data.clone(), Some(company.clone()));
            let html = view! {<OrderTaxFields work code price_includes_tax=inclusive/>}.to_html();
            assert!(html.contains("data-testid=\"order-tax-code\""), "{html}");
            assert!(html.contains(">No tax<"), "{html}");
            assert!(html.contains(">VAT 11%<"), "{html}");
            assert!(html.contains(">VAT-DPP 12% of 11/12 of the price<"), "{html}");
            assert!(html.contains("data-testid=\"order-price-includes-tax\""), "{html}");
            assert!(html.contains("Tax codes are entered by the company; Archava does not verify tax rates or rules."), "{html}");
            // No company, or a company without an applicable code: no tax fields.
            let html = view! {<OrderTaxFields work=loaded(data, None) code price_includes_tax=inclusive/>}.to_html();
            assert!(!html.contains("order-tax-code"), "{html}");
            let html = view! {<OrderTaxFields work=loaded(json!({"taxCodes":[]}), Some(company)) code price_includes_tax=inclusive/>}.to_html();
            assert!(!html.contains("order-tax-code"), "{html}");
        });
    }
    #[test]
    fn only_a_company_order_with_a_chosen_code_carries_the_tax_fields() {
        let mut company = json!({"operation":"order.create_for_company","lines":[]});
        order_tax_fields(&mut company, "", true);
        assert_eq!(
            company,
            json!({"operation":"order.create_for_company","lines":[]})
        );
        order_tax_fields(&mut company, "VAT", false);
        assert_eq!(company["taxCode"], "VAT");
        assert_eq!(company["priceIncludesTax"], false);
        let mut plain = json!({"operation":"order.create","lines":[]});
        order_tax_fields(&mut plain, "VAT", true);
        assert_eq!(plain, json!({"operation":"order.create","lines":[]}));
    }
}
