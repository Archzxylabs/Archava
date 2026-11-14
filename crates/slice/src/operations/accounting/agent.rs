//! Human financial review from locally authorized goal facts.
use super::*;
/// Local authorized facts for both the action editor and human goal review.
pub(crate) fn agent_references(context: &Value) -> Value {
    let mut refs = json!({"__companyId":context["companyId"]});
    for source in context["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["available"] == true)
    {
        let module = text(source, "module");
        for (collection, rows) in source["data"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, v)| v.is_array())
        {
            refs[format!("{module}/{collection}")] = rows.clone();
        }
        if module == "accounting" {
            let data = &source["data"];
            let book = &data["configuredBook"];
            refs["__currency"] = book["currency"].clone();
            refs["__minorUnits"] = book["minorUnits"].clone();
            for (label, key) in [
                ("purchase", "purchasePolicy"),
                ("stock", "stockPolicy"),
                ("invoice", "invoicePolicy"),
            ] {
                refs[format!("__{label}PolicyBook")] = book["id"].clone();
                refs[format!("__{label}PolicyRevision")] = match data.get(key) {
                    Some(Value::Null) => json!(0),
                    Some(v) => v["revision"].clone(),
                    None => Value::Null,
                };
            }
        }
    }
    refs
}
pub(crate) fn agent_step_review(context: &Value, step: &Value) -> AnyView {
    let operation = step["tool"]
        .as_str()
        .unwrap_or_default()
        .strip_prefix("accounting.")
        .unwrap_or_default();
    let refs = agent_references(context);
    let book = context["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["module"] == "accounting" && s["available"] == true)
        .map(|s| &s["data"]["configuredBook"])
        .cloned()
        .unwrap_or(Value::Null);
    let mut command = step["input"].clone();
    command["type"] = json!(operation);
    let body = if operation == "journal.draft" {
        let lines = command["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| {
                let account = refs["accounting/accounts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|a| a["id"] == l["accountId"]);
                PurchaseReviewLine {
                    role: "Journal line".into(),
                    account: account
                        .map(|a| format!("{} · {}", text(a, "code"), text(a, "name")))
                        .unwrap_or_else(|| "Account details unavailable".into()),
                    debit: amount(&l["debitMinor"], &book),
                    credit: amount(&l["creditMinor"], &book),
                }
            })
            .collect();
        view!{<p>{format!("Draft journal · {}",text(&command,"date"))}</p>{source_journal(lines,"agent-financial-journal","Journal proposed for the reviewed book")}<details><summary>"Draft and approval references"</summary>{details(preview(command,&book))}</details>}.into_any()
    } else if matches!(
        operation,
        "journal.post" | "journal.reverse" | "journal.void"
    ) {
        if command["id"].is_object() {
            view!{<p class="hint">"This step uses the journal produced by an earlier reviewed step. Review that step’s exact amounts and accounts; current posting permissions still apply."</p>{details(command)}}.into_any()
        } else {
            let journal = refs["accounting/journals"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["id"] == command["id"]);
            if let Some(journal) = journal.filter(|j| j["lines"].is_array()) {
                let reverse = operation == "journal.reverse";
                let lines = journal["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| PurchaseReviewLine {
                        role: if reverse {
                            "Reversal line"
                        } else {
                            "Existing journal line"
                        }
                        .into(),
                        account: format!("{} · {}", text(l, "accountCode"), text(l, "accountName")),
                        debit: amount(
                            &l[if reverse { "creditMinor" } else { "debitMinor" }],
                            &book,
                        ),
                        credit: amount(
                            &l[if reverse { "debitMinor" } else { "creditMinor" }],
                            &book,
                        ),
                    })
                    .collect();
                view!{<p>{format!("{} · {} · {}",operation,text(journal,"code"),text(journal,"date"))}</p>{source_journal(lines,"agent-financial-journal","Exact journal amounts and original accounts")}<details><summary>"Journal and approval references"</summary>{details(command)}</details>}.into_any()
            } else {
                view!{<p class="refusal">"Exact journal lines are unavailable. Refresh the plan or review this journal in Accounting before approving."</p>}.into_any()
            }
        }
    } else {
        review(command, &book, &refs)
    };
    view!{<section data-testid="agent-financial-review"><h4>"Financial impact for review"</h4><p>{format!("Company: {} · book: {} · currency: {}",text(&book,"companyName"),text(&book,"name"),text(&book,"currency"))}</p>{body}</section>}.into_any()
}
pub(crate) fn agent_step_ready(context: &Value, step: &Value) -> bool {
    let Some(op) = step["tool"]
        .as_str()
        .and_then(|s| s.strip_prefix("accounting."))
    else {
        return true;
    };
    let refs = agent_references(context);
    let book = context["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["module"] == "accounting" && s["available"] == true)
        .map(|s| &s["data"]["configuredBook"])
        .cloned()
        .unwrap_or(Value::Null);
    let mut body = step["input"].clone();
    body["type"] = json!(op);
    match op {
        "invoice_terms.set"
        | "supplier_terms.set"
        | "invoice_source.draft"
        | "purchase_source.draft"
        | "stock_source.draft"
        | "payable_payment.draft"
        | "payable_return.draft"
        | "clearing_settlement.draft" => source_review_accounts_match(&body, &book, &refs),
        "journal.post" | "journal.reverse" | "journal.void" => {
            body["id"].is_object()
                || refs["accounting/journals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|j| j["id"] == body["id"])
                    .is_some_and(|j| {
                        j["lines"].as_array().is_some_and(|l| !l.is_empty())
                            && book["currency"].is_string()
                    })
        }
        "journal.draft" => {
            if !book["currency"].is_string() {
                return false;
            }
            let Some(lines) = body["lines"].as_array().filter(|l| l.len() >= 2) else {
                return false;
            };
            let mut debit = 0_i128;
            let mut credit = 0_i128;
            for line in lines {
                let Some(d) = line["debitMinor"]
                    .as_str()
                    .and_then(|v| archava_domain::erp::accounting::minor(v).ok())
                else {
                    return false;
                };
                let Some(c) = line["creditMinor"]
                    .as_str()
                    .and_then(|v| archava_domain::erp::accounting::minor(v).ok())
                else {
                    return false;
                };
                if (d == 0) == (c == 0) {
                    return false;
                }
                if !line["accountId"].is_object()
                    && !refs["accounting/accounts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|a| {
                            a["id"] == line["accountId"]
                                && a["revision"] == line["expectedAccountRevision"]
                                && a["active"] != false
                        })
                {
                    return false;
                }
                debit += i128::from(d);
                credit += i128::from(c);
            }
            debit == credit && debit > 0
        }
        _ => true,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_goal_review_shows_exact_money_and_original_accounts_before_references() {
        let id = "journal-reference-for-reviewed-posting";
        let context = json!({"companyId":"company","sources":[{"module":"accounting","available":true,"data":{"configuredBook":{"id":"book","companyName":"Reviewed Company","name":"Official book","currency":"IDR","minorUnits":2},"journals":[{"id":id,"code":"J-ORIGINAL","date":"2026-11-09","revision":1,"lines":[{"accountCode":"INV","accountName":"Original inventory <account>","debitMinor":"705100","creditMinor":"0"},{"accountCode":"CAP","accountName":"Original capital","debitMinor":"0","creditMinor":"705100"}]}]}}]});
        let step = json!({"tool":"accounting.journal.post","input":{"id":id,"expectedRevision":1,"reason":"Review original journal"}});
        assert!(agent_step_ready(&context, &step));
        let html = agent_step_review(&context, &step).to_html();
        assert!(html.contains("IDR 7051.00"));
        assert!(html.contains("Original inventory &lt;account&gt;"));
        assert!(html.contains("Reviewed Company"));
        assert!(html.find("IDR 7051.00").unwrap() < html.find(id).unwrap());
        let mut unavailable = context.clone();
        unavailable["sources"][0]["data"]["journals"][0]
            .as_object_mut()
            .unwrap()
            .remove("lines");
        assert!(!agent_step_ready(&unavailable, &step));
        let refs = agent_references(&context);
        assert_eq!(refs["__currency"], "IDR");
        assert_eq!(refs["__companyId"], "company");
        assert_eq!(refs["__minorUnits"], 2);
    }
    #[test]
    fn draft_review_refuses_unbalanced_values_and_wrong_current_account_versions() {
        let context = json!({"sources":[{"module":"accounting","available":true,"data":{"configuredBook":{"currency":"USD","minorUnits":2},"accounts":[{"id":"a","revision":3,"active":true},{"id":"b","revision":2,"active":true}]}}]});
        let mut step = json!({"tool":"accounting.journal.draft","input":{"lines":[{"accountId":"a","expectedAccountRevision":3,"debitMinor":"9000000000000000","creditMinor":"0"},{"accountId":"b","expectedAccountRevision":2,"debitMinor":"0","creditMinor":"9000000000000000"}]}});
        assert!(agent_step_ready(&context, &step));
        step["input"]["lines"][1]["creditMinor"] = json!("8999999999999999");
        assert!(!agent_step_ready(&context, &step));
        step["input"]["lines"][1]["creditMinor"] = json!("9000000000000000");
        step["input"]["lines"][0]["expectedAccountRevision"] = json!(2);
        assert!(!agent_step_ready(&context, &step));
    }
}
