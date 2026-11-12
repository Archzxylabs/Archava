//! Current bank controls are read locally for human review, never supplied by a model.
use super::*;

pub(super) fn valid_review(body: &Value, book: &Value, refs: &Value) -> bool {
    let value = &refs["__bankReadiness"];
    value["bankPolicyId"] == body["bankPolicyId"]
        && value["periodId"] == body["periodId"]
        && value["policyRevision"] == body["expectedPolicyRevision"]
        && value["periodRevision"] == body["expectedPeriodRevision"]
        && value["check"]["bookId"] == book["id"]
        && value["check"]["evidenceSha256"].is_string()
        && value["check"]["evidenceSha256"] == body["expectedCheckHash"]
        && value["check"]["ready"] == true
        && value["canConfirm"] == true
        && value["periodState"] == "open"
}

pub(super) fn summary(value: &Value, book: &Value) -> AnyView {
    let check = &value["check"];
    let heading = format!(
        "{} · {}",
        text(&value["bankAccount"], "name"),
        text(value, "periodCode")
    );
    let opening_bank = amount(&check["openingBankMinor"], book);
    let opening_ledger = amount(&check["openingLedgerMinor"], book);
    let closing_bank = amount(&check["closingBankMinor"], book);
    let closing_ledger = amount(&check["closingLedgerMinor"], book);
    let statement_remaining = amount(&check["unmatchedStatementMinor"], book);
    let cash_remaining = amount(&check["unmatchedCashMinor"], book);
    let conditions = [
        (
            "completeStatementCoverage",
            "Statements cover the whole period",
        ),
        ("openingAgrees", "Opening bank and book balances agree"),
        ("closingAgrees", "Closing bank and book balances agree"),
        (
            "allStatementRowsMatched",
            "All statement transactions are matched",
        ),
        (
            "allPeriodCashLegsResolved",
            "Posted cash entries are matched or explicitly reversed",
        ),
    ];
    let checks = conditions
        .into_iter()
        .map(|(key, label)| {
            let result = if check["checks"][key] == true {
                "Passed"
            } else {
                "Needs attention"
            };
            view! {<li>{format!("{label}: {result}")}</li>}
        })
        .collect::<Vec<_>>();
    let coverage = format!(
        "{} of {} days; {} statement(s)",
        integer(check, "coveredDays"),
        integer(check, "periodDays"),
        integer(check, "statementCount")
    );
    let review_state = if value["reviewed"] == true {
        "Current evidence has a recorded review."
    } else {
        "Current evidence needs a fresh review."
    };
    view!{<section data-testid="bank-period-controls"><h4>{heading}</h4><p>{format!("{} to {}",text(check,"from"),text(check,"to"))}</p>
        <dl class="operator-details"><dt>"Statement coverage"</dt><dd>{coverage}</dd><dt>"Opening bank balance"</dt><dd>{opening_bank}</dd><dt>"Opening book balance"</dt><dd>{opening_ledger}</dd><dt>"Closing bank balance"</dt><dd>{closing_bank}</dd><dt>"Closing book balance"</dt><dd>{closing_ledger}</dd><dt>"Unmatched statement amount"</dt><dd>{statement_remaining}</dd><dt>"Unresolved cash entries"</dt><dd>{cash_remaining}</dd></dl>
        <ul>{checks}</ul><p>{review_state}</p>
        {(value["importerCannotReview"]==true).then(||view!{<p class="refusal">"This bank requires a reviewer who did not import its statement. Ask an authorized independent reviewer to confirm."</p>})}
        {(integer(check,"unallocatedReversalCashLegs")>0).then(||view!{<p class="hint">"Posted full journal reversals resolve the indicated ledger corrections. Their complete history is part of this review."</p>})}
    </section>}.into_any()
}

