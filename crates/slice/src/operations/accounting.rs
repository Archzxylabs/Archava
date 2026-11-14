use super::operator::details;
mod agent;
mod aging;
mod bank;
mod bank_check;
mod clearing;
mod close_check;
mod inventory_check;
mod payables;
mod tax_codes;
mod tax_report;
mod terms;
use super::*;
pub(crate) use agent::{agent_references, agent_step_ready, agent_step_review};
pub(super) use bank_check::BankCheckField;
pub(super) use terms::{source as terms_source, ConditionField};
pub(super) fn decimal(minor: &str, exponent: u8) -> Option<String> {
    let (sign, digits) = minor
        .strip_prefix('-')
        .map(|s| ("-", s))
        .unwrap_or(("", minor));
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || exponent > 4 {
        return None;
    }
    if exponent == 0 {
        return Some(format!("{sign}{digits}"));
    }
    let padded = format!("{:0>width$}", digits, width = usize::from(exponent) + 1);
    let i = padded.len() - usize::from(exponent);
    Some(format!("{sign}{}.{}", &padded[..i], &padded[i..]))
}
pub(super) fn parse_amount(raw: &str, exponent: u8) -> Option<String> {
    if exponent > 4 || raw.trim() != raw || raw.is_empty() {
        return None;
    }
    let mut pieces = raw.split('.');
    let whole = pieces.next()?;
    let fraction = pieces.next().unwrap_or_default();
    if pieces.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > usize::from(exponent)
    {
        return None;
    }
    let digits = format!("{whole}{fraction:0<width$}", width = usize::from(exponent));
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    archava_domain::erp::accounting::minor(digits).ok()?;
    Some(digits.into())
}
fn parse_money(raw: &str, exponent: u8, signed: bool) -> Option<String> {
    if signed {
        if let Some(digits) = raw.strip_prefix('-') {
            let minor = parse_amount(digits, exponent)?;
            return (minor != "0").then(|| format!("-{minor}"));
        }
    }
    parse_amount(raw, exponent)
}
fn amount(v: &Value, book: &Value) -> String {
    let currency = book["currency"].as_str().unwrap_or("Currency");
    let exp = book["minorUnits"]
        .as_u64()
        .and_then(|v| u8::try_from(v).ok());
    match (v.as_str(), exp) {
        (Some(v), Some(exp)) => decimal(v, exp)
            .map(|v| format!("{currency} {v}"))
            .unwrap_or_else(|| "Amount unavailable".into()),
        _ => "Amount unavailable".into(),
    }
}
pub(super) fn preview(mut body: Value, book: &Value) -> Value {
    let command = if body["command"].is_object() {
        &mut body["command"]
    } else {
        &mut body
    };
    if let Some(lines) = command["lines"].as_array_mut() {
        for line in lines {
            if let Some(o) = line.as_object_mut() {
                for (key, label) in [("debitMinor", "debit"), ("creditMinor", "credit")] {
                    if let Some(v) = o.remove(key) {
                        o.insert(label.into(), json!(amount(&v, book)));
                    }
                }
            }
        }
    }
    body
}
/// The source unit a book's posting policies carry (trade currency rule 5):
/// `whole_idr` on an IDR book with two minor units, `minor` on a book in any
/// other currency. The database refuses every other pairing, so the book
/// decides it. `None` when the book metadata is missing or malformed.
fn book_source_unit(book: &Value) -> Option<&'static str> {
    let exponent = book["minorUnits"].as_u64()?;
    match book["currency"].as_str()? {
        "IDR" => (exponent == 2).then_some("whole_idr"),
        code if code.len() == 3
            && code.bytes().all(|b| b.is_ascii_uppercase())
            && exponent <= 4 =>
        {
            Some("minor")
        }
        _ => None,
    }
}
/// [`book_source_unit`] of the book the operator workspace has configured,
/// read from the form references.
pub(super) fn policy_source_unit(refs: &Value) -> Option<&'static str> {
    book_source_unit(&json!({"currency":refs["__currency"],"minorUnits":refs["__minorUnits"]}))
}
/// Whether a captured source can be reviewed against `book`: the unit in its
/// policy snapshot must be the book's. A snapshot without a unit predates the
/// `minor` unit and is whole Rupiah. Captured amounts are already book minor
/// units under either unit, so the review formats them with the book.
fn captured_unit_fits(book: &Value, source: &Value) -> bool {
    match (
        book_source_unit(book),
        source["policy"]["source_unit"].as_str(),
    ) {
        (Some(unit), Some(captured)) => unit == captured,
        (Some(unit), None) => unit == "whole_idr",
        (None, _) => false,
    }
}
/// How the captured source reached book units, for the reference details.
fn source_unit_caption(book: &Value, source: &Value) -> String {
    match book_source_unit(book).filter(|_| captured_unit_fits(book, source)) {
        Some("whole_idr") => "whole Rupiah converted exactly to IDR book units".into(),
        Some(_) => format!(
            "{0} minor units posted unchanged as {0} book units",
            text(book, "currency")
        ),
        None => "Source unit unavailable for this book".into(),
    }
}
pub(super) fn source_reference<'a>(refs: &'a Value, id: &str, action: &str) -> Option<&'a Value> {
    refs[match action {
        "purchase_source.draft" => "accounting/purchase_sources",
        "stock_source.draft" => "accounting/stock_sources",
        "invoice_source.draft" => "accounting/invoice_sources",
        _ => return None,
    }]
    .as_array()?
    .iter()
    .find(|r| r["id"] == id)
}
pub(super) fn reviewed_preview(body: Value, book: &Value, refs: &Value) -> Value {
    let mut display = preview(body, book);
    if display["type"] == "invoice_source.draft" {
        if let Some(source) = display["sourceId"]
            .as_str()
            .and_then(|id| source_reference(refs, id, "invoice_source.draft"))
        {
            display["reviewedSource"] = json!({"invoice":source["invoiceCode"],"effect":source["effect"],"bookDate":source["sourceDate"],"amount":amount(&source["amountMinor"],book),"policyRevision":source["policyRevision"],"sourceUnit":source_unit_caption(book,source),"collectionAccount":"Verified provider money uses payment clearing; bank settlement is separate"});
            // A source that carries tax states its gross, net and tax and
            // the tax control account captured with it.
            if let Some((net, tax)) = source_tax_split(source) {
                let reviewed = &mut display["reviewedSource"];
                reviewed["gross"] = json!(amount(&source["amountMinor"], book));
                reviewed["net"] = json!(amount(&json!(net.to_string()), book));
                reviewed["tax"] = json!(amount(&json!(tax.to_string()), book));
                reviewed["taxAccount"] = json!({"accountId":source["taxAccountId"],"expectedAccountRevision":source["taxAccountRevision"]});
            }
        }
    }
    if display["type"] == "purchase_source.draft" {
        if let Some(source) = display["sourceId"]
            .as_str()
            .and_then(|id| source_reference(refs, id, "purchase_source.draft"))
        {
            display["reviewedSource"] = json!({"order":source["orderId"],"supplierDocument":source["documentId"],"effect":source["effect"],"bookDate":source["sourceDate"],"goods":amount(&source["goodsMinor"],book),"landedFee":amount(&source["landedFeeMinor"],book),"policyRevision":source["policyRevision"],"accountMappings":source["policy"]["mappings"],"notice":"Amounts and mappings come from the captured operating source. Supplier bank payment is separate; unclaimed landed fees use a separate accrual."});
            // A bill or void that carries tax states its tax, the total owed
            // and the tax control account captured with it; goods stay net.
            if let Some((tax, total)) = purchase_tax_split(source) {
                let reviewed = &mut display["reviewedSource"];
                reviewed["tax"] = json!(amount(&json!(tax.to_string()), book));
                reviewed["total"] = json!(amount(&json!(total.to_string()), book));
                reviewed["taxAccount"] = json!({"accountId":source["taxAccountId"],"expectedAccountRevision":source["taxAccountRevision"]});
            }
        }
    }
    display
}
/// The net and tax (book minor units) of a captured invoice source that
/// carries tax; `None` for a source without tax, which is reviewed as before.
fn source_tax_split(source: &Value) -> Option<(i64, i64)> {
    let tax: i64 = source["taxMinor"].as_str()?.parse().ok()?;
    let gross: i64 = source["amountMinor"].as_str()?.parse().ok()?;
    (tax != 0).then_some((gross.checked_sub(tax)?, tax))
}
/// The tax and the total owed (goods plus tax, book minor units) of a
/// captured purchase source that carries tax; `None` for a source without
/// tax, which is reviewed as before.
fn purchase_tax_split(source: &Value) -> Option<(i64, i64)> {
    let tax: i64 = source["taxMinor"].as_str()?.parse().ok()?;
    let goods: i64 = source["goodsMinor"].as_str()?.parse().ok()?;
    (tax != 0).then_some((tax, goods.checked_add(tax)?))
}
struct PurchaseReviewLine {
    role: String,
    account: String,
    debit: String,
    credit: String,
}
struct InvoiceReview {
    effect: String,
    invoice: String,
    amount: String,
    date: String,
    policy_revision: String,
    /// Net and tax of a source that carries tax, formatted for the book.
    tax: Option<(String, String)>,
    lines: Vec<PurchaseReviewLine>,
    accounts_match: bool,
}
fn invoice_review(body: &Value, book: &Value, refs: &Value) -> Option<InvoiceReview> {
    use archava_domain::erp::accounting::sources::{taxed_legs, InvoiceLegAccount, InvoiceRole};
    let source = source_reference(refs, body["sourceId"].as_str()?, "invoice_source.draft")?;
    if !captured_unit_fits(book, source) {
        return None;
    }
    let effect = source["effect"].as_str()?;
    let split = source_tax_split(source);
    let legs = taxed_legs(
        effect,
        source["amountMinor"].as_str()?.parse().ok()?,
        split.map_or(0, |(_, tax)| tax),
    )
    .ok()?;
    let selected = body["accounts"].as_array()?;
    let mappings = source["policy"]["mappings"].as_array()?;
    let mut valid = !legs.is_empty() && selected.len() == legs.len();
    let mut expected = std::collections::BTreeSet::new();
    let lines = legs
        .into_iter()
        .map(|leg| {
            let id = match leg.account {
                InvoiceLegAccount::Policy(role) => mappings
                    .iter()
                    .find(|m| m["role"] == role.as_str())
                    .and_then(|m| m["accountId"].as_str()),
                InvoiceLegAccount::TaxControl => source["taxAccountId"].as_str(),
            };
            let choice = selected.iter().find(|a| a["accountId"].as_str() == id);
            let account = refs["accounting/accounts"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| {
                    a["id"].as_str() == id
                        && choice.is_some_and(|c| c["expectedAccountRevision"] == a["revision"])
                });
            valid &= id.is_some_and(|id| expected.insert(id.to_owned()))
                && choice.is_some()
                && account.is_some();
            PurchaseReviewLine {
                role: match leg.account {
                    InvoiceLegAccount::Policy(InvoiceRole::Receivable) => "Customer receivable",
                    InvoiceLegAccount::Policy(InvoiceRole::Revenue) => "Sales revenue",
                    InvoiceLegAccount::Policy(InvoiceRole::PaymentClearing) => "Payment clearing",
                    InvoiceLegAccount::TaxControl => "Output tax",
                }
                .into(),
                account: account
                    .map(|a| format!("{} · {}", text(a, "code"), text(a, "name")))
                    .unwrap_or_else(|| "Original account details unavailable".into()),
                debit: amount(&json!(leg.debit_minor), book),
                credit: amount(&json!(leg.credit_minor), book),
            }
        })
        .collect();
    Some(InvoiceReview {
        effect: effect.into(),
        invoice: text(source, "invoiceCode"),
        amount: amount(&source["amountMinor"], book),
        date: text(source, "sourceDate"),
        policy_revision: source["policyRevision"].to_string(),
        tax: split.map(|(net, tax)| {
            (
                amount(&json!(net.to_string()), book),
                amount(&json!(tax.to_string()), book),
            )
        }),
        lines,
        accounts_match: valid,
    })
}
struct PurchaseReview {
    effect: &'static str,
    goods: String,
    fee: String,
    date: String,
    policy_revision: String,
    /// Tax and total owed of a source that carries tax, formatted for the book.
    tax: Option<(String, String)>,
    lines: Vec<PurchaseReviewLine>,
    accounts_match: bool,
}
fn purchase_review(body: &Value, book: &Value, refs: &Value) -> Option<PurchaseReview> {
    use archava_domain::erp::accounting::purchase_sources::{
        taxed_legs, PurchaseEffect, PurchaseLegAccount, PurchaseRole,
    };
    let source = source_reference(refs, body["sourceId"].as_str()?, "purchase_source.draft")?;
    if !captured_unit_fits(book, source) {
        return None;
    }
    let effect: PurchaseEffect = serde_json::from_value(source["effect"].clone()).ok()?;
    let split = purchase_tax_split(source);
    let legs = taxed_legs(
        effect,
        source["goodsMinor"].as_str()?.parse().ok()?,
        source["landedFeeMinor"].as_str()?.parse().ok()?,
        split.map_or(0, |(tax, _)| tax),
    )
    .ok()?;
    let selected = body["accounts"].as_array()?;
    let mappings = source["policy"]["mappings"].as_array()?;
    let mut accounts_match = !legs.is_empty() && legs.len() == selected.len();
    let mut expected_ids = std::collections::BTreeSet::new();
    let lines = legs
        .into_iter()
        .map(|leg| {
            let id = match leg.account {
                PurchaseLegAccount::Policy(role) => mappings
                    .iter()
                    .find(|m| m["role"] == role.as_str())
                    .and_then(|m| m["accountId"].as_str()),
                PurchaseLegAccount::TaxControl => source["taxAccountId"].as_str(),
            };
            if let Some(id) = id {
                accounts_match &= expected_ids.insert(id.to_owned());
            }
            let selected = selected.iter().find(|a| a["accountId"].as_str() == id);
            accounts_match &= id.is_some() && selected.is_some();
            let account = refs["accounting/accounts"]
                .as_array()
                .and_then(|rows| rows.iter().find(|a| a["id"].as_str() == id))
                .filter(|a| selected.is_some_and(|s| a["revision"] == s["expectedAccountRevision"]))
                .map(|a| format!("{} · {}", text(a, "code"), text(a, "name")))
                .unwrap_or_else(|| {
                    "Account details unavailable — review the source references".into()
                });
            let role = match leg.account {
                PurchaseLegAccount::Policy(PurchaseRole::Inventory) => "Inventory",
                PurchaseLegAccount::Policy(PurchaseRole::GoodsReceivedNotInvoiced) => {
                    "Goods received, not yet invoiced"
                }
                PurchaseLegAccount::Policy(PurchaseRole::Payable) => "Supplier payable",
                PurchaseLegAccount::Policy(PurchaseRole::LandedFeeAccrual) => "Landed fee accrual",
                PurchaseLegAccount::TaxControl => "Input tax",
            };
            PurchaseReviewLine {
                role: role.into(),
                account,
                debit: amount(&json!(leg.debit_minor), book),
                credit: amount(&json!(leg.credit_minor), book),
            }
        })
        .collect();
    Some(PurchaseReview {
        effect: match effect {
            PurchaseEffect::Receipt => "Received goods and acquisition fees",
            PurchaseEffect::Bill => "Matched supplier goods claim",
            PurchaseEffect::Void => "Void of a matched supplier claim",
        },
        goods: amount(&source["goodsMinor"], book),
        fee: amount(&source["landedFeeMinor"], book),
        date: text(source, "sourceDate"),
        policy_revision: source["policyRevision"].to_string(),
        tax: split.map(|(tax, total)| {
            (
                amount(&json!(tax.to_string()), book),
                amount(&json!(total.to_string()), book),
            )
        }),
        lines,
        accounts_match,
    })
}
pub(super) fn purchase_review_accounts_match(body: &Value, book: &Value, refs: &Value) -> bool {
    purchase_review(body, book, refs).is_some_and(|review| review.accounts_match)
}
struct StockReview {
    effect: &'static str,
    amount: String,
    date: String,
    policy_revision: String,
    lines: Vec<PurchaseReviewLine>,
    accounts_match: bool,
}
fn stock_review(body: &Value, book: &Value, refs: &Value) -> Option<StockReview> {
    use archava_domain::erp::accounting::{minor, MAX_MINOR};
    // A stock source carries no policy snapshot; its amounts and legs are
    // book minor units under the one unit the book's policy can have.
    book_source_unit(book)?;
    let source = source_reference(refs, body["sourceId"].as_str()?, "stock_source.draft")?;
    let net: i64 = source["amountMinor"].as_str()?.parse().ok()?;
    let total = net.checked_abs().filter(|v| *v > 0 && *v <= MAX_MINOR)?;
    let fulfillment = match source["effect"].as_str()? {
        "fulfillment" if net > 0 => true,
        "restock_return" if net < 0 => false,
        _ => return None,
    };
    let selected = body["accounts"].as_array()?;
    let legs = source["legs"].as_array()?;
    if !(2..=100).contains(&legs.len()) {
        return None;
    }
    let mut accounts_match = selected.len() == legs.len();
    let mut ids = std::collections::BTreeSet::new();
    let (mut inventory, mut cogs) = (0_i64, 0_i64);
    let mut lines = Vec::new();
    for leg in legs {
        let id = leg["accountId"].as_str()?;
        if !ids.insert(id) {
            return None;
        }
        let debit = minor(leg["debitMinor"].as_str()?).ok()?;
        let credit = minor(leg["creditMinor"].as_str()?).ok()?;
        let (role, is_cogs, accumulated) = match leg["role"].as_str()? {
            "inventory" => ("Original inventory", false, &mut inventory),
            "cost_of_goods_sold" => ("Original cost of goods sold", true, &mut cogs),
            _ => return None,
        };
        let is_debit = fulfillment == is_cogs;
        let magnitude = if is_debit && credit == 0 {
            debit
        } else if !is_debit && debit == 0 {
            credit
        } else {
            return None;
        };
        if magnitude == 0 {
            return None;
        }
        *accumulated = accumulated.checked_add(magnitude)?;
        let chosen = selected.iter().find(|a| a["accountId"] == id);
        accounts_match &= chosen.is_some();
        let account = refs["accounting/accounts"]
            .as_array()
            .and_then(|rows| rows.iter().find(|a| a["id"] == id))
            .filter(|a| chosen.is_some_and(|r| r["expectedAccountRevision"] == a["revision"]))
            .map(|a| format!("{} · {}", text(a, "code"), text(a, "name")))
            .unwrap_or_else(|| "Account details unavailable — review the source references".into());
        lines.push(PurchaseReviewLine {
            role: role.into(),
            account,
            debit: amount(&leg["debitMinor"], book),
            credit: amount(&leg["creditMinor"], book),
        });
    }
    if inventory != total || cogs != total {
        return None;
    }
    Some(StockReview {
        effect: if fulfillment {
            "Cost of fulfilled goods"
        } else {
            "Restocked goods at their original recognized cost"
        },
        amount: amount(&source["amountMinor"], book),
        date: text(source, "sourceDate"),
        policy_revision: source["policyRevision"].to_string(),
        lines,
        accounts_match,
    })
}
pub(super) fn source_review_accounts_match(body: &Value, book: &Value, refs: &Value) -> bool {
    if body["type"]
        .as_str()
        .is_some_and(|s| s.starts_with("bank_"))
    {
        return bank::valid_review(body, book, refs);
    }
    match body["type"].as_str() {
        Some("invoice_terms.set" | "supplier_terms.set") => terms::valid_review(body, book, refs),
        Some("clearing_settlement.draft") => clearing::valid_review(body, book, refs),
        Some("payable_payment.draft" | "payable_return.draft") => {
            payables::valid_review(body, book, refs)
        }
        Some("invoice_source.draft") => {
            invoice_review(body, book, refs).is_some_and(|r| r.accounts_match)
        }
        Some("purchase_source.draft") => purchase_review_accounts_match(body, book, refs),
        Some("stock_source.draft") => {
            stock_review(body, book, refs).is_some_and(|r| r.accounts_match)
        }
        _ => true,
    }
}
fn source_journal(
    lines: Vec<PurchaseReviewLine>,
    test_id: &'static str,
    caption: &'static str,
) -> impl IntoView {
    view! {
        <div class="ops-table-wrap source-journal-wrap">
            <table class="ops-table source-journal" data-testid=test_id>
                <caption>{caption}</caption>
                <thead><tr><th scope="col">"Purpose"</th><th scope="col">"Reviewed account"</th><th scope="col">"Debit"</th><th scope="col">"Credit"</th></tr></thead>
                <tbody>{lines.into_iter().map(|line|view! {
                    <tr>
                        <td><span class="source-mobile-label" aria-hidden="true">"Purpose"</span><span>{line.role}</span></td>
                        <td><span class="source-mobile-label" aria-hidden="true">"Reviewed account"</span><span>{line.account}</span></td>
                        <td class="source-journal-money"><span class="source-mobile-label" aria-hidden="true">"Debit"</span><span class="source-money">{line.debit}</span></td>
                        <td class="source-journal-money"><span class="source-mobile-label" aria-hidden="true">"Credit"</span><span class="source-money">{line.credit}</span></td>
                    </tr>
                }).collect::<Vec<_>>()}</tbody>
            </table>
        </div>
    }
}
fn stock_review_view(body: Value, book: &Value, refs: &Value) -> AnyView {
    let Some(summary) = stock_review(&body, book, refs) else {
        return view! {<p class="refusal">"Original stock cost allocations are unavailable. Refresh and select the captured source before saving."</p>}.into_any();
    };
    view! {
        <section data-testid="stock-source-summary"><h4>{summary.effect}</h4>
            <dl class="operator-details"><dt>"Recognized cost change"</dt><dd>{summary.amount}</dd><dt>"Book date"</dt><dd>{summary.date}</dd><dt>"Captured stock policy"</dt><dd>{format!("Revision {}", summary.policy_revision)}</dd></dl>
            <p class="hint">"Restocking uses the original fulfillment accounts. Saving creates a draft; posting requires a separate review."</p>
        </section>
        {source_journal(summary.lines, "stock-source-journal", "Proposed journal from the original stock cost allocations")}
        {(!summary.accounts_match).then(||view!{<p class="refusal">"Select every original source account before saving."</p>})}
        <details data-testid="stock-source-references"><summary>"Source and approval references"</summary>{details(body)}</details>
    }.into_any()
}
pub(super) fn review(body: Value, book: &Value, refs: &Value) -> AnyView {
    if let Some(review) = terms::review(&body, book, refs) {
        return review;
    }
    if let Some(review) = clearing::review(&body, book, refs) {
        return review;
    }
    if let Some(review) = payables::review(&body, book, refs) {
        return review;
    }
    if let Some(review) = bank::review(&body, book, refs) {
        return review;
    }
    let display = reviewed_preview(body.clone(), book, refs);
    if let Some(source) = body["sourceId"]
        .as_str()
        .and_then(|id| source_reference(refs, id, body["type"].as_str().unwrap_or_default()))
    {
        if source["sourceState"] == "no_monetary_effect" {
            return view!{<section data-testid="source-no-monetary-effect"><h4>"No journal is needed for this source"</h4><p>"This captured event has no monetary effect. It does not need a draft or posting to complete source-aware close."</p></section>}.into_any();
        }
    }
    if body["type"] == "invoice_source.draft" {
        let Some(r) = invoice_review(&body, book, refs) else {
            return view!{<p class="refusal">"Invoice source details are unavailable. Refresh the source and original accounts before saving."</p>}.into_any();
        };
        // A source that carries tax shows its gross, net and tax and a third
        // line on the captured tax control account; without tax the review is
        // the one below, unchanged.
        if let Some((net, tax)) = r.tax.clone() {
            return view!{<section data-testid="invoice-source-summary"><h4>{format!("Invoice {} · {}",r.invoice,r.effect)}</h4><dl class="operator-details"><dt>"Source amount (gross)"</dt><dd>{r.amount}</dd><dt>"Net"</dt><dd data-testid="invoice-source-net">{net}</dd><dt>"Tax"</dt><dd data-testid="invoice-source-tax">{tax}</dd><dt>"Book date"</dt><dd>{r.date}</dd><dt>"Original invoice policy"</dt><dd>{format!("Revision {}",r.policy_revision)}</dd></dl><p class="hint">"Saving creates a draft. The tax posts to the control account of the invoice's tax code, which the company entered; Archava does not verify tax rates or rules. Posting is a separate review."</p></section>{source_journal(r.lines,"invoice-source-journal","Proposed journal from the captured invoice source")}{(!r.accounts_match).then(||view!{<p class="refusal">"Select the receivable, revenue and tax control accounts of this source before saving."</p>})}<details><summary>"Source and approval references"</summary>{details(display)}</details>}.into_any();
        }
        return view!{<section data-testid="invoice-source-summary"><h4>{format!("Invoice {} · {}",r.invoice,r.effect)}</h4><dl class="operator-details"><dt>"Source amount"</dt><dd>{r.amount}</dd><dt>"Book date"</dt><dd>{r.date}</dd><dt>"Original invoice policy"</dt><dd>{format!("Revision {}",r.policy_revision)}</dd></dl><p class="hint">"Saving creates a draft. Verified provider collections use payment clearing; bank settlement and posting are separate."</p></section>{source_journal(r.lines,"invoice-source-journal","Proposed journal from the captured invoice source")}{(!r.accounts_match).then(||view!{<p class="refusal">"Select the complete original source accounts before saving."</p>})}<details><summary>"Source and approval references"</summary>{details(display)}</details>}.into_any();
    }
    if body["type"] == "stock_source.draft" {
        return stock_review_view(body, book, refs);
    }
    if body["type"] != "purchase_source.draft" {
        return details(display);
    }
    let Some(summary) = purchase_review(&body, book, refs) else {
        return view! {<p class="refusal">"Source details are unavailable. Refresh and select the source again before saving."</p>{details(display)}}.into_any();
    };
    // A bill or void that carries tax shows its net goods, tax and the total
    // owed with a third line on the captured tax control account; without tax
    // the review is the one below, unchanged.
    if let Some((tax, total)) = summary.tax.clone() {
        return view! {
            <section data-testid="purchase-source-summary">
                <h4>{summary.effect}</h4>
                <dl class="operator-details"><dt>"Goods (net)"</dt><dd data-testid="purchase-source-net">{summary.goods}</dd><dt>"Input tax"</dt><dd data-testid="purchase-source-tax">{tax}</dd><dt>"Total owed to the supplier"</dt><dd data-testid="purchase-source-total">{total}</dd><dt>"Book date"</dt><dd>{summary.date}</dd><dt>"Original posting policy"</dt><dd>{format!("Revision {}",summary.policy_revision)}</dd></dl>
                <p class="hint">"Saving creates a draft journal. The tax is the amount entered from the supplier's document; it posts to the control account of the document's tax code, which the company entered. Archava does not verify tax rates or rules. Posting requires a separate action. Supplier bank payment is separate."</p>
            </section>
            {source_journal(summary.lines, "purchase-source-journal", "Proposed journal using the original source account mapping and the tax control account")}
            {(!summary.accounts_match).then(||view!{<p class="refusal">"Select the goods received, tax control and payable accounts of this source before saving."</p>})}
            <details data-testid="purchase-source-references"><summary>"Source and approval references"</summary>{details(display)}</details>
        }.into_any();
    }
    view! {
        <section data-testid="purchase-source-summary">
            <h4>{summary.effect}</h4>
            <dl class="operator-details"><dt>"Goods"</dt><dd>{summary.goods}</dd><dt>"Landed fee"</dt><dd>{summary.fee}</dd><dt>"Book date"</dt><dd>{summary.date}</dd><dt>"Original posting policy"</dt><dd>{format!("Revision {}",summary.policy_revision)}</dd></dl>
            <p class="hint">"Saving creates a draft journal. Posting requires a separate action. Supplier bank payment is separate."</p>
        </section>
        {source_journal(summary.lines, "purchase-source-journal", "Proposed journal using the original source account mapping")}
        {(!summary.accounts_match).then(||view!{<p class="refusal">"The selected accounts do not match the original source mapping. Reselect the source accounts before saving."</p>})}
        <details data-testid="purchase-source-references"><summary>"Source and approval references"</summary>{details(display)}</details>
    }.into_any()
}
#[component]
pub(super) fn MoneyField(
    label: String,
    id: String,
    value: Signal<Value>,
    set: Callback<Value>,
    refs: RwSignal<Value>,
    signed: bool,
) -> impl IntoView {
    let exponent = Signal::derive(move || {
        refs.get()["__minorUnits"]
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
    });
    let raw = RwSignal::new(String::new());
    Effect::new(move |_| {
        let v = value.get();
        let exp = exponent.get();
        let current = raw.get_untracked();
        let parsed = exp
            .and_then(|e| parse_money(&current, e, signed))
            .map(Value::String)
            .unwrap_or(Value::Null);
        if parsed != v {
            raw.set_if_alive(
                v.as_str()
                    .and_then(|v| exp.and_then(|e| decimal(v, e)))
                    .unwrap_or_default(),
            );
        }
    });
    let set_value = move |e| {
        let input = event_target_value(&e);
        let parsed = exponent
            .get_untracked()
            .and_then(|e| parse_money(&input, e, signed));
        raw.set_if_alive(input);
        set.run(parsed.map(Value::String).unwrap_or(Value::Null));
    };
    view! {<label class="ops-field"><span>{label}</span><input type="text" inputmode="decimal" maxlength="32" data-testid=id required prop:value=move||raw.get() on:input=set_value disabled=move||exponent.get().is_none()/><small>{move||format!("{} — up to {} decimal places. Use a dot for decimals.",text(&refs.get(),"__currency"),exponent.get().unwrap_or(0))}</small></label>}
}

