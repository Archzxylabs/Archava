//! Human bank review; server-side authority and remaining amounts are checked again on save.
use super::*;
use archava_domain::erp::accounting::bank::{allocate, statement_totals, Statement};

fn record<'a>(refs: &'a Value, collection: &str, key: &Value) -> Option<&'a Value> {
    refs[format!("accounting/{collection}")]
        .as_array()?
        .iter()
        .find(|r| &r["id"] == key)
}

fn cash_account<'a>(body: &Value, refs: &'a Value) -> Option<&'a Value> {
    let reference = &body["account"];
    record(refs, "accounts", &reference["accountId"]).filter(|a| {
        a["active"] == true
            && a["class"] == "asset"
            && a["cashRole"] == "cash"
            && a["revision"] == reference["expectedAccountRevision"]
    })
}

pub(super) fn valid_review(body: &Value, book: &Value, refs: &Value) -> bool {
    match body["type"].as_str() {
        Some("bank_policy.set") => {
            let Some(bank) = record(refs, "bank_accounts", &body["bankAccountId"]) else {
                return false;
            };
            bank["bankPolicyRevision"] == body["expectedRevision"]
                && archava_domain::erp::accommodation::parse_calendar_date(
                    body["fromDate"].as_str().unwrap_or_default(),
                )
                .is_some()
                && body["independentReview"].is_boolean()
        }
        Some("bank_reconciliation.confirm") => super::bank_check::valid_review(body, book, refs),
        Some("bank_account.create") => {
            body["bookId"] == book["id"] && cash_account(body, refs).is_some()
        }
        Some("bank_statement.import") => {
            let Some(bank) = record(refs, "bank_accounts", &body["bankAccountId"]) else {
                return false;
            };
            bank["accountId"] == body["account"]["accountId"]
                && cash_account(body, refs).is_some()
                && serde_json::from_value::<Statement>(body["statement"].clone())
                    .ok()
                    .is_some_and(|s| {
                        statement_totals(&s, book["currency"].as_str().unwrap_or_default()).is_ok()
                    })
        }
        Some("bank_allocation.record") => {
            let Some(row) = record(refs, "bank_rows", &body["bankRowId"]) else {
                return false;
            };
            let Some(bank) = record(refs, "bank_accounts", &row["bankAccountId"]) else {
                return false;
            };
            let Some(journal) = record(refs, "journals", &body["journalId"]) else {
                return false;
            };
            let Some(leg) = journal["cashLegs"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|l| l["ordinal"] == body["journalLineOrdinal"])
            else {
                return false;
            };
            let debit = leg["debitMinor"]
                .as_str()
                .and_then(|v| archava_domain::erp::accounting::minor(v).ok());
            let credit = leg["creditMinor"]
                .as_str()
                .and_then(|v| archava_domain::erp::accounting::minor(v).ok());
            let Some((debit, credit)) = debit.zip(credit) else {
                return false;
            };
            if !((debit > 0 && credit == 0) || (credit > 0 && debit == 0)) {
                return false;
            }
            journal["state"] == "posted"
                && journal["revision"] == body["expectedJournalRevision"]
                && bank["accountId"] == body["account"]["accountId"]
                && leg["accountId"] == bank["accountId"]
                && cash_account(body, refs).is_some()
                && allocate(
                    row["remainingMinor"].as_str().unwrap_or_default(),
                    &(debit - credit).to_string(),
                    body["amountMinor"].as_str().unwrap_or_default(),
                )
                .is_ok()
        }
        Some("bank_allocation.reverse") => record(refs, "bank_allocations", &body["id"])
            .is_some_and(|a| a["state"] == "matched" && a["revision"] == body["expectedRevision"]),
        _ => false,
    }
}

