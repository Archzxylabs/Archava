//! Tax on a sales order and on the invoice made from it (TAX.md stage B),
//! and on a supplier document (stage D).
//! The order names a tax code and a price basis; the invoice stores the
//! version it was taxed with. A supplier document names an input tax code and
//! the tax amount the supplier printed. Every amount is the server's integer,
//! shown in the document's currency; the only arithmetic here is the preview
//! of a document that does not exist yet, with the domain's own rule.
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

/// Said under the tax ids and the tax invoice reference of an invoice.
const IDENTIFIER_NOTICE: &str = "The tax ids are those the company and the customer party had when this invoice was created; later changes to either do not change the invoice. The tax invoice reference is a number issued outside Archava, recorded as entered. Archava does not verify tax ids or tax invoice references.";

/// The recorded tax invoice reference in words, or that there is none.
fn reference_line(invoice: &Value) -> String {
    let stored = &invoice["taxReference"];
    match stored["reference"].as_str() {
        Some(reference) => {
            let code = stored["transactionCode"]
                .as_str()
                .map(|code| format!(" · transaction code {code}"))
                .unwrap_or_default();
            format!("{reference}{code} · entry {}", integer(stored, "version"))
        }
        None => "Not recorded".to_string(),
    }
}
/// The tax ids a taxed invoice stored at creation and its tax invoice
/// reference. Renders nothing for an untaxed invoice.
fn invoice_identifiers(invoice: &Value) -> AnyView {
    if !invoice["tax"].is_object() {
        return ().into_any();
    }
    let id = |key: &str| {
        invoice[key]
            .as_str()
            .map_or_else(|| "None stored".to_string(), str::to_owned)
    };
    view! {<dl class="ops-totals" data-testid="invoice-tax-identifiers">
        <dt>"Seller tax id"</dt><dd data-testid="invoice-seller-tax-id">{id("sellerTaxId")}</dd>
        <dt>"Buyer tax id"</dt><dd data-testid="invoice-buyer-tax-id">{id("buyerTaxId")}</dd>
        <dt>"Tax invoice reference"</dt><dd data-testid="invoice-tax-reference">{reference_line(invoice)}</dd>
    </dl><p class="hint" data-testid="invoice-tax-identifiers-notice">{IDENTIFIER_NOTICE}</p>}
    .into_any()
}
/// The command that records, or replaces, the tax invoice reference of an
/// invoice. A blank transaction code is left out; a replacement carries its
/// reason.
pub fn tax_reference_command(
    invoice: &Value,
    reference: &str,
    transaction_code: &str,
    reason: &str,
) -> Value {
    let replace = invoice["taxReference"].is_object();
    let mut command = json!({
        "type": if replace {"replace_tax_reference"} else {"record_tax_reference"},
        "invoiceId": invoice["id"],
        "expectedRevision": invoice["revision"],
        "reference": reference.trim(),
    });
    if !transaction_code.trim().is_empty() {
        command["transactionCode"] = json!(transaction_code.trim());
    }
    if replace {
        command["reason"] = json!(reason);
    }
    command
}
/// Whether the reference form is offered: a taxed invoice that is issued or
/// settled.
fn takes_reference(invoice: &Value) -> bool {
    invoice["tax"].is_object() && matches!(invoice["status"].as_str(), Some("issued" | "settled"))
}

