//! Human review of source-backed supplier bank movements.
use super::*;
use archava_domain::erp::accounting::payables::{self as rules, Effect};
fn row<'a>(refs: &'a Value, collection: &str, id: &Value) -> Option<&'a Value> {
    refs[format!("accounting/{collection}")]
        .as_array()?
        .iter()
        .find(|r| &r["id"] == id)
}
#[component]
pub(super) fn Balances(work: Workbench) -> impl IntoView {
    view! {{move||{let data=work.data.get();let balances=&data["payablesSummary"];let book=&data["configuredBook"];
        balances.is_object().then(||view!{<section data-testid="payable-current-balances"><h3>"Current supplier payables"</h3>
            <p class="hint">"All captured bills in this company book, across all pages. These are current balances; the report date filter below does not change them. Unimported historical balances and payment terms require separate review."</p>
            <dl class="operator-details"><dt>"Recognized bills after posted corrections"</dt><dd>{amount(&balances["recognizedMinor"],book)}</dd>
            <dt>"Posted payments less posted returns"</dt><dd>{amount(&balances["postedPaymentMinor"],book)}</dd>
            <dt>"Outstanding"</dt><dd>{amount(&balances["outstandingMinor"],book)}</dd>
            <dt>"Reserved in payment drafts"</dt><dd>{amount(&balances["reservedPaymentMinor"],book)}</dd>
            <dt>"Available for a new payment draft"</dt><dd>{amount(&balances["availableMinor"],book)}</dd></dl>
            <p class="hint">{text(&data,"payablesNotice")}</p></section>})
    }}}
}
pub(super) fn valid_review(body: &Value, book: &Value, refs: &Value) -> bool {
    let Some(bank_row) = row(refs, "bank_rows", &body["bankRowId"]) else {
        return false;
    };
    let Some(bank_date) = bank_row["date"].as_str() else {
        return false;
    };
    if book["id"].as_str().is_none_or(str::is_empty) {
        return false;
    }
    let Some(binding) = row(refs, "bank_accounts", &bank_row["bankAccountId"]) else {
        return false;
    };
    let Some(payable) = row(refs, "accounts", &body["payableAccount"]["accountId"]) else {
        return false;
    };
    let Some(cash) = row(refs, "accounts", &body["cashAccount"]["accountId"]) else {
        return false;
    };
    if cash["id"] != binding["accountId"]
        || payable["active"] != true
        || payable["class"] != "liability"
        || cash["active"] != true
        || cash["class"] != "asset"
        || cash["cashRole"] != "cash"
        || payable["revision"] != body["payableAccount"]["expectedAccountRevision"]
        || cash["revision"] != body["cashAccount"]["expectedAccountRevision"]
        || bank_row["bookId"] != book["id"]
    {
        return false;
    }
    let (effect, available) = match body["type"].as_str() {
        Some("payable_payment.draft") => {
            let Some(bill) = row(refs, "payables", &body["sourceId"]) else {
                return false;
            };
            if bill["balance"]["billPosted"] != true
                || bill["documentState"] != "matched"
                || bill["currentDocumentRevision"] != bill["documentRevision"]
                || body["expectedDocumentRevision"] != bill["documentRevision"]
                || payable["id"] != bill["payableAccountId"]
                || bill["sourceDate"]
                    .as_str()
                    .is_none_or(|date| bank_date < date)
            {
                return false;
            }
            (Effect::Payment, bill["balance"]["availableMinor"].as_str())
        }
        Some("payable_return.draft") => {
            let Some(original) = row(refs, "payable_settlements", &body["originalPaymentId"])
            else {
                return false;
            };
            if original["effect"] != "payment"
                || original["state"] != "posted"
                || original["revision"] != body["expectedJournalRevision"]
                || original["cashAccountId"] != cash["id"]
                || original["payableAccountId"] != payable["id"]
                || original["date"]
                    .as_str()
                    .is_none_or(|date| bank_date < date)
            {
                return false;
            }
            (Effect::Return, original["returnAvailableMinor"].as_str())
        }
        _ => return false,
    };
    let Some(available) = available else {
        return false;
    };
    let Ok(capacity) = rules::capacity(available, "0", "0") else {
        return false;
    };
    let amount = body["amountMinor"].as_str().unwrap_or_default();
    rules::claim(&capacity, amount).is_ok()
        && rules::bank_claim(
            effect,
            bank_row["unreservedMinor"].as_str().unwrap_or_default(),
            amount,
        )
        .is_ok()
}