#[component]
pub(super) fn Reports(work: Workbench) -> impl IntoView {
    let from = RwSignal::new(String::new());
    let to = RwSignal::new(String::new());
    let report = RwSignal::new(None::<Value>);
    let notice = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let sequence = RwSignal::new(0_u64);
    Effect::new(move |_| {
        let data = work.data.get();
        report.set_if_alive(None);
        sequence.update_if_alive(|s| *s += 1);
        busy.set_if_alive(false);
        if let Some(period) = data["periods"].as_array().and_then(|p| p.first()) {
            if from.get_untracked().is_empty() {
                from.set_if_alive(text(period, "startDate"));
                to.set_if_alive(text(period, "endDate"));
            }
        }
    });
    let load = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(held) = session::held() else { return };
        let data = work.data.get_untracked();
        let book = text(&data["configuredBook"], "id");
        if book.is_empty() {
            notice.set_if_alive("Set up and select a company book first.".into());
            return;
        }
        let start = from.get_untracked();
        let end = to.get_untracked();
        if archava_domain::erp::accommodation::parse_calendar_date(&start).is_none()
            || archava_domain::erp::accommodation::parse_calendar_date(&end).is_none()
            || start > end
        {
            notice.set_if_alive("Choose a valid report date range.".into());
            return;
        }
        sequence.update_if_alive(|s| *s += 1);
        let seq = sequence.get_untracked();
        busy.set_if_alive(true);
        notice.set_if_alive(String::new());
        let company = work.company_key.get_untracked();
        let path = format!(
            "/erp/accounting/reports?tenant={}&bookId={}&from={start}&to={end}",
            crate::contacts::query_encode(&held.tenant),
            book
        );
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if sequence.is_disposed()
                || sequence.get_untracked() != seq
                || work.company_key.get_untracked() != company
            {
                return;
            }
            busy.set_if_alive(false);
            match answer {
                Ok(v) if v.status == 200 => report.set_if_alive(Some(v.body)),
                Ok(v) => notice.set_if_alive(message(&v)),
                Err(_) => notice
                    .set_if_alive("The reports could not be loaded. Refresh and try again.".into()),
            }
        });
    };
    view! {<section class="ops-detail" data-testid="accounting-reports"><h2>"Financial reports"</h2><p class="hint">{move||text(&work.data.get(),"notice")}</p><p class="hint">{move||text(&work.data.get(),"invoicePostingNotice")}</p><p class="hint">{move||text(&work.data.get(),"purchasePostingNotice")}</p><p class="hint">{move||text(&work.data.get(),"stockPostingNotice")}</p><p class="hint">"Reports include every posted journal in the chosen date range. Drafts and voids are excluded. The balance sheet includes earlier posted balances through the end date."</p>
        <payables::Balances work/>
        <aging::AgingReports work/>
        <close_check::CloseReadiness work/>
        <inventory_check::InventoryControl work/>
        <tax_codes::TaxCodes work/>
        <tax_report::TaxReport work/>
        <form class="ops-form" on:submit=load><label class="ops-field"><span>"From"</span><input type="date" required data-testid="accounting-report-from" prop:value=move||from.get() on:input=move|e|from.set_if_alive(event_target_value(&e))/></label><label class="ops-field"><span>"Through"</span><input type="date" required data-testid="accounting-report-to" prop:value=move||to.get() on:input=move|e|to.set_if_alive(event_target_value(&e))/></label><button type="submit" class="btn secondary" data-testid="accounting-report-run" disabled=move||busy.get()||!work.permits("reports.read")>"Refresh financial reports"</button></form>
        {move||{let n=notice.get();(!n.is_empty()).then(||view!{<p class="refusal">{n}</p>})}}
        {move||report.get().map(|r|{let book=r["book"].clone();let rows=r["trialBalance"]["accounts"].as_array().cloned().unwrap_or_default();let row_book=book.clone();
            view!{<div data-testid="accounting-report-result"><p>{format!("{} · {} through {} · observed {}",text(&book,"name"),text(&r,"from"),text(&r,"to"),text(&r,"observedAt"))}</p>
                <p class="hint" data-testid="accounting-source-coverage">{format!("Captured invoice, purchase and stock cost sources through this end date: {} posted, {} awaiting posting, {} without a monetary effect. Legacy operating documents and bank reconciliation require separate review.",r["sourceCoverage"]["postedCount"],r["sourceCoverage"]["unpostedCount"],r["sourceCoverage"]["noMonetaryEffectCount"])}</p>
                <dl class="operator-details"><dt>"Revenue"</dt><dd>{amount(&r["profitAndLoss"]["revenueMinor"],&book)}</dd><dt>"Cost of goods sold"</dt><dd>{amount(&r["profitAndLoss"]["costOfGoodsSoldMinor"],&book)}</dd><dt>"Operating expense"</dt><dd>{amount(&r["profitAndLoss"]["expenseMinor"],&book)}</dd><dt>"Net profit"</dt><dd>{amount(&r["profitAndLoss"]["netProfitMinor"],&book)}</dd><dt>"Assets"</dt><dd>{amount(&r["balanceSheet"]["assetsMinor"],&book)}</dd><dt>"Liabilities"</dt><dd>{amount(&r["balanceSheet"]["liabilitiesMinor"],&book)}</dd><dt>"Total equity including unclosed earnings"</dt><dd>{amount(&r["balanceSheet"]["totalEquityMinor"],&book)}</dd><dt>"Operating cash movement"</dt><dd>{amount(&r["cashFlow"]["operatingMinor"],&book)}</dd><dt>"Investing cash movement"</dt><dd>{amount(&r["cashFlow"]["investingMinor"],&book)}</dd><dt>"Financing cash movement"</dt><dd>{amount(&r["cashFlow"]["financingMinor"],&book)}</dd><dt>"Unclassified cash movement"</dt><dd>{amount(&r["cashFlow"]["unclassifiedMinor"],&book)}</dd><dt>"Net cash movement"</dt><dd>{amount(&r["cashFlow"]["netChangeMinor"],&book)}</dd></dl>
                {(r["cashFlow"]["unclassifiedJournals"].as_u64().unwrap_or(0)>0).then(||view!{<p class="refusal">"Some cash journals have mixed or missing classifications. Review those sources before relying on cash flow categories."</p>})}
                <div class="ops-table-wrap"><table class="ops-table"><caption>"Trial balance — exact account movements and closing balances"</caption><thead><tr><th>"Account"</th><th>"Debit"</th><th>"Credit"</th><th>"Closing debit less credit"</th></tr></thead><tbody>{rows.into_iter().map(|row|view!{<tr><td>{format!("{} · {}",text(&row,"code"),text(&row,"name"))}</td><td>{amount(&row["debitMinor"],&row_book)}</td><td>{amount(&row["creditMinor"],&row_book)}</td><td>{amount(&row["closingNetMinor"],&row_book)}</td></tr>}).collect::<Vec<_>>()}</tbody></table></div><p class="hint">{text(&r,"notice")}</p></div>}
        })}
    </section>}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invoice_credit_review_uses_original_roles_and_zero_effect_does_not_offer_a_journal() {
        let book = json!({"currency":"IDR","minorUnits":2});
        let mut refs = json!({"accounting/invoice_sources":[{"id":"source","effect":"correction","invoiceCode":"INV-ORIGINAL","amountMinor":"-1500000","sourceDate":"2026-11-09","policyRevision":1,"policy":{"mappings":[{"role":"receivable","accountId":"ar"},{"role":"revenue","accountId":"revenue"},{"role":"payment_clearing","accountId":"clearing"}]}}],"accounting/accounts":[{"id":"ar","revision":3,"code":"AR","name":"Original receivables"},{"id":"revenue","revision":1,"code":"REV","name":"Original revenue"}]});
        let body = json!({"type":"invoice_source.draft","sourceId":"source","accounts":[{"accountId":"ar","expectedAccountRevision":3},{"accountId":"revenue","expectedAccountRevision":1}]});
        assert!(source_review_accounts_match(&body, &book, &refs));
        let r = invoice_review(&body, &book, &refs).unwrap();
        assert_eq!(r.lines[0].account, "REV · Original revenue");
        assert_eq!(r.lines[0].debit, "IDR 15000.00");
        assert_eq!(r.lines[1].credit, "IDR 15000.00");
        let html = review(body.clone(), &book, &refs).to_html();
        assert!(html.contains("INV-ORIGINAL"));
        assert!(
            html.find("IDR -15000.00").unwrap()
                < html.find("Source and approval references").unwrap()
        );
        refs["accounting/invoice_sources"][0]["amountMinor"] = json!("0");
        refs["accounting/invoice_sources"][0]["sourceState"] = json!("no_monetary_effect");
        assert!(!source_review_accounts_match(&body, &book, &refs));
        assert!(review(body, &book, &refs)
            .to_html()
            .contains("No journal is needed for this source"));
    }
    #[test]
    fn a_taxed_invoice_source_review_shows_gross_net_and_tax_with_the_tax_account() {
        use leptos::prelude::RenderHtml;
        let accounts = json!([{"id":"ar","revision":3,"code":"AR","name":"Receivables"},{"id":"revenue","revision":1,"code":"REV","name":"Revenue"},{"id":"vat","revision":2,"code":"VAT-OUT","name":"Output tax payable"}]);
        let mappings = json!([{"role":"receivable","accountId":"ar"},{"role":"revenue","accountId":"revenue"},{"role":"payment_clearing","accountId":"clearing"}]);
        let three = json!([{"accountId":"ar","expectedAccountRevision":3},{"accountId":"revenue","expectedAccountRevision":1},{"accountId":"vat","expectedAccountRevision":2}]);
        let two = json!([{"accountId":"ar","expectedAccountRevision":3},{"accountId":"revenue","expectedAccountRevision":1}]);

        // IDR: Rp33.300 gross with Rp3.300 of tax, captured as book units.
        let idr = json!({"currency":"IDR","minorUnits":2});
        let refs = json!({"accounting/accounts":accounts,"accounting/invoice_sources":[{"id":"taxed","effect":"issue","invoiceCode":"INV-TAX","amountMinor":"3330000","taxMinor":"330000","taxAccountId":"vat","taxAccountRevision":2,"sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"whole_idr","mappings":mappings}}]});
        let body = json!({"type":"invoice_source.draft","sourceId":"taxed","code":"ISSUE","accounts":three});
        assert!(source_review_accounts_match(&body, &idr, &refs));
        let r = invoice_review(&body, &idr, &refs).unwrap();
        assert_eq!(r.amount, "IDR 33300.00");
        assert_eq!(
            r.tax,
            Some(("IDR 30000.00".to_owned(), "IDR 3300.00".to_owned()))
        );
        let lines: Vec<_> = r
            .lines
            .iter()
            .map(|l| {
                (
                    l.role.as_str(),
                    l.account.as_str(),
                    l.debit.as_str(),
                    l.credit.as_str(),
                )
            })
            .collect();
        assert_eq!(
            lines,
            vec![
                (
                    "Customer receivable",
                    "AR · Receivables",
                    "IDR 33300.00",
                    "IDR 0.00"
                ),
                ("Sales revenue", "REV · Revenue", "IDR 0.00", "IDR 30000.00"),
                (
                    "Output tax",
                    "VAT-OUT · Output tax payable",
                    "IDR 0.00",
                    "IDR 3300.00"
                ),
            ]
        );
        let html = review(body.clone(), &idr, &refs).to_html();
        assert!(
            html.contains("data-testid=\"invoice-source-summary\""),
            "{html}"
        );
        assert!(html.contains("Source amount (gross)"), "{html}");
        assert!(
            html.contains("data-testid=\"invoice-source-net\""),
            "{html}"
        );
        assert!(
            html.contains("data-testid=\"invoice-source-tax\""),
            "{html}"
        );
        for shown in [
            "IDR 33300.00",
            "IDR 30000.00",
            "IDR 3300.00",
            "Output tax",
            "VAT-OUT · Output tax payable",
        ] {
            assert!(html.contains(shown), "{shown}: {html}");
        }
        assert!(
            html.contains("Archava does not verify tax rates or rules"),
            "{html}"
        );
        assert!(!html.contains("class=\"refusal\""), "{html}");
        let preview = reviewed_preview(body.clone(), &idr, &refs);
        assert_eq!(preview["reviewedSource"]["gross"], "IDR 33300.00");
        assert_eq!(preview["reviewedSource"]["net"], "IDR 30000.00");
        assert_eq!(preview["reviewedSource"]["tax"], "IDR 3300.00");
        assert_eq!(
            preview["reviewedSource"]["taxAccount"],
            json!({"accountId":"vat","expectedAccountRevision":2})
        );
        // The command that is sent is the reviewed body: three accounts, no amounts.
        assert_eq!(body["accounts"].as_array().unwrap().len(), 3);
        assert!(body.get("reviewedSource").is_none());

        // Two accounts, another third account or a stale tax account revision
        // do not match a taxed source, and the panel says so.
        let mut short = body.clone();
        short["accounts"] = two.clone();
        assert!(!source_review_accounts_match(&short, &idr, &refs));
        let html = review(short, &idr, &refs).to_html();
        assert!(
            html.contains("Select the receivable, revenue and tax control accounts of this source before saving."),
            "{html}"
        );
        let mut wrong = body.clone();
        wrong["accounts"][2]["accountId"] = json!("clearing");
        assert!(!source_review_accounts_match(&wrong, &idr, &refs));
        let mut stale = body.clone();
        stale["accounts"][2]["expectedAccountRevision"] = json!(1);
        assert!(!source_review_accounts_match(&stale, &idr, &refs));

        // A credit of the taxed invoice reverses the three sides.
        let mut credit = refs.clone();
        credit["accounting/invoice_sources"][0]["effect"] = json!("correction");
        credit["accounting/invoice_sources"][0]["amountMinor"] = json!("-1110000");
        credit["accounting/invoice_sources"][0]["taxMinor"] = json!("-110000");
        let r = invoice_review(&body, &idr, &credit).unwrap();
        assert_eq!(
            r.tax,
            Some(("IDR -10000.00".to_owned(), "IDR -1100.00".to_owned()))
        );
        assert_eq!(
            (r.lines[0].role.as_str(), r.lines[0].debit.as_str()),
            ("Sales revenue", "IDR 10000.00")
        );
        assert_eq!(
            (r.lines[1].role.as_str(), r.lines[1].debit.as_str()),
            ("Output tax", "IDR 1100.00")
        );
        assert_eq!(
            (r.lines[2].role.as_str(), r.lines[2].credit.as_str()),
            ("Customer receivable", "IDR 11100.00")
        );

        // USD: 11.09 gross with 1.10 of tax in minor units, posted unchanged.
        let usd = json!({"currency":"USD","minorUnits":2});
        let mut usd_refs = refs.clone();
        usd_refs["accounting/invoice_sources"][0] = json!({"id":"taxed","effect":"issue","invoiceCode":"INV-USD-TAX","amountMinor":"1109","taxMinor":"110","taxAccountId":"vat","taxAccountRevision":2,"sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"minor","mappings":mappings}});
        assert!(source_review_accounts_match(&body, &usd, &usd_refs));
        let html = review(body.clone(), &usd, &usd_refs).to_html();
        for shown in [
            "INV-USD-TAX",
            "USD 11.09",
            "USD 9.99",
            "USD 1.10",
            "Output tax",
        ] {
            assert!(html.contains(shown), "{shown}: {html}");
        }
        let preview = reviewed_preview(body.clone(), &usd, &usd_refs);
        assert_eq!(preview["reviewedSource"]["net"], "USD 9.99");
        assert_eq!(preview["reviewedSource"]["tax"], "USD 1.10");

        // An untaxed source, and a source of a 0% code (tax 0 with its
        // account captured), are reviewed with two accounts as before: no
        // net, tax or tax line, and a third account does not match.
        for zero in [
            json!({"id":"taxed","effect":"issue","invoiceCode":"INV-PLAIN","amountMinor":"3000000","sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"whole_idr","mappings":mappings}}),
            json!({"id":"taxed","effect":"issue","invoiceCode":"INV-PLAIN","amountMinor":"3000000","taxMinor":"0","taxAccountId":"vat","taxAccountRevision":2,"sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"whole_idr","mappings":mappings}}),
        ] {
            let mut plain = refs.clone();
            plain["accounting/invoice_sources"][0] = zero;
            let mut untaxed = body.clone();
            untaxed["accounts"] = two.clone();
            assert!(source_review_accounts_match(&untaxed, &idr, &plain));
            assert!(!source_review_accounts_match(&body, &idr, &plain));
            let r = invoice_review(&untaxed, &idr, &plain).unwrap();
            assert_eq!(r.tax, None);
            assert_eq!(r.lines.len(), 2);
            let html = review(untaxed.clone(), &idr, &plain).to_html();
            assert!(html.contains("<dt>Source amount</dt>"), "{html}");
            for absent in [
                "Output tax",
                "invoice-source-net",
                "invoice-source-tax",
                "(gross)",
                "does not verify tax",
            ] {
                assert!(!html.contains(absent), "{absent}: {html}");
            }
            let preview = reviewed_preview(untaxed, &idr, &plain);
            for key in ["gross", "net", "tax", "taxAccount"] {
                assert!(preview["reviewedSource"].get(key).is_none(), "{key}");
            }
            assert_eq!(preview["reviewedSource"]["amount"], "IDR 30000.00");
        }
    }
    #[test]
    fn purchase_decision_leads_with_exact_money_and_binds_original_accounts_without_changing_input()
    {
        use leptos::prelude::RenderHtml;
        let book = json!({"currency":"IDR","minorUnits":2});
        let refs = json!({"accounting/accounts":[{"id":"original","revision":2,"code":"STOCK","name":"Inventory <review>"},{"id":"grni","revision":1,"code":"GRNI","name":"Unbilled goods"},{"id":"fee","revision":1,"code":"FEE","name":"Acquisition fees"}],"accounting/purchase_sources":[{"id":"source","effect":"receipt","goodsMinor":"1400000","landedFeeMinor":"10100","sourceDate":"2026-11-08","policyRevision":7,"policy":{"mappings":[{"role":"inventory","accountId":"original","expectedAccountRevision":1},{"role":"goods_received_not_invoiced","accountId":"grni","expectedAccountRevision":1},{"role":"landed_fee_accrual","accountId":"fee","expectedAccountRevision":1},{"role":"payable","accountId":"ap","expectedAccountRevision":1}]}}]});
        let body = json!({"type":"purchase_source.draft","sourceId":"source","code":"RECEIPT","accounts":[{"accountId":"original","expectedAccountRevision":2},{"accountId":"grni","expectedAccountRevision":1},{"accountId":"fee","expectedAccountRevision":1}]});
        let before = body.clone();
        let summary = purchase_review(&body, &book, &refs).unwrap();
        assert!(summary.accounts_match);
        assert_eq!(summary.lines.len(), 3);
        assert_eq!(summary.lines[0].account, "STOCK · Inventory <review>");
        assert_eq!(summary.lines[0].debit, "IDR 14101.00");
        assert_eq!(summary.lines[1].credit, "IDR 14000.00");
        assert_eq!(summary.lines[2].credit, "IDR 101.00");
        let html = review(body.clone(), &book, &refs).to_html();
        assert!(
            html.find("IDR 14000.00").unwrap()
                < html.find("Source and approval references").unwrap()
        );
        assert!(html.contains("Inventory &lt;review&gt;"));
        assert!(!html.contains("Inventory <review>"));
        assert_eq!(
            body, before,
            "Presentation cannot rewrite the confirmed command"
        );
        let mut changed = body.clone();
        changed["accounts"][0]["accountId"] = json!("future-inventory");
        assert!(!purchase_review_accounts_match(&changed, &book, &refs));
        changed["accounts"][0] = json!({"accountId":"original","expectedAccountRevision":3});
        let unavailable = purchase_review(&changed, &book, &refs).unwrap();
        assert!(
            unavailable.lines[0].account.contains("unavailable"),
            "An old cached caption must not impersonate a newer reviewed account version"
        );
        assert!(purchase_review(&body, &json!({"currency":"USD","minorUnits":2}), &refs).is_none());
    }
    #[test]
    fn a_taxed_purchase_source_review_shows_net_tax_and_total_with_the_input_tax_account() {
        use leptos::prelude::RenderHtml;
        let accounts = json!([{"id":"grni","revision":1,"code":"GRNI","name":"Unbilled goods"},{"id":"ap","revision":2,"code":"AP","name":"Supplier payables"},{"id":"vat","revision":3,"code":"VAT-IN","name":"Input tax receivable"},{"id":"stock","revision":1,"code":"STOCK","name":"Inventory"}]);
        let mappings = json!([{"role":"inventory","accountId":"stock","expectedAccountRevision":1},{"role":"goods_received_not_invoiced","accountId":"grni","expectedAccountRevision":1},{"role":"payable","accountId":"ap","expectedAccountRevision":1},{"role":"landed_fee_accrual","accountId":"fee","expectedAccountRevision":1}]);
        let three = json!([{"accountId":"grni","expectedAccountRevision":1},{"accountId":"vat","expectedAccountRevision":3},{"accountId":"ap","expectedAccountRevision":2}]);
        let two = json!([{"accountId":"grni","expectedAccountRevision":1},{"accountId":"ap","expectedAccountRevision":2}]);
        let line = |l: &PurchaseReviewLine| {
            (
                l.role.clone(),
                l.account.clone(),
                l.debit.clone(),
                l.credit.clone(),
            )
        };
        let own = |a: &str, b: &str, c: &str, d: &str| {
            (a.to_owned(), b.to_owned(), c.to_owned(), d.to_owned())
        };

        // IDR: goods Rp140.000 with Rp15.400 of supplier tax, captured as book units.
        let idr = json!({"currency":"IDR","minorUnits":2});
        let refs = json!({"accounting/accounts":accounts,"accounting/purchase_sources":[{"id":"taxed","effect":"bill","orderId":"po","documentId":"doc","goodsMinor":"14000000","landedFeeMinor":"0","taxMinor":"1540000","taxAccountId":"vat","taxAccountRevision":3,"sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"whole_idr","mappings":mappings}}]});
        let body = json!({"type":"purchase_source.draft","sourceId":"taxed","code":"BILL","accounts":three});
        assert!(purchase_review_accounts_match(&body, &idr, &refs));
        let r = purchase_review(&body, &idr, &refs).unwrap();
        assert_eq!(r.goods, "IDR 140000.00");
        assert_eq!(
            r.tax,
            Some(("IDR 15400.00".to_owned(), "IDR 155400.00".to_owned()))
        );
        assert_eq!(
            r.lines.iter().map(line).collect::<Vec<_>>(),
            vec![
                own(
                    "Goods received, not yet invoiced",
                    "GRNI · Unbilled goods",
                    "IDR 140000.00",
                    "IDR 0.00"
                ),
                own(
                    "Input tax",
                    "VAT-IN · Input tax receivable",
                    "IDR 15400.00",
                    "IDR 0.00"
                ),
                own(
                    "Supplier payable",
                    "AP · Supplier payables",
                    "IDR 0.00",
                    "IDR 155400.00"
                ),
            ]
        );
        let html = review(body.clone(), &idr, &refs).to_html();
        for expected in [
            "data-testid=\"purchase-source-net\"",
            "data-testid=\"purchase-source-tax\"",
            "data-testid=\"purchase-source-total\"",
            "IDR 15400.00",
            "IDR 155400.00",
            "Input tax",
            "VAT-IN · Input tax receivable",
            "Archava does not verify tax rates or rules.",
        ] {
            assert!(html.contains(expected), "{expected}: {html}");
        }
        assert!(
            !html.contains("Select the goods received, tax control and payable accounts"),
            "{html}"
        );
        let preview = reviewed_preview(body.clone(), &idr, &refs);
        assert_eq!(preview["reviewedSource"]["goods"], "IDR 140000.00");
        assert_eq!(preview["reviewedSource"]["tax"], "IDR 15400.00");
        assert_eq!(preview["reviewedSource"]["total"], "IDR 155400.00");
        assert_eq!(
            preview["reviewedSource"]["taxAccount"],
            json!({"accountId":"vat","expectedAccountRevision":3})
        );

        // Two accounts, a stale tax account revision or another third account do not match.
        let mut missing = body.clone();
        missing["accounts"] = two.clone();
        assert!(!purchase_review_accounts_match(&missing, &idr, &refs));
        assert!(review(missing, &idr, &refs)
            .to_html()
            .contains("Select the goods received, tax control and payable accounts"));
        let mut stale = body.clone();
        stale["accounts"][1]["expectedAccountRevision"] = json!(2);
        assert!(purchase_review(&stale, &idr, &refs).unwrap().lines[1]
            .account
            .contains("unavailable"));
        let mut other = body.clone();
        other["accounts"][1] = json!({"accountId":"stock","expectedAccountRevision":1});
        assert!(!purchase_review_accounts_match(&other, &idr, &refs));

        // The void of the bill reverses all three.
        let mut void = refs.clone();
        void["accounting/purchase_sources"][0]["effect"] = json!("void");
        void["accounting/purchase_sources"][0]["goodsMinor"] = json!("-14000000");
        void["accounting/purchase_sources"][0]["taxMinor"] = json!("-1540000");
        let r = purchase_review(&body, &idr, &void).unwrap();
        assert!(r.accounts_match);
        assert_eq!(
            r.lines.iter().map(line).collect::<Vec<_>>(),
            vec![
                own(
                    "Supplier payable",
                    "AP · Supplier payables",
                    "IDR 155400.00",
                    "IDR 0.00"
                ),
                own(
                    "Goods received, not yet invoiced",
                    "GRNI · Unbilled goods",
                    "IDR 0.00",
                    "IDR 140000.00"
                ),
                own(
                    "Input tax",
                    "VAT-IN · Input tax receivable",
                    "IDR 0.00",
                    "IDR 15400.00"
                ),
            ]
        );

        // USD under `minor`: the captured minor units are shown unchanged.
        let usd = json!({"currency":"USD","minorUnits":2});
        let mut usd_refs = refs.clone();
        usd_refs["accounting/purchase_sources"][0] = json!({"id":"taxed","effect":"bill","goodsMinor":"7000","landedFeeMinor":"0","taxMinor":"752","taxAccountId":"vat","taxAccountRevision":3,"sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"minor","mappings":mappings}});
        let r = purchase_review(&body, &usd, &usd_refs).unwrap();
        assert_eq!(r.goods, "USD 70.00");
        assert_eq!(r.tax, Some(("USD 7.52".to_owned(), "USD 77.52".to_owned())));
        assert_eq!(r.lines[2].credit, "USD 77.52");

        // Without tax (no tax fields, or a 0% code that keeps its account) the
        // review is the two-line one it was.
        for source in [
            json!({"id":"taxed","effect":"bill","goodsMinor":"14000000","landedFeeMinor":"0","sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"whole_idr","mappings":mappings}}),
            json!({"id":"taxed","effect":"bill","goodsMinor":"14000000","landedFeeMinor":"0","taxMinor":"0","taxAccountId":"vat","taxAccountRevision":3,"sourceDate":"2026-11-14","policyRevision":1,"policy":{"source_unit":"whole_idr","mappings":mappings}}),
        ] {
            let mut plain = refs.clone();
            plain["accounting/purchase_sources"][0] = source;
            let mut untaxed = body.clone();
            untaxed["accounts"] = two.clone();
            let r = purchase_review(&untaxed, &idr, &plain).unwrap();
            assert!(r.accounts_match);
            assert_eq!(r.tax, None);
            assert_eq!(r.lines.len(), 2);
            assert!(!purchase_review_accounts_match(&body, &idr, &plain));
            let html = review(untaxed.clone(), &idr, &plain).to_html();
            assert!(
                !html.contains("purchase-source-tax") && !html.contains("Input tax"),
                "{html}"
            );
            let preview = reviewed_preview(untaxed, &idr, &plain);
            for key in ["tax", "total", "taxAccount"] {
                assert!(preview["reviewedSource"].get(key).is_none(), "{key}");
            }
        }
    }
    #[test]
    fn stock_review_shows_original_cost_reversal_and_refuses_partial_or_different_allocations() {
        use leptos::prelude::RenderHtml;
        let book = json!({"currency":"IDR","minorUnits":2});
        let mut refs = json!({"accounting/stock_sources":[{"id":"stock","effect":"restock_return","amountMinor":"-700100","sourceDate":"2026-11-08","policyRevision":2,"legs":[{"accountId":"original-inventory","role":"inventory","debitMinor":"700100","creditMinor":"0"},{"accountId":"original-cogs","role":"cost_of_goods_sold","debitMinor":"0","creditMinor":"700100"}]}],"accounting/accounts":[{"id":"original-inventory","revision":1,"code":"STOCK","name":"Original goods"},{"id":"original-cogs","revision":2,"code":"COGS","name":"Original <cost>"}]});
        let body = json!({"type":"stock_source.draft","sourceId":"stock","code":"RESTOCK","accounts":[{"accountId":"original-inventory","expectedAccountRevision":1},{"accountId":"original-cogs","expectedAccountRevision":2}],"reason":"Original recognized account review"});
        let before = body.clone();
        assert!(source_review_accounts_match(&body, &book, &refs));
        let html = review(body.clone(), &book, &refs).to_html();
        assert!(
            html.find("IDR -7001.00").unwrap()
                < html.find("Source and approval references").unwrap()
        );
        assert!(html.contains("Original &lt;cost&gt;"));
        assert!(html.contains("data-testid=\"stock-source-references\""));
        assert!(!html.contains("<details open"));
        assert_eq!(body, before);
        let mut wrong = body.clone();
        wrong["accounts"][1]["accountId"] = json!("new-policy-cogs");
        assert!(!source_review_accounts_match(&wrong, &book, &refs));
        refs["accounting/stock_sources"][0]["legs"][0]["debitMinor"] = json!("700000");
        assert!(
            stock_review(&body, &book, &refs).is_none(),
            "A partial account summary cannot masquerade as the complete source amount"
        );
        assert!(source_reference(&refs, "stock", "invoice_source.draft").is_none());
        assert!(source_reference(&refs, "stock", "unknown_source.draft").is_none());
    }
    #[test]
    fn captured_source_review_shows_the_bound_exact_amount_and_policy() {
        let book = json!({"currency":"IDR","minorUnits":2});
        let refs = json!({"accounting/invoice_sources":[{"id":"source","invoiceCode":"INV-REAL","effect":"correction","amountMinor":"-1500000","sourceDate":"2026-11-08","policyRevision":3}]});
        let body = json!({"type":"invoice_source.draft","sourceId":"source","code":"CREDIT"});
        let display = reviewed_preview(body.clone(), &book, &refs);
        assert_eq!(display["reviewedSource"]["amount"], "IDR -15000.00");
        assert_eq!(display["reviewedSource"]["invoice"], "INV-REAL");
        assert_eq!(display["reviewedSource"]["policyRevision"], 3);
        assert!(source_reference(&refs, "different-source", "invoice_source.draft").is_none());
        assert!(
            body.get("reviewedSource").is_none(),
            "Display metadata does not enter the command contract"
        );
    }
    #[test]
    fn purchase_review_uses_the_typed_source_and_exact_goods_fee_and_original_policy() {
        let refs = json!({"accounting/invoice_sources":[{"id":"same","amountMinor":"99999"}],"accounting/purchase_sources":[{"id":"same","orderId":"po","effect":"receipt","goodsMinor":"1400000","landedFeeMinor":"10100","sourceDate":"2026-11-08","policyRevision":7,"policy":{"mappings":[{"role":"inventory","accountId":"original"}]}}]});
        let body = json!({"type":"purchase_source.draft","sourceId":"same","code":"RECEIPT"});
        let display = reviewed_preview(
            body.clone(),
            &json!({"currency":"IDR","minorUnits":2}),
            &refs,
        );
        assert_eq!(display["reviewedSource"]["goods"], "IDR 14000.00");
        assert_eq!(display["reviewedSource"]["landedFee"], "IDR 101.00");
        assert_eq!(display["reviewedSource"]["policyRevision"], 7);
        assert_eq!(
            display["reviewedSource"]["accountMappings"][0]["accountId"],
            "original"
        );
        assert!(body.get("reviewedSource").is_none());
    }
    #[test]
    fn source_reviews_render_for_a_usd_book_under_the_minor_unit() {
        use leptos::prelude::RenderHtml;
        let book = json!({"currency":"USD","minorUnits":2});
        let accounts = json!([{"id":"ar","revision":3,"code":"AR","name":"Receivables"},{"id":"revenue","revision":1,"code":"REV","name":"Revenue"},{"id":"stock","revision":2,"code":"STOCK","name":"Inventory"},{"id":"grni","revision":1,"code":"GRNI","name":"Unbilled goods"},{"id":"fee","revision":1,"code":"FEE","name":"Acquisition fees"},{"id":"cogs","revision":2,"code":"COGS","name":"Cost of goods"}]);
        let policy = |mappings: Value| json!({"source_unit":"minor","mappings":mappings});
        let mut refs = json!({"accounting/accounts":accounts,
            // USD 19.99 invoiced: 1999 source minor units are 1999 book units.
            "accounting/invoice_sources":[{"id":"invoice","effect":"issue","invoiceCode":"INV-USD","amountMinor":"1999","sourceDate":"2026-11-14","policyRevision":1,"policy":policy(json!([{"role":"receivable","accountId":"ar"},{"role":"revenue","accountId":"revenue"},{"role":"payment_clearing","accountId":"clearing"}]))}],
            "accounting/purchase_sources":[{"id":"purchase","effect":"receipt","goodsMinor":"14000","landedFeeMinor":"101","sourceDate":"2026-11-14","policyRevision":1,"policy":policy(json!([{"role":"inventory","accountId":"stock"},{"role":"goods_received_not_invoiced","accountId":"grni"},{"role":"landed_fee_accrual","accountId":"fee"},{"role":"payable","accountId":"ap"}]))}],
            "accounting/stock_sources":[{"id":"stock-source","effect":"fulfillment","amountMinor":"7001","sourceDate":"2026-11-14","policyRevision":1,"legs":[{"accountId":"stock","role":"inventory","debitMinor":"0","creditMinor":"7001"},{"accountId":"cogs","role":"cost_of_goods_sold","debitMinor":"7001","creditMinor":"0"}]}]});
        let invoice = json!({"type":"invoice_source.draft","sourceId":"invoice","accounts":[{"accountId":"ar","expectedAccountRevision":3},{"accountId":"revenue","expectedAccountRevision":1}]});
        assert!(source_review_accounts_match(&invoice, &book, &refs));
        let html = review(invoice.clone(), &book, &refs).to_html();
        assert!(
            html.contains("data-testid=\"invoice-source-summary\""),
            "{html}"
        );
        assert!(html.contains("INV-USD"));
        assert!(html.contains("USD 19.99"), "{html}");
        assert!(!html.contains("USD 1999"), "{html}");
        assert_eq!(
            reviewed_preview(invoice.clone(), &book, &refs)["reviewedSource"]["sourceUnit"],
            "USD minor units posted unchanged as USD book units"
        );
        let purchase = json!({"type":"purchase_source.draft","sourceId":"purchase","code":"RECEIPT","accounts":[{"accountId":"stock","expectedAccountRevision":2},{"accountId":"grni","expectedAccountRevision":1},{"accountId":"fee","expectedAccountRevision":1}]});
        assert!(source_review_accounts_match(&purchase, &book, &refs));
        let summary = purchase_review(&purchase, &book, &refs).unwrap();
        assert_eq!(summary.goods, "USD 140.00");
        assert_eq!(summary.fee, "USD 1.01");
        assert_eq!(summary.lines[0].debit, "USD 141.01");
        let html = review(purchase.clone(), &book, &refs).to_html();
        assert!(
            html.contains("data-testid=\"purchase-source-summary\""),
            "{html}"
        );
        let stock = json!({"type":"stock_source.draft","sourceId":"stock-source","accounts":[{"accountId":"stock","expectedAccountRevision":2},{"accountId":"cogs","expectedAccountRevision":2}]});
        assert!(source_review_accounts_match(&stock, &book, &refs));
        let html = review(stock, &book, &refs).to_html();
        assert!(
            html.contains("data-testid=\"stock-source-summary\""),
            "{html}"
        );
        assert!(html.contains("USD 70.01"), "{html}");
        // A zero-decimal book shows no fraction.
        let yen = json!({"currency":"JPY","minorUnits":0});
        assert_eq!(
            invoice_review(&invoice, &yen, &refs).unwrap().amount,
            "JPY 1999"
        );
        // The unit must be the book's: a whole-Rupiah snapshot is not
        // reviewable on a USD book, nor a minor-unit snapshot on an IDR book.
        let idr = json!({"currency":"IDR","minorUnits":2});
        assert!(invoice_review(&invoice, &idr, &refs).is_none());
        assert!(purchase_review(&purchase, &idr, &refs).is_none());
        refs["accounting/invoice_sources"][0]["policy"]["source_unit"] = json!("whole_idr");
        refs["accounting/purchase_sources"][0]["policy"]["source_unit"] = json!("whole_idr");
        assert!(invoice_review(&invoice, &book, &refs).is_none());
        assert!(purchase_review(&purchase, &book, &refs).is_none());
        assert!(invoice_review(&invoice, &idr, &refs).is_some());
        assert_eq!(
            reviewed_preview(invoice.clone(), &idr, &refs)["reviewedSource"]["sourceUnit"],
            "whole Rupiah converted exactly to IDR book units"
        );
        for malformed in [
            json!({"currency":"usd","minorUnits":2}),
            json!({"currency":"USD"}),
            json!({"currency":"IDR","minorUnits":0}),
            json!({}),
        ] {
            assert_eq!(book_source_unit(&malformed), None, "{malformed}");
        }
    }
    #[test]
    fn human_currency_inputs_and_large_signed_report_values_remain_exact() {
        assert_eq!(parse_amount("123.45", 2), Some("12345".into()));
        assert_eq!(parse_amount("0001.2", 2), Some("120".into()));
        assert_eq!(parse_amount("1", 0), Some("1".into()));
        assert_eq!(parse_amount("0.001", 3), Some("1".into()));
        for bad in [
            "1,25",
            "-1",
            "1.001",
            "1e3",
            " 1",
            ".5",
            "90000000000000.01",
        ] {
            assert_eq!(parse_amount(bad, 2), None);
        }
        assert_eq!(
            decimal("27000000000000000", 2),
            Some("270000000000000.00".into())
        );
        assert_eq!(decimal("-1", 3), Some("-0.001".into()));
        assert_eq!(decimal("0", 2), Some("0.00".into()));
        let command =
            json!({"type":"journal.draft","lines":[{"debitMinor":"12345","creditMinor":"0"}]});
        let review = preview(command.clone(), &json!({"currency":"USD","minorUnits":2}));
        assert_eq!(review["lines"][0]["debit"], "USD 123.45");
        assert_eq!(command["lines"][0]["debitMinor"], "12345");
    }
    #[test]
    fn signed_bank_inputs_preserve_exact_currency_and_refuse_rounded_or_negative_zero() {
        assert_eq!(parse_money("-123.45", 2, true), Some("-12345".into()));
        assert_eq!(parse_money("-0001.20", 2, true), Some("-120".into()));
        assert_eq!(
            parse_money("-90000000000000.00", 2, true),
            Some("-9000000000000000".into())
        );
        assert_eq!(parse_money("0", 2, true), Some("0".into()));
        assert_eq!(parse_money("-1", 2, false), None);
        for bad in ["-0", "-0.00", "-0.001", "+1", "-90000000000000.01", "1e3"] {
            assert_eq!(parse_money(bad, 2, true), None, "{bad}");
        }
    }
}