/// The tax ids and the tax invoice reference of a taxed invoice, with the
/// form that records the reference (or replaces it, with a reason) for a
/// person who may change an issued invoice. Renders nothing for an untaxed
/// invoice.
#[component]
pub fn InvoiceTaxReference(work: Workbench, invoice: Value) -> impl IntoView {
    let reference = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let reason = RwSignal::new(String::new());
    let shown = invoice_identifiers(&invoice);
    let form = takes_reference(&invoice).then(|| {
        let replace = invoice["taxReference"].is_object();
        let row = invoice.clone();
        let submit = move |e: leptos::ev::SubmitEvent| {
            e.prevent_default();
            if reference.get_untracked().trim().is_empty() {
                work.fail("Enter the tax invoice reference as it was issued.");
                return;
            }
            work.send(tax_reference_command(
                &row,
                &reference.get_untracked(),
                &code.get_untracked(),
                &reason.get_untracked(),
            ));
        };
        view! {<div hidden=move||!work.permits("billing.settle")><form class="ops-form" data-testid="invoice-tax-reference-form" on:submit=submit>
            <Field label="Tax invoice reference issued outside Archava" id="invoice-tax-reference-input" value=reference/>
            <Field label="Transaction type code (optional, your own classification)" id="invoice-tax-transaction-code" value=code required=false/>
            {replace.then(||view!{<Field label="Reason for replacing the reference" id="invoice-tax-reference-reason" value=reason/>})}
            <button type="submit" class="btn secondary" data-testid="invoice-tax-reference-save" disabled=move||work.blocked()>{if replace {"Replace tax invoice reference"} else {"Record tax invoice reference"}}</button>
            <p class="hint">{if replace {"The earlier reference stays in the billing audit trail."} else {"Archava never generates this number. Once recorded it can only be replaced, with a reason."}}</p>
        </form></div>}
    });
    view! { {shown} {form} }
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

/// The sentence beside every supplier tax field and amount (rules 1 and 7).
const SUPPLIER_NOTICE: &str = " Enter the tax amount printed on the supplier's document: that amount is the one owed and posted, shown beside the amount the code gives on the goods total. The goods total and the claimed prices stay without tax.";

/// The distinct input tax codes the Purchasing workspace lists, in its order.
fn input_codes(codes: &[Value]) -> Vec<String> {
    let mut names = Vec::<String>::new();
    for row in codes {
        let code = text(row, "code");
        if !code.is_empty() && !names.contains(&code) {
            names.push(code);
        }
    }
    names
}
/// The version of `code` that applies on `day` (`YYYY-MM-DD`): the server
/// stores the version that applies on the supplier document's own date.
fn input_version_on<'a>(codes: &'a [Value], code: &str, day: &str) -> Option<&'a Value> {
    codes.iter().find(|row| {
        row["code"] == code
            && row["effectiveFrom"]
                .as_str()
                .is_some_and(|from| from <= day)
            && row["effectiveTo"].as_str().is_none_or(|to| day <= to)
    })
}
/// What the supplier claim form says about the chosen tax code before the
/// claim is saved: the version that applies on the document date and, once
/// the goods total and the supplier's tax are entered, the computed tax, the
/// entered tax, the difference and what is owed. `Err` is a refusal the server
/// would give. `None` without a chosen code.
pub fn supplier_tax_preview(
    codes: &[Value],
    code: &str,
    day: &str,
    currency: &str,
    goods: Option<i64>,
    entered: Option<i64>,
) -> Option<Result<String, String>> {
    if code.is_empty() {
        return None;
    }
    if day.is_empty() {
        return Some(Ok(format!(
            "Enter the vendor document date: the version of {code} that applies on that date is used."
        )));
    }
    let Some(version) = input_version_on(codes, code, day) else {
        return Some(Err(format!(
            "No confirmed version of tax code {code} applies on {day}. The claim is refused until the date is corrected or a version that applies is entered and confirmed in Accounting."
        )));
    };
    let label = format!(
        "{} · version {}",
        code_label(version),
        integer(version, "version")
    );
    let rule = fraction(&version["rate"])
        .zip(fraction(&version["base"]))
        .and_then(|(rate, base)| TaxRule::new(rate, base).ok());
    let (Some(rule), Some(goods)) = (rule, goods.filter(|goods| *goods >= 0)) else {
        return Some(Ok(format!(
            "{label} applies on {day}. Enter the goods total to see the tax it gives."
        )));
    };
    let Ok((computed, _)) = rule.exclusive(goods) else {
        return Some(Err("The goods total is too large to tax.".into()));
    };
    let Some(entered) = entered else {
        return Some(Ok(format!(
            "{label} applies on {day} · computed tax {}. Enter the tax amount printed on the supplier's document.",
            currency_amount(computed, currency)
        )));
    };
    Some(match rule.entered(goods, entered) {
        Ok(tax) => Ok(format!(
            "{label} applies on {day} · computed tax {} · entered tax {} · difference {} · owed to the supplier {}",
            currency_amount(tax.computed, currency),
            currency_amount(tax.entered, currency),
            currency_amount(tax.difference, currency),
            currency_amount(tax.total, currency)
        )),
        Err(_) => Err(
            "The tax amount cannot be negative or more than the goods total.".to_string(),
        ),
    })
}