#[component]
pub(crate) fn BankCheckField(
    values: RwSignal<Value>,
    refs: RwSignal<Value>,
    set: Callback<Value>,
) -> impl IntoView {
    let sequence = RwSignal::new(0_u64);
    let requested = RwSignal::new(Value::Null);
    let session_signal = use_context::<session::SessionSignal>();
    let observed_session = RwSignal::new(None::<(String, String, String)>);
    let busy = RwSignal::new(false);
    let notice =
        RwSignal::new("Choose a bank reconciliation basis and financial period.".to_string());
    let load = Callback::new(move |()| {
        sequence.update_if_alive(|n| *n += 1);
        let generation = sequence.get_untracked();
        let draft = values.get_untracked();
        let current_refs = refs.get_untracked();
        let policy = text(&draft, "bankPolicyId");
        let period = text(&draft, "periodId");
        let policy_revision = integer(&draft, "expectedPolicyRevision");
        let period_revision = integer(&draft, "expectedPeriodRevision");
        let company = text(&current_refs, "__companyId");
        set.run(Value::Null);
        refs.update_if_alive(|v| v["__bankReadiness"] = Value::Null);
        let Some(held) = session::held() else {
            notice.set_if_alive("Sign in to review bank controls.".into());
            return;
        };
        if policy.is_empty()
            || period.is_empty()
            || company.is_empty()
            || policy_revision < 1
            || period_revision < 1
        {
            busy.set_if_alive(false);
            notice.set_if_alive(
                "Choose the current bank basis and financial period to load their controls.".into(),
            );
            return;
        }
        busy.set_if_alive(true);
        notice.set_if_alive("Loading complete bank-period controls…".into());
        let path = format!(
            "/erp/accounting/bank-reconciliation?tenant={}&bankPolicyId={}&periodId={}",
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&policy),
            crate::contacts::query_encode(&period)
        );
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if sequence.is_disposed() || sequence.get_untracked() != generation {
                return;
            }
            let Some(current_session) = session::held() else {
                return;
            };
            if current_session.tenant != held.tenant
                || current_session.principal != held.principal
                || current_session.csrf != held.csrf
            {
                return;
            }
            let (Some(now), Some(current_refs)) =
                (values.try_get_untracked(), refs.try_get_untracked())
            else {
                return;
            };
            if text(&current_refs, "__companyId") != company
                || text(&now, "bankPolicyId") != policy
                || text(&now, "periodId") != period
                || integer(&now, "expectedPolicyRevision") != policy_revision
                || integer(&now, "expectedPeriodRevision") != period_revision
            {
                return;
            }
            busy.set_if_alive(false);
            match answer {
                Ok(answer) if answer.status == 200 => {
                    let value = answer.body;
                    if value["policyRevision"] != policy_revision
                        || value["periodRevision"] != period_revision
                    {
                        notice.set_if_alive("The policy or period changed. Refresh its selected record and review again.".into());
                        return;
                    }
                    let hash = value["check"]["evidenceSha256"].clone();
                    let message = if value["importerCannotReview"] == true {
                        "An independent reviewer must confirm this evidence."
                    } else if value["check"]["ready"] == true {
                        "Complete bank controls are loaded. Review them before confirmation."
                    } else {
                        "The bank controls need attention. Review the coverage and balance checks below."
                    };
                    refs.update_if_alive(|v| v["__bankReadiness"] = value);
                    set.run(if hash.is_string() { hash } else { Value::Null });
                    notice.set_if_alive(message.into());
                }
                Ok(answer) => notice.set_if_alive(
                    answer.body["message"]
                        .as_str()
                        .unwrap_or("Bank controls are unavailable in your current scope.")
                        .to_string(),
                ),
                Err(_) => notice.set_if_alive(
                    "Bank controls could not be loaded. Refresh them before reviewing.".into(),
                ),
            }
        });
    });
    Effect::new(move |_| {
        let draft = values.get();
        let loaded = refs.get();
        let current_session = session_signal.and_then(|s| s.get());
        let identity = current_session
            .as_ref()
            .map(|s| json!([s.tenant, s.principal]));
        // Keep the session marker private to this component; never put CSRF in refs or payloads.
        let marker = current_session.map(|s| (s.tenant, s.principal, s.csrf));
        let key = json!([
            draft["bankPolicyId"],
            draft["periodId"],
            draft["expectedPolicyRevision"],
            draft["expectedPeriodRevision"],
            loaded["__companyId"],
            identity
        ]);
        if requested.get_untracked() != key || observed_session.get_untracked() != marker {
            requested.set_if_alive(key);
            observed_session.set_if_alive(marker);
            load.run(());
        }
    });
    view! {<div class="operator-group" data-testid="bank-controls-loader"><p class="hint" role="status">{move||notice.get()}</p><button type="button" class="btn secondary" data-testid="bank-controls-refresh" disabled=move||busy.get() on:click=move|_|load.run(())>"Refresh bank controls"</button>
        {move||{let loaded=refs.get();let value=&loaded["__bankReadiness"];if value.is_object(){let book=json!({"currency":loaded["__currency"],"minorUnits":loaded["__minorUnits"]});summary(value,&book)}else{view!{<span/>}.into_any()}}}
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn period_review_binds_exact_current_evidence_in_addition_to_record_versions() {
        let hash = "a".repeat(64);
        let body = json!({"bankPolicyId":"policy","expectedPolicyRevision":1,"periodId":"period","expectedPeriodRevision":3,"expectedCheckHash":hash});
        let book = json!({"id":"book"});
        let refs = json!({"__bankReadiness":{"bankPolicyId":"policy","policyRevision":1,"periodId":"period","periodRevision":3,"periodState":"open","canConfirm":true,"check":{"bookId":"book","ready":true,"evidenceSha256":hash}}});
        assert!(valid_review(&body, &book, &refs));
        for (field, value) in [
            ("expectedCheckHash", json!("b".repeat(64))),
            ("expectedPeriodRevision", json!(1)),
            ("bankPolicyId", json!("other")),
        ] {
            let mut stale = body.clone();
            stale[field] = value;
            assert!(!valid_review(&stale, &book, &refs));
        }
        let mut blocked = refs.clone();
        blocked["__bankReadiness"]["canConfirm"] = json!(false);
        assert!(!valid_review(&body, &book, &blocked));
        let mut other = book;
        other["id"] = json!("another-book");
        assert!(!valid_review(&body, &other, &refs));
    }
}
