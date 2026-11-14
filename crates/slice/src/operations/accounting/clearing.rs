//! Human bank settlement review, using the exact original collection or refund
//! source: a verified provider event, or a manual receipt, which is worded as
//! manual and never as verified.
use super::*;
use archava_domain::erp::accounting::clearing::{self as rules, Effect};
fn record<'a>(refs: &'a Value, collection: &str, key: &Value) -> Option<&'a Value> {
    refs[format!("accounting/{collection}")]
        .as_array()?
        .iter()
        .find(|r| &r["id"] == key)
}
pub(super) fn valid_review(body: &Value, book: &Value, refs: &Value) -> bool {
    let Some(source) = record(refs, "clearing_sources", &body["sourceId"]) else {
        return false;
    };
    let Some(row) = record(refs, "bank_rows", &body["bankRowId"]) else {
        return false;
    };
    let Some(binding) = record(refs, "bank_accounts", &row["bankAccountId"]) else {
        return false;
    };
    let effect = match source["effect"].as_str() {
        Some("collection") => Effect::Collection,
        Some("refund") => Effect::Refund,
        _ => return false,
    };
    let Some(date) = row["date"].as_str() else {
        return false;
    };
    if source["balance"]["sourcePosted"] != true
        || book["id"].as_str().is_none_or(str::is_empty)
        || source["bookId"] != book["id"]
        || row["bookId"] != book["id"]
        || source["sourceDate"].as_str().is_none_or(|d| date < d)
        || source["clearingAccountId"] != body["clearingAccount"]["accountId"]
        || binding["accountId"] != body["cashAccount"]["accountId"]
    {
        return false;
    }
    let gross = body["grossMinor"].as_str().unwrap_or_default();
    let fee = body["feeMinor"].as_str().unwrap_or_default();
    let Ok(legs) = rules::legs(effect, gross, fee) else {
        return false;
    };
    if (legs.len() == 3) != body["feeAccount"].is_object() {
        return false;
    }
    for (key, class, role) in [
        ("clearingAccount", "asset", "none"),
        ("cashAccount", "asset", "cash"),
        ("feeAccount", "expense", "operating"),
    ] {
        if key == "feeAccount" && legs.len() == 2 {
            continue;
        }
        let Some(account) = record(refs, "accounts", &body[key]["accountId"]) else {
            return false;
        };
        if account["active"] != true
            || account["class"] != class
            || account["cashRole"] != role
            || account["revision"] != body[key]["expectedAccountRevision"]
        {
            return false;
        }
    }
    rules::claim(
        source["balance"]["availableGrossMinor"]
            .as_str()
            .unwrap_or_default(),
        "0",
        "0",
        gross,
    )
    .is_ok()
        && rules::bank_claim(
            effect,
            row["unreservedMinor"].as_str().unwrap_or_default(),
            gross,
            fee,
        )
        .is_ok()
}
pub(super) fn review(body: &Value, book: &Value, refs: &Value) -> Option<AnyView> {
    if body["type"] != "clearing_settlement.draft" {
        return None;
    }
    if !valid_review(body, book, refs) {
        return Some(view!{<p class="refusal" data-testid="clearing-review-refused">"Refresh the posted provider source, actual bank transaction and original accounts. Review the exact gross, explicit fee and current available amounts before saving."</p>}.into_any());
    }
    let source = record(refs, "clearing_sources", &body["sourceId"])?;
    let bank = record(refs, "bank_rows", &body["bankRowId"])?;
    let effect = if source["effect"] == "collection" {
        Effect::Collection
    } else {
        Effect::Refund
    };
    // A manual receipt is a person's statement: nothing about it is "verified".
    let manual = source["origin"] == "manual";
    let legs = rules::legs(
        effect,
        body["grossMinor"].as_str()?,
        body["feeMinor"].as_str()?,
    )
    .ok()?;
    let cash = if effect == Effect::Collection {
        amount(&json!(legs[1].debit_minor), book)
    } else {
        amount(&json!(legs[1].credit_minor), book)
    };
    let lines = legs
        .into_iter()
        .map(|leg| {
            let (key, label) = match leg.role {
                rules::Role::Clearing if manual => (
                    "clearingAccount",
                    "Original payment clearing of the manual receipt",
                ),
                rules::Role::Clearing => ("clearingAccount", "Original verified payment clearing"),
                rules::Role::Cash => ("cashAccount", "Actual bank cash"),
                rules::Role::Fee if manual => ("feeAccount", "Explicit fee"),
                rules::Role::Fee => ("feeAccount", "Explicit gateway fee"),
            };
            let account = record(refs, "accounts", &body[key]["accountId"]);
            PurchaseReviewLine {
                role: label.into(),
                account: account
                    .map(|a| format!("{} · {}", text(a, "code"), text(a, "name")))
                    .unwrap_or_default(),
                debit: amount(&json!(leg.debit_minor), book),
                credit: amount(&json!(leg.credit_minor), book),
            }
        })
        .collect();
    let source = text(source, "code");
    let bankref = text(bank, "externalRef");
    let date = text(bank, "date");
    let gross = amount(&body["grossMinor"], book);
    let fee = amount(&body["feeMinor"], book);
    let body = body.clone();
    let (heading, source_label, gross_label, hint, journal, references) = if manual {
        (
            "Record the bank settlement of a manual receipt",
            "Original manual receipt source",
            "Gross amount received",
            "Saving creates a draft. Separate posting moves the manually recorded money from clearing to actual bank cash without posting customer receivables or revenue again. Bank matching is another reviewed action. This action sends no money.",
            "Proposed manual receipt to bank journal",
            "Manual receipt source, bank evidence and approval references",
        )
    } else {
        (
            "Record actual gateway bank settlement",
            "Original verified source",
            "Gross provider money",
            "Saving creates a draft. Separate posting transfers verified clearing to actual bank cash without posting customer receivables or revenue again. Bank matching is another reviewed action. This action sends no money.",
            "Proposed verified source to bank journal",
            "Provider source, bank evidence and approval references",
        )
    };
    Some(view!{<section data-testid="clearing-review-summary"><h4>{heading}</h4><dl class="operator-details"><dt>{source_label}</dt><dd>{source}</dd><dt>"Actual bank transaction"</dt><dd>{bankref}</dd><dt>"Journal date"</dt><dd>{date}</dd><dt>{gross_label}</dt><dd>{gross}</dd><dt>"Explicit fee"</dt><dd>{fee}</dd><dt>"Actual bank amount"</dt><dd>{cash}</dd></dl>
        <p class="hint">{hint}</p></section>
        {source_journal(lines,"clearing-source-journal",journal)}<details><summary>{references}</summary>{details(body)}</details>}.into_any())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gateway_review_shows_original_gross_explicit_fee_and_actual_net_cash_before_references() {
        let book = json!({"id":"book","currency":"IDR","minorUnits":2});
        let refs = json!({"accounting/clearing_sources":[{"id":"source","bookId":"book","code":"INVOICE-VERIFIED","effect":"collection","sourceDate":"2026-11-11","clearingAccountId":"clearing","balance":{"sourcePosted":true,"availableGrossMinor":"3000000"}}],
            "accounting/bank_rows":[{"id":"row","bookId":"book","bankAccountId":"bank","date":"2026-11-11","unreservedMinor":"2980000","externalRef":"ACTUAL-NET-COLLECTION"}],"accounting/bank_accounts":[{"id":"bank","accountId":"cash"}],
            "accounting/accounts":[{"id":"clearing","active":true,"revision":2,"class":"asset","cashRole":"none","code":"CLEARING","name":"Original verified clearing"},{"id":"cash","active":true,"revision":1,"class":"asset","cashRole":"cash","code":"BANK","name":"Actual bank cash"},{"id":"fee","active":true,"revision":1,"class":"expense","cashRole":"operating","code":"FEE","name":"Explicit reviewed fee"}]});
        let body = json!({"type":"clearing_settlement.draft","sourceId":"source","bankRowId":"row","clearingAccount":{"accountId":"clearing","expectedAccountRevision":2},"cashAccount":{"accountId":"cash","expectedAccountRevision":1},"feeAccount":{"accountId":"fee","expectedAccountRevision":1},"grossMinor":"3000000","feeMinor":"20000"});
        assert!(valid_review(&body, &book, &refs));
        let html = review(&body, &book, &refs).unwrap().to_html();
        for amount in ["IDR 30000.00", "IDR 29800.00", "IDR 200.00"] {
            assert!(html.contains(amount));
            assert!(
                html.find(amount).unwrap()
                    < html
                        .find("Provider source, bank evidence and approval references")
                        .unwrap()
            );
        }
        assert!(html.contains("Original verified clearing"));
        assert!(html.contains("ACTUAL-NET-COLLECTION"));
        for (pointer, value) in [
            (
                "/accounting~1clearing_sources/0/balance/sourcePosted",
                json!(false),
            ),
            ("/accounting~1bank_rows/0/unreservedMinor", json!("2979999")),
            ("/accounting~1accounts/0/revision", json!(3)),
            ("/accounting~1accounts/2/class", json!("revenue")),
        ] {
            let mut changed = refs.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(!valid_review(&body, &book, &changed));
        }
        let mut missing_fee = body;
        missing_fee.as_object_mut().unwrap().remove("feeAccount");
        assert!(!valid_review(&missing_fee, &book, &refs));
    }
    #[test]
    fn a_manual_receipt_source_is_reviewed_as_manual_and_never_as_verified() {
        let book = json!({"id":"book","currency":"IDR","minorUnits":2});
        let refs = json!({"accounting/clearing_sources":[{"id":"source","bookId":"book","code":"INV-7 · collection","effect":"collection","origin":"manual","method":"bank_transfer","sourceDate":"2026-11-13","clearingAccountId":"clearing","balance":{"sourcePosted":true,"availableGrossMinor":"3000000"}}],
            "accounting/bank_rows":[{"id":"row","bookId":"book","bankAccountId":"bank","date":"2026-11-13","unreservedMinor":"3000000","externalRef":"TRF-8842"}],"accounting/bank_accounts":[{"id":"bank","accountId":"cash"}],
            "accounting/accounts":[{"id":"clearing","active":true,"revision":1,"class":"asset","cashRole":"none","code":"CLEARING","name":"Payment clearing"},{"id":"cash","active":true,"revision":1,"class":"asset","cashRole":"cash","code":"BANK","name":"Bank"}]});
        let body = json!({"type":"clearing_settlement.draft","sourceId":"source","bankRowId":"row","clearingAccount":{"accountId":"clearing","expectedAccountRevision":1},"cashAccount":{"accountId":"cash","expectedAccountRevision":1},"grossMinor":"3000000","feeMinor":"0"});
        assert!(valid_review(&body, &book, &refs));
        let html = review(&body, &book, &refs).unwrap().to_html();
        assert!(
            html.contains("Record the bank settlement of a manual receipt"),
            "{html}"
        );
        assert!(html.contains("Original manual receipt source"), "{html}");
        assert!(
            html.contains("Proposed manual receipt to bank journal"),
            "{html}"
        );
        assert!(html.contains("IDR 30000.00"), "{html}");
        assert!(!html.to_lowercase().contains("verified"), "{html}");
        assert!(!html.to_lowercase().contains("gateway"), "{html}");
        // A bank row dated before the day the money was received is not offered.
        let mut early = refs.clone();
        *early.pointer_mut("/accounting~1bank_rows/0/date").unwrap() = json!("2026-11-12");
        assert!(!valid_review(&body, &book, &early));
    }
}