/// The tax fields of the supplier claim form and of its correction: the
/// company's input tax codes and the supplier's tax amount in the document's
/// currency, with the computed amount beside it. Shown when the company's
/// book has input codes (or the claim being corrected already names one); an
/// empty choice saves the claim without tax, as before.
#[component]
pub fn SupplierTaxFields(
    work: Workbench,
    code: RwSignal<String>,
    amount: RwSignal<String>,
    total: RwSignal<String>,
    day: RwSignal<String>,
    currency: String,
    edit: bool,
) -> impl IntoView {
    let codes = Signal::derive(move || work.rows("taxCodes"));
    let currency = StoredValue::new(currency);
    let preview = move || {
        let cur = currency.get_value();
        supplier_tax_preview(
            &codes.get(),
            &code.get(),
            &day.get(),
            &cur,
            trade_money::parse_trade(&cur, &total.get()).ok(),
            trade_money::parse_trade(&cur, &amount.get()).ok(),
        )
    };
    view! {<Show when=move||!codes.get().is_empty()||!code.get_untracked().is_empty()>
        <label class="ops-field"><span>"Input tax code (optional)"</span><select data-testid=if edit{"supplier-doc-edit-tax-code"}else{"supplier-doc-tax-code"} prop:value=move||code.get() on:change=move|e|code.set_if_alive(event_target_value(&e))>
            <option value="" prop:selected=move||code.get().is_empty()>"No tax"</option>
            {move||{let mut names=input_codes(&codes.get());let chosen=code.get_untracked();if !chosen.is_empty()&&!names.contains(&chosen){names.push(chosen)}names.into_iter().map(|name|{let selected=name.clone();let shown=name.clone();view!{<option value=name prop:selected=move||code.get()==selected>{shown}</option>}}).collect::<Vec<_>>()}}
        </select></label>
        <div hidden=move||code.get().is_empty()><TradeField label="Tax amount on the supplier's document" id=if edit{"supplier-doc-edit-tax"}else{"supplier-doc-tax"} value=amount currency=Signal::stored(currency.get_value()) required=false/></div>
        {move||preview().map(|outcome|match outcome{
            Ok(line)=>view!{<p class="hint" data-testid="supplier-doc-tax-preview">{line}</p>}.into_any(),
            Err(line)=>view!{<p class="refusal" data-testid="supplier-doc-tax-refusal">{line}</p>}.into_any(),
        })}
        <p class="hint" data-testid="supplier-doc-tax-notice">{DOCUMENT_NOTICE}{SUPPLIER_NOTICE}</p>
    </Show>}
}

/// Adds the chosen input tax code and the supplier's tax amount to a claim
/// command; without a code the command is the one it was.
pub fn supplier_tax_fields(command: &mut Value, code: &str, amount: i64) {
    if !code.is_empty() {
        command["taxCode"] = json!(code);
        command["taxMinor"] = json!(amount);
    }
}