pub(super) fn review(body: &Value, book: &Value, refs: &Value) -> Option<AnyView> {
    let operation = body["type"].as_str()?;
    if !operation.starts_with("bank_") {
        return None;
    }
    if operation == "bank_reconciliation.confirm" {
        let current = &refs["__bankReadiness"];
        if !current.is_object() {
            return Some(view!{<p class="refusal">"Load and review the current bank-period controls before confirming."</p>}.into_any());
        }
        let summary = super::bank_check::summary(current, book);
        let valid = valid_review(body, book, refs);
        let references = body.clone();
        return Some(view!{{summary}{(!valid).then(||view!{<p class="refusal">"These controls cannot be confirmed with the current selection. Refresh the records and controls, resolve any failed checks, and review again."</p>})}<details><summary>"Reconciliation review references"</summary>{details(references)}</details>}.into_any());
    }
    if !valid_review(body, book, refs) {
        return Some(view!{<p class="refusal" data-testid="bank-review-refused">"Complete the bank details and refresh the cash account, statement and journal records before saving. The statement total and selected account must agree."</p>}.into_any());
    }
    let references = body.clone();
    let view = match operation {
        "bank_policy.set" => {
            let bank = record(refs, "bank_accounts", &body["bankAccountId"])?;
            let reviewer = if body["independentReview"] == true {
                "A reviewer who did not import the statement must confirm."
            } else {
                "An authorized importer may also review their own statement."
            };
            view!{<section data-testid="bank-policy-review"><h4>{text(bank,"name")}</h4><p>{format!("Reconciliation starts at the financial period beginning {}.",text(body,"fromDate"))}</p><p>{reviewer}</p><p>"Earlier posted cash forms the opening balance to compare with the statement. The first approved reconciliation seals this opening basis."</p></section>}.into_any()
        }
        "bank_statement.import" => {
            let statement: Statement = serde_json::from_value(body["statement"].clone()).ok()?;
            let totals = statement_totals(&statement, book["currency"].as_str()?).ok()?;
            let bank = record(refs, "bank_accounts", &body["bankAccountId"])?;
            let bank_name = format!("{} · {}", text(bank, "code"), text(bank, "name"));
            let opening = amount(&json!(statement.opening_minor), book);
            let closing = amount(&json!(statement.closing_minor), book);
            let inflow = amount(&json!(totals.inflow_minor), book);
            let outflow = amount(&json!(totals.outflow_minor), book);
            let row_count = totals.row_count;
            let rows = statement.rows.into_iter().map(|r| {
                let movement = amount(&json!(r.amount_minor), book);
                view!{<tr><td>{r.date}</td><td>{r.external_ref}</td><td>{r.description}</td><td>{movement}</td></tr>}
            }).collect::<Vec<_>>();
            view!{<section data-testid="bank-import-review"><h4>{bank_name}</h4>
                <p>{format!("Statement {} · {} to {}",statement.reference,statement.from_date,statement.to_date)}</p>
                <dl class="operator-details"><dt>"Opening balance"</dt><dd>{opening}</dd><dt>"Money in"</dt><dd>{inflow}</dd><dt>"Money out"</dt><dd>{outflow}</dd><dt>"Closing balance"</dt><dd>{closing}</dd><dt>"Transactions"</dt><dd>{row_count}</dd></dl>
                <p>"The opening balance plus all transactions equals the closing balance. Saving records statement evidence; use reconciliation to match existing posted cash entries."</p>
                <div class="ops-table-wrap"><table class="ops-table"><caption>"Complete statement transactions"</caption><thead><tr><th>"Date"</th><th>"Bank reference"</th><th>"Description"</th><th>"Movement"</th></tr></thead><tbody>{rows}</tbody></table></div>
            </section>}.into_any()
        }
        "bank_account.create" => {
            let account = cash_account(body, refs)?;
            let account_name = format!("{} · {}", text(account, "code"), text(account, "name"));
            view!{<section data-testid="bank-account-review"><h4>{text(body,"name")}</h4><p>{format!("Cash ledger account: {account_name}")}</p><p>"This registers the account for reviewed manual statement imports and matching."</p></section>}.into_any()
        }
        "bank_allocation.record" => {
            let row = record(refs, "bank_rows", &body["bankRowId"])?;
            let journal = record(refs, "journals", &body["journalId"])?;
            let account = cash_account(body, refs)?;
            let account_name = format!("{} · {}", text(account, "code"), text(account, "name"));
            let matched = amount(&body["amountMinor"], book);
            let remaining = amount(&row["remainingMinor"], book);
            view!{<section data-testid="bank-match-review"><h4>"Match existing cash evidence"</h4><dl class="operator-details"><dt>"Statement transaction"</dt><dd>{text(row,"externalRef")}</dd><dt>"Statement amount remaining"</dt><dd>{remaining}</dd><dt>"Posted journal"</dt><dd>{text(journal,"code")}</dd><dt>"Cash account"</dt><dd>{account_name}</dd><dt>"Amount to match"</dt><dd>{matched}</dd></dl><p>"Saving links the statement transaction and posted cash entry. Current remaining amounts are checked again before saving."</p></section>}.into_any()
        }
        "bank_allocation.reverse" => {
            let matched = record(refs, "bank_allocations", &body["id"])?;
            let value = amount(&matched["amountMinor"], book);
            view!{<section data-testid="bank-unmatch-review"><h4>"Reverse this cash match"</h4><p>{format!("Amount: {value}")}</p><p>"The original match stays in history. Its statement and cash amounts become available for a new reviewed match."</p></section>}.into_any()
        }
        _ => return None,
    };
    Some(view!{ {view}<details><summary>"Statement and review references"</summary>{details(references)}</details> }.into_any())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn refs() -> Value {
        json!({"accounting/accounts":[{"id":"cash","code":"BANK","name":"Reviewed cash account","active":true,"class":"asset","cashRole":"cash","revision":3}],
            "accounting/bank_accounts":[{"id":"bank","accountId":"cash","code":"LOCAL","name":"Manual bank"}],
            "accounting/bank_rows":[{"id":"row","bankAccountId":"bank","externalRef":"original-reference","remainingMinor":"12345"}],
            "accounting/journals":[{"id":"journal","code":"POSTED","state":"posted","revision":2,"cashLegs":[{"ordinal":1,"accountId":"cash","debitMinor":"12345","creditMinor":"0"}]}]})
    }
    #[test]
    fn import_review_requires_whole_control_total_currency_and_current_cash_reference() {
        let book = json!({"id":"book","currency":"USD","minorUnits":2});
        let input = json!({"type":"bank_statement.import","bankAccountId":"bank","account":{"accountId":"cash","expectedAccountRevision":3},"statement":{"reference":"NOV","fromDate":"2026-11-01","toDate":"2026-11-30","currency":"USD","openingMinor":"0","closingMinor":"12345","rows":[{"externalRef":"original","date":"2026-11-09","amountMinor":"12345"}]}});
        let original = input.clone();
        assert!(valid_review(&input, &book, &refs()));
        assert_eq!(input, original);
        for (field, value) in [("closingMinor", json!("12344")), ("currency", json!("EUR"))] {
            let mut bad = input.clone();
            bad["statement"][field] = value;
            assert!(!valid_review(&bad, &book, &refs()));
        }
        let mut bad = input;
        bad["account"]["expectedAccountRevision"] = json!(2);
        assert!(!valid_review(&bad, &book, &refs()));
    }
    #[test]
    fn match_review_checks_posting_direction_amount_and_the_exact_bank_cash_leg() {
        let book = json!({"id":"book","currency":"USD","minorUnits":2});
        let input = json!({"type":"bank_allocation.record","bankRowId":"row","journalId":"journal","journalLineOrdinal":1,"expectedJournalRevision":2,"account":{"accountId":"cash","expectedAccountRevision":3},"amountMinor":"12345"});
        assert!(valid_review(&input, &book, &refs()));
        for (field, value) in [
            ("amountMinor", json!("12346")),
            ("expectedJournalRevision", json!(1)),
            ("journalLineOrdinal", json!(2)),
        ] {
            let mut bad = input.clone();
            bad[field] = value;
            assert!(!valid_review(&bad, &book, &refs()));
        }
        let mut draft = refs();
        draft["accounting/journals"][0]["state"] = json!("draft");
        assert!(!valid_review(&input, &book, &draft));
        let mut other = refs();
        other["accounting/journals"][0]["cashLegs"][0]["accountId"] = json!("other-cash");
        assert!(!valid_review(&input, &book, &other));
        let mut withdrawal = refs();
        withdrawal["accounting/bank_rows"][0]["remainingMinor"] = json!("-12345");
        assert!(!valid_review(&input, &book, &withdrawal));
    }
}