pub(super) fn review(body: &Value, book: &Value, refs: &Value) -> Option<AnyView> {
    let effect = match body["type"].as_str() {
        Some("payable_payment.draft") => Effect::Payment,
        Some("payable_return.draft") => Effect::Return,
        _ => return None,
    };
    if !valid_review(body, book, refs) {
        return Some(view!{<p class="refusal" data-testid="payable-review-refused">"Refresh the bill or original payment, actual bank transaction and both accounts. The posted amount, current versions and available balances must agree before saving."</p>}.into_any());
    }
    let actual = row(refs, "bank_rows", &body["bankRowId"])?;
    let reviewed = row(
        refs,
        if effect == Effect::Payment {
            "payables"
        } else {
            "payable_settlements"
        },
        &body[if effect == Effect::Payment {
            "sourceId"
        } else {
            "originalPaymentId"
        }],
    )?;
    let legs = rules::legs(effect, body["amountMinor"].as_str()?).ok()?;
    let lines = legs
        .into_iter()
        .zip(["payableAccount", "cashAccount"])
        .map(|(leg, key)| {
            let account = row(refs, "accounts", &body[key]["accountId"]);
            PurchaseReviewLine {
                role: if key == "payableAccount" {
                    "Supplier payable"
                } else {
                    "Bank cash"
                }
                .into(),
                account: account
                    .map(|a| format!("{} · {}", text(a, "code"), text(a, "name")))
                    .unwrap_or_default(),
                debit: amount(&json!(leg.debit_minor), book),
                credit: amount(&json!(leg.credit_minor), book),
            }
        })
        .collect();
    let bill = text(reviewed, "code");
    let date = text(actual, "date");
    let reference = text(actual, "externalRef");
    let value = amount(&body["amountMinor"], book);
    let body = body.clone();
    Some(view!{
        <section data-testid="payable-review-summary"><h4>{if effect==Effect::Payment{"Record supplier payment"}else{"Record returned supplier payment"}}</h4>
        <dl class="operator-details"><dt>"Reviewed bill or payment"</dt><dd>{bill}</dd><dt>"Actual bank transaction"</dt><dd>{reference}</dd><dt>"Bank and journal date"</dt><dd>{date}</dd><dt>"Amount"</dt><dd class="source-money">{value}</dd></dl>
        <p class="hint">"Saving records a draft and reserves its amount. Posting needs a separate review. Match its cash line to this bank transaction after posting. This action sends no money."</p></section>
        {source_journal(lines,"payable-source-journal","Proposed supplier payment journal")}
        <details><summary>"Bill, bank evidence and approval references"</summary>{details(body)}</details>
    }.into_any())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn human_payment_review_uses_original_accounts_exact_money_and_unreserved_bank_evidence() {
        let book = json!({"id":"book","currency":"IDR","minorUnits":2});
        let refs = json!({
            "accounting/payables":[{"id":"bill","code":"SUPPLIER-CLAIM","balance":{"billPosted":true,"availableMinor":"700000"},"documentState":"matched","documentRevision":2,"currentDocumentRevision":2,"payableAccountId":"ap","sourceDate":"2026-11-11"}],
            "accounting/bank_rows":[{"id":"row","bankAccountId":"bank","bookId":"book","date":"2026-11-11","unreservedMinor":"-700000","externalRef":"ACTUAL-WITHDRAWAL"}],
            "accounting/bank_accounts":[{"id":"bank","accountId":"cash"}],
            "accounting/accounts":[{"id":"ap","revision":3,"active":true,"class":"liability","code":"AP","name":"Original supplier payable"},{"id":"cash","revision":1,"active":true,"class":"asset","cashRole":"cash","code":"BANK","name":"Reviewed bank"}]
        });
        let command = json!({"type":"payable_payment.draft","sourceId":"bill","expectedDocumentRevision":2,"bankRowId":"row","payableAccount":{"accountId":"ap","expectedAccountRevision":3},"cashAccount":{"accountId":"cash","expectedAccountRevision":1},"amountMinor":"700000"});
        assert!(valid_review(&command, &book, &refs));
        let html = review(&command, &book, &refs).unwrap().to_html();
        assert!(html.contains("IDR 7000.00"));
        assert!(html.contains("AP · Original supplier payable"));
        assert!(html.contains("ACTUAL-WITHDRAWAL"));
        assert!(
            html.find("IDR 7000.00").unwrap()
                < html
                    .find("Bill, bank evidence and approval references")
                    .unwrap()
        );
        for (pointer, value) in [
            ("/accounting~1bank_rows/0/unreservedMinor", json!("-699999")),
            ("/accounting~1bank_rows/0/unreservedMinor", json!("700000")),
            ("/accounting~1bank_rows/0/date", Value::Null),
            ("/accounting~1payables/0/currentDocumentRevision", json!(3)),
            ("/accounting~1payables/0/balance/billPosted", json!(false)),
            ("/accounting~1accounts/0/revision", json!(4)),
        ] {
            let mut changed = refs.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(!valid_review(&command, &book, &changed), "{pointer}");
        }
    }
}