/// The goods, tax and amount owed of a taxed supplier document, with the tax
/// code version the document stored, the tax that version gives on the goods
/// total and the difference to what the supplier printed. Renders nothing for
/// a document without tax.
#[component]
pub fn SupplierTax(document: Value) -> impl IntoView {
    let snapshot = document["tax"].clone();
    snapshot.is_object().then(|| {
        let currency = currency_of(&document);
        let label = format!(
            "Tax on the supplier's document · {} · version {}",
            code_label(&snapshot),
            integer(&snapshot, "version")
        );
        view! {<dl class="ops-totals" data-testid="supplier-document-tax">
            <dt>"Goods (without tax)"</dt><dd data-testid="supplier-document-tax-net">{row_money(&document,"declaredTotal")}</dd>
            <dt>{label}</dt><dd data-testid="supplier-document-tax-entered">{currency_amount(integer(&snapshot,"enteredMinor"),&currency)}</dd>
            <dt>"Tax the code gives on the goods"</dt><dd data-testid="supplier-document-tax-computed">{currency_amount(integer(&snapshot,"computedMinor"),&currency)}</dd>
            <dt>"Difference (entered less computed)"</dt><dd data-testid="supplier-document-tax-difference">{currency_amount(integer(&snapshot,"differenceMinor"),&currency)}</dd>
            <dt>"Owed to the supplier"</dt><dd data-testid="supplier-document-tax-total">{currency_amount(integer(&snapshot,"totalMinor"),&currency)}</dd>
        </dl><p class="hint" data-testid="supplier-document-tax-notice">{DOCUMENT_NOTICE}" The entered amount is the one owed and posted. Matching compares goods only."</p>}
    })
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
    #[test]
    fn a_taxed_invoice_shows_its_stored_tax_ids_and_reference_with_the_form() {
        Owner::new().with(|| {
            let work = loaded(json!({"permissions":["billing.read","billing.settle"]}), None);
            let mut invoice = json!({"id":"i1","revision":2,"status":"issued","currency":"IDR","gross":33_300,"correction":0,"taxMinor":3_300,"correctionTaxMinor":0,
                "tax":snapshot((11,100),(1,1),false),"sellerTaxId":"01.234.567.8-999.000","buyerTaxId":"<02>","taxReference":null});
            let html = view! {<InvoiceTaxReference work invoice=invoice.clone()/>}.to_html();
            assert!(html.contains("data-testid=\"invoice-tax-identifiers\""), "{html}");
            assert!(html.contains(">01.234.567.8-999.000<"), "{html}");
            assert!(html.contains(">&lt;02&gt;<"), "{html}");
            assert!(html.contains("data-testid=\"invoice-tax-reference\">Not recorded<"), "{html}");
            assert!(html.contains("Archava does not verify tax ids or tax invoice references."), "{html}");
            assert!(html.contains("later changes to either do not change the invoice"), "{html}");
            assert!(html.contains("Record tax invoice reference"), "{html}");
            assert!(html.contains("Archava never generates this number."), "{html}");
            assert!(!html.contains("invoice-tax-reference-reason"), "{html}");
            assert_eq!(
                tax_reference_command(&invoice, " 010.000-26.00000001 ", "  ", "ignored"),
                json!({"type":"record_tax_reference","invoiceId":"i1","expectedRevision":2,"reference":"010.000-26.00000001"})
            );

            // A recorded reference is shown with its code and is replaced with a reason.
            invoice["taxReference"] = json!({"reference":"010.000-26.00000001","transactionCode":"01","version":1});
            invoice["buyerTaxId"] = Value::Null;
            invoice["status"] = json!("settled");
            let html = view! {<InvoiceTaxReference work invoice=invoice.clone()/>}.to_html();
            assert!(html.contains("data-testid=\"invoice-buyer-tax-id\">None stored<"), "{html}");
            assert!(html.contains(">010.000-26.00000001 · transaction code 01 · entry 1<"), "{html}");
            assert!(html.contains("Replace tax invoice reference"), "{html}");
            assert!(html.contains("invoice-tax-reference-reason"), "{html}");
            assert!(html.contains("The earlier reference stays in the billing audit trail."), "{html}");
            assert_eq!(
                tax_reference_command(&invoice, "010.000-26.00000002", "02", "Mistyped"),
                json!({"type":"replace_tax_reference","invoiceId":"i1","expectedRevision":2,"reference":"010.000-26.00000002","transactionCode":"02","reason":"Mistyped"})
            );

            // A draft or voided taxed invoice shows what it stored and offers no form.
            for status in ["draft", "void"] {
                invoice["status"] = json!(status);
                let html = view! {<InvoiceTaxReference work invoice=invoice.clone()/>}.to_html();
                assert!(html.contains("invoice-tax-identifiers"), "{html}");
                assert!(!html.contains("invoice-tax-reference-form"), "{html}");
            }
            for word in ["compliant", "official", "Coretax", "e-Faktur", "verified by"] {
                assert!(!html.contains(word), "{word}");
            }
        });
    }
    #[test]
    fn an_untaxed_invoice_shows_no_tax_ids_and_no_reference_form() {
        Owner::new().with(|| {
            let work = loaded(json!({"permissions":["billing.settle"]}), None);
            for invoice in [
                json!({"id":"i","revision":1,"status":"issued","currency":"IDR","gross":30_000,"correction":0,"tax":null,"sellerTaxId":null,"buyerTaxId":null,"taxReference":null}),
                json!({"id":"i","revision":1,"status":"issued","currency":"IDR","gross":30_000,"correction":0}),
            ] {
                let html = view! {<InvoiceTaxReference work invoice/>}.to_html();
                assert!(!html.contains("invoice-tax"), "{html}");
                assert!(!html.contains("<form") && !html.contains("<dl"), "{html}");
            }
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
    fn input_rows() -> Vec<Value> {
        vec![
            json!({"code":"VAT-IN","version":2,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},"effectiveFrom":"2026-01-01","effectiveTo":null}),
            json!({"code":"VAT-IN","version":1,"rate":{"numerator":10,"denominator":100},"base":{"numerator":1,"denominator":1},"effectiveFrom":"2020-01-01","effectiveTo":"2025-12-31"}),
            json!({"code":"GST","version":1,"rate":{"numerator":43,"denominator":400},"base":{"numerator":1,"denominator":1},"effectiveFrom":"2020-01-01","effectiveTo":null}),
        ]
    }
    #[test]
    fn the_claim_preview_uses_the_version_on_the_document_date_and_sets_entered_beside_computed() {
        let codes = input_rows();
        let preview = |code: &str, day: &str, currency: &str, goods, entered| {
            supplier_tax_preview(&codes, code, day, currency, goods, entered)
        };
        assert_eq!(
            preview("", "2026-11-14", "IDR", Some(140_000), Some(15_400)),
            None
        );
        // The version is chosen by the document's date, not by today.
        let now = preview("VAT-IN", "2026-11-14", "IDR", Some(140_000), Some(15_399))
            .unwrap()
            .unwrap();
        assert_eq!(now, "VAT-IN 11% · version 2 applies on 2026-11-14 · computed tax Rp 15.400 · entered tax Rp 15.399 · difference −Rp 1 · owed to the supplier Rp 155.399");
        let earlier = preview("VAT-IN", "2025-12-31", "IDR", Some(140_000), Some(14_000))
            .unwrap()
            .unwrap();
        assert!(
            earlier.contains("VAT-IN 10% · version 1 applies on 2025-12-31"),
            "{earlier}"
        );
        assert!(
            earlier.contains("computed tax Rp 14.000") && earlier.contains("difference Rp 0"),
            "{earlier}"
        );
        // Minor units, rounded once half away from zero: 10.75% of 70.00 is 7.525.
        let usd = preview("GST", "2026-11-14", "USD", Some(7_000), Some(752))
            .unwrap()
            .unwrap();
        assert!(usd.contains("computed tax USD 7.53"), "{usd}");
        assert!(usd.contains("entered tax USD 7.52"), "{usd}");
        assert!(usd.contains("owed to the supplier USD 77.52"), "{usd}");
        // Before the amounts are entered the preview still names the version.
        let waiting = preview("GST", "2026-11-14", "USD", Some(7_000), None)
            .unwrap()
            .unwrap();
        assert!(
            waiting.contains("computed tax USD 7.53") && waiting.contains("Enter the tax amount"),
            "{waiting}"
        );
        assert!(preview("GST", "", "USD", None, None)
            .unwrap()
            .unwrap()
            .contains("Enter the vendor document date"));
        // What the server refuses is said before saving.
        let none = preview("VAT-IN", "2019-12-31", "IDR", Some(140_000), Some(15_400))
            .unwrap()
            .unwrap_err();
        assert!(
            none.contains("No confirmed version of tax code VAT-IN applies on 2019-12-31"),
            "{none}"
        );
        let above = preview("VAT-IN", "2026-11-14", "IDR", Some(140_000), Some(140_001))
            .unwrap()
            .unwrap_err();
        assert!(above.contains("more than the goods total"), "{above}");
        assert_eq!(
            input_codes(&codes),
            vec!["VAT-IN".to_owned(), "GST".to_owned()]
        );
    }
    #[test]
    fn a_taxed_supplier_document_shows_goods_entered_and_computed_tax_and_what_is_owed() {
        Owner::new().with(|| {
            let tax = |entered: i64, computed: i64, total: i64| json!({"codeId":"c","code":"VAT-IN","version":1,"kind":"input",
                "rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
                "enteredMinor":entered,"computedMinor":computed,"differenceMinor":entered-computed,"totalMinor":total,
                "enteredByCompany":true,"verifiedByArchava":false});
            let idr = json!({"currency":"IDR","declaredTotal":140_000,"tax":tax(15_399,15_400,155_399)});
            let html = view! {<SupplierTax document=idr/>}.to_html();
            assert!(html.contains("data-testid=\"supplier-document-tax\""), "{html}");
            assert!(html.contains(">Rp 140.000<"), "{html}");
            assert!(html.contains(">Rp 15.399<"), "{html}");
            assert!(html.contains(">Rp 15.400<"), "{html}");
            assert!(html.contains(">−Rp 1<"), "{html}");
            assert!(html.contains(">Rp 155.399<"), "{html}");
            assert!(html.contains("s document · VAT-IN 11% · version 1<"), "{html}");
            assert!(html.contains("Tax codes are entered by the company; Archava does not verify tax rates or rules."), "{html}");
            let usd = json!({"currency":"USD","declaredTotal":7_000,"tax":tax(752,753,7_752)});
            let html = view! {<SupplierTax document=usd/>}.to_html();
            assert!(html.contains(">USD 70.00<") && html.contains(">USD 7.52<") && html.contains(">USD 7.53<") && html.contains(">USD 77.52<"), "{html}");
            // A document without tax, and a read from before the tax fields.
            for document in [
                json!({"currency":"IDR","declaredTotal":140_000}),
                json!({"currency":"IDR","declaredTotal":140_000,"tax":null}),
            ] {
                assert_eq!(view! {<SupplierTax document/>}.to_html(), EMPTY);
            }
        });
    }
    #[test]
    fn only_a_claim_with_a_chosen_code_carries_the_tax_members() {
        let plain = json!({"operation":"document.capture","declaredTotal":140_000});
        let mut command = plain.clone();
        supplier_tax_fields(&mut command, "", 15_400);
        assert_eq!(command, plain);
        supplier_tax_fields(&mut command, "VAT-IN", 15_400);
        assert_eq!(command["taxCode"], "VAT-IN");
        assert_eq!(command["taxMinor"], 15_400);
        assert_eq!(command["declaredTotal"], 140_000);
    }
}
