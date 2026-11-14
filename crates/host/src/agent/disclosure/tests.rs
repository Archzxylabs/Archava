use super::*;
fn permit() -> Permit {
    Permit {policy:serde_json::from_value(json!({"providerBinding":"a".repeat(64),"modules":["commerce"],"dataClasses":["conversation_text","record_references","business_facts"],"companies":[],"includeWorkspaceRecords":true})).unwrap(),identity:"current-policy".into(),company_sources:Default::default()}
}
#[test]
fn provider_projection_omits_unselected_modules_history_queries_and_unknown_collections() {
    let p = permit();
    let full = json!({"companyId":null,"userReferenceIds":["user-reference"],"requestedReferenceIds":["private-old-lookup"],"observedAt":"now","history":["private-history"],"lookupResults":[{"module":"commerce","collection":"products","query":"private-query","path":"private-path","exactId":false}],"sources":[{"module":"commerce","available":true,"data":{"products":[{"id":"product","revision":1,"title":"private-title","quantity":7,"price":12345}],"new_secrets":[{"id":"private-secret"}],"permissions":["private-grant"]},"coverage":{"products":{"wholeCollection":true,"included":1}}},{"module":"billing","available":true,"data":{"invoices":[{"id":"private-invoice"}]}}]});
    let v = p.project(&full);
    let text = v.to_string();
    for private in [
        "private-history",
        "private-query",
        "private-path",
        "private-title",
        "private-secret",
        "private-grant",
        "private-invoice",
        "private-old-lookup",
        "12345",
    ] {
        assert!(!text.contains(private), "{private}: {text}");
    }
    assert_eq!(
        v["sources"][0]["data"]["products"][0],
        json!({"id":"product","revision":1,"quantity":7})
    );
    assert_eq!(v["requestedReferenceIds"], json!(["user-reference"]));
    assert_eq!(v["sources"][1]["available"], false);
    assert!(full["sources"][0]["data"]["products"][0]["title"].is_string());
}
#[test]
fn raw_supplier_and_gateway_bank_evidence_is_withheld_even_with_all_data_classes() {
    let mut p = permit();
    p.policy.modules.insert("accounting".into());
    for class in [
        DataClass::Names,
        DataClass::ContactDetails,
        DataClass::Notes,
        DataClass::FinancialAmounts,
    ] {
        p.policy.data_classes.insert(class);
    }
    let full = json!({"sources":[{"module":"accounting","available":true,"data":{
        "payables":[{"id":"PRIVATE-PAYABLE","outstandingMinor":"987654321"}],
        "payable_settlements":[{"id":"PRIVATE-PAYMENT","bankRowId":"PRIVATE-PAYMENT-BANK"}],
        "clearing_sources":[{"id":"PRIVATE-GATEWAY","amountMinor":"123456789"}],
        "clearing_settlements":[{"id":"PRIVATE-PAYOUT","bankRowId":"PRIVATE-PAYOUT-BANK"}],
        "invoice_term_sources":[{"id":"PRIVATE-INVOICE-TERMS"}],
        "supplier_term_sources":[{"id":"PRIVATE-SUPPLIER-TERMS"}],
        "document_terms":[{"id":"PRIVATE-TERMS-HISTORY","reason":"PRIVATE-REVIEW"}]
    }}]});
    let projected = p.project(&full).to_string();
    assert!(!projected.contains("PRIVATE"));
    assert!(!projected.contains("987654321"));
    assert!(!projected.contains("123456789"));
    for collection in [
        "payables",
        "payable_settlements",
        "clearing_sources",
        "clearing_settlements",
        "invoice_term_sources",
        "supplier_term_sources",
        "document_terms",
    ] {
        assert!(!projected.contains(collection), "{projected}");
    }
}
#[test]
fn the_global_byte_cap_keeps_exact_requested_records_and_honest_partial_coverage() {
    let rows = (0..25)
        .map(|i| json!({"id":format!("row-{i}"),"name":"界".repeat(2500)}))
        .collect::<Vec<_>>();
    let mut input = json!({"authorizedContext":{"requestedReferenceIds":["row-24"],"sources":[{"module":"commerce","data":{"products":rows},"coverage":{"products":{"included":25,"wholeCollection":true}}}]},"actionContracts":["static-protocol"]});
    bound_payload(&mut input).unwrap();
    assert!(input.to_string().len() <= INPUT_BYTE_LIMIT);
    assert_eq!(input["inputBudget"]["limited"], true);
    let s = &input["authorizedContext"]["sources"][0];
    assert!(s["data"]["products"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == "row-24"));
    assert_eq!(s["coverage"]["products"]["wholeCollection"], false);
    assert_eq!(
        s["coverage"]["products"]["included"],
        s["data"]["products"].as_array().unwrap().len()
    );
    let mut protected = json!({"authorizedContext":{"requestedReferenceIds":["exact"],"sources":[{"data":{"products":[{"id":"exact","name":"x".repeat(INPUT_BYTE_LIMIT)}]}}]}});
    assert!(bound_payload(&mut protected).is_err());
    assert_eq!(
        protected["authorizedContext"]["sources"][0]["data"]["products"][0]["name"]
            .as_str()
            .unwrap()
            .len(),
        INPUT_BYTE_LIMIT
    );
}
#[test]
fn current_message_permission_is_separate_from_stored_notes_and_company_consent() {
    let mut p = permit();
    assert!(p.can_plan(None));
    p.policy.data_classes.remove(&DataClass::ConversationText);
    p.policy.data_classes.insert(DataClass::Notes);
    assert!(!p.can_plan(None));
    p.policy.data_classes.insert(DataClass::ConversationText);
    assert!(!p.can_plan(Some(Uuid::new_v4())));
    let history = json!({"objective":"never-share-objective","state":"completed","context":{"companyId":null},"plan":{"steps":[{"tool":"commerce.product.set","input":{"id":"p","notes":"explicit-note","price":100},"dependsOn":[]}]},"outcome":{"secret":"never-share-outcome"}});
    let projected = p.prior(Some(&history)).to_string();
    assert!(!projected.contains("never-share"));
    assert!(!projected.contains("100"));
    assert!(
        !projected.contains("explicit-note"),
        "Selected notes do not authorize historical derived inputs after record scope changes"
    );
    assert!(!projected.contains("\"input\""));
}
#[test]
fn prior_completion_requires_matching_approval_mission_tool_and_verified_receipt() {
    let mut p = permit();
    let mut goal = json!({"state":"holding","mission_id":"owned-mission","approved_hash":"owned-hash","plan_hash":"owned-hash","context":{"companyId":null},"plan":{"steps":[{"tool":"commerce.product.create","input":{"title":"PRIVATE-OLD-INPUT"}},{"tool":"commerce.product.set","input":{"price":99999}}]},"outcome":{"id":"owned-mission","summary":"PRIVATE-OUTCOME","steps":[{"step_index":0,"tool_target":"commerce.product.create","status":"passed","payload_output":{"verified":true,"receipt":{"id":"PRIVATE-OLD-ID"}}},{"step_index":1,"tool_target":"commerce.product.set","status":"passed","payload_output":{"verified":false}}]}});
    let value = p.prior(Some(&goal));
    assert_eq!(value["verifiedCompletedSteps"], 1);
    assert_eq!(value["steps"][0]["verifiedCompleted"], true);
    assert_eq!(value["steps"][1]["verifiedCompleted"], false);
    assert!(!value.to_string().contains("PRIVATE"));
    goal["outcome"]["id"] = json!("wrong-mission");
    assert_eq!(p.prior(Some(&goal))["verifiedCompletedSteps"], 0);
    goal["outcome"]["id"] = json!("owned-mission");
    goal["approved_hash"] = Value::Null;
    goal["state"] = json!("completed");
    assert_eq!(p.prior(Some(&goal))["verifiedCompletedSteps"], 0);
    p.policy.data_classes.remove(&DataClass::BusinessFacts);
    assert_eq!(p.prior(Some(&goal)), Value::Null);
}
#[test]
fn source_organization_party_and_affiliation_aliases_inherit_the_company_selection() {
    let mut p = permit();
    p.policy.modules.insert("organizations".into());
    p.policy.data_classes.insert(DataClass::Names);
    p.company_sources.insert(
        "source-org".into(),
        "00000000-0000-4000-8000-000000000002".into(),
    );
    let context = json!({"sources":[{"module":"organizations","available":true,"data":{"organizations":[{"id":"source-org","name":"UNAPPROVED COMPANY"}],"parties":[{"id":"party","sourceOrganizationId":"source-org","name":"UNAPPROVED PARTY"}],"affiliations":[{"id":"affiliation","organizationId":"source-org","name":"UNAPPROVED AFFILIATION"}]}}]});
    let v = p.project(&context);
    assert!(!v.to_string().contains("UNAPPROVED"));
    for key in ["organizations", "parties", "affiliations"] {
        assert!(v["sources"][0]["data"][key].as_array().unwrap().is_empty());
        assert_eq!(v["sources"][0]["coverage"][key]["withheldByPolicy"], true);
    }
}
fn sharing(modules: &[&str], classes: &[&str], companies: &[&str], workspace: bool) -> Permit {
    Permit {policy:serde_json::from_value(json!({"providerBinding":"a".repeat(64),"modules":modules,"dataClasses":classes,"companies":companies,"includeWorkspaceRecords":workspace})).unwrap(),identity:"aggregate-policy".into(),company_sources:Default::default()}
}
#[test]
fn an_aggregate_needs_its_module_and_every_data_category_of_its_shape() {
    let facts = sharing(
        &["billing"],
        &["conversation_text", "business_facts"],
        &[],
        true,
    );
    assert_eq!(facts.aggregate_refusal("billing", false, None), None);
    assert_eq!(
        facts.aggregate_refusal("billing", false, Some("business_facts")),
        None
    );
    assert_eq!(
        facts.aggregate_refusal("commerce", false, None),
        Some(("aggregate_module_not_shared", None))
    );
    assert_eq!(
        facts.aggregate_refusal("billing", true, None),
        Some(("aggregate_data_class_not_shared", Some("financial_amounts")))
    );
    assert_eq!(
        facts.aggregate_refusal("billing", false, Some("names")),
        Some(("aggregate_data_class_not_shared", Some("names")))
    );
    // A category the registry could misspell is never treated as shared.
    assert_eq!(
        facts.aggregate_refusal("billing", false, Some("everything")),
        Some(("aggregate_data_class_not_shared", None))
    );
    // Amounts and names without statuses and dates: even a count is refused.
    let amounts = sharing(
        &["billing"],
        &["conversation_text", "financial_amounts", "names"],
        &[],
        true,
    );
    assert_eq!(
        amounts.aggregate_refusal("billing", true, Some("names")),
        Some(("aggregate_data_class_not_shared", Some("business_facts")))
    );
    let all = sharing(
        &["billing"],
        &[
            "conversation_text",
            "business_facts",
            "financial_amounts",
            "names",
        ],
        &[],
        true,
    );
    assert_eq!(all.aggregate_refusal("billing", true, Some("names")), None);
}
#[test]
fn an_aggregate_counts_only_approved_companies_and_never_all_by_default() {
    let a = "00000000-0000-4000-8000-000000000001";
    let b = "00000000-0000-4000-8000-000000000002";
    let id = |s: &str| Uuid::parse_str(s).unwrap();
    let none = sharing(&["billing"], &["business_facts"], &[], false);
    let scope = none.aggregate_scope(None);
    assert_eq!(scope.companies, Some(vec![]), "empty never means all");
    assert!(!scope.unassigned);
    let one = sharing(&["billing"], &["business_facts"], &[a], true);
    let scope = one.aggregate_scope(None);
    assert_eq!(scope.companies, Some(vec![id(a)]));
    assert!(scope.unassigned);
    // A company-scoped turn counts that company only, and only if approved.
    let scope = one.aggregate_scope(Some(id(a)));
    assert_eq!(scope.companies, Some(vec![id(a)]));
    assert!(!scope.unassigned);
    let scope = one.aggregate_scope(Some(id(b)));
    assert_eq!(scope.companies, Some(vec![]));
    assert!(!scope.unassigned);
}
#[test]
fn aggregate_results_and_offers_follow_the_current_selection() {
    let result = |group: Value, measure: &str| json!({"kind":"aggregate","module":"billing","collection":"invoices","measure":{"kind":measure,"field":if measure=="sum"{json!("outstanding")}else{Value::Null}},"groupBy":group,"filter":{"dateField":"issued","companyId":"00000000-0000-4000-8000-000000000001"},"rowsCovered":3,"sections":[if measure=="sum"{json!({"rows":3,"groups":[{"label":"Private Customer Name","rows":3,"amount":705100}]})}else{json!({"rows":3,"groups":[{"label":"issued","rows":3}]})}]});
    let by_customer = result(json!("customer"), "sum");
    let by_status = result(json!("status"), "count");
    let refused = json!({"kind":"aggregate","refused":true,"reason":"aggregate_dimension_not_listed","message":"m","module":"billing","collection":"invoices"});
    let context = json!({"sources":[],"aggregateResults":[by_customer,by_status,refused]});
    let facts = sharing(
        &["billing"],
        &["conversation_text", "business_facts"],
        &[],
        true,
    );
    let out = facts.project(&context);
    let text = out.to_string();
    assert!(!text.contains("Private Customer Name"), "{text}");
    assert!(!text.contains("705100"), "{text}");
    let shown = out["aggregateResults"].as_array().unwrap();
    assert_eq!(shown.len(), 2, "the count by status and the refusal");
    assert_eq!(shown[0]["groupBy"], "status");
    // Without record references the company identifier does not travel.
    assert!(shown[0]["filter"].get("companyId").is_none());
    assert_eq!(shown[0]["filter"]["companyFilterApplied"], true);
    assert!(!text.contains("00000000-0000-4000-8000-000000000001"));
    assert_eq!(shown[1], refused);
    let all = sharing(
        &["billing"],
        &[
            "conversation_text",
            "record_references",
            "business_facts",
            "financial_amounts",
            "names",
        ],
        &[],
        true,
    );
    let out = all.project(&context);
    assert_eq!(out["aggregateResults"].as_array().unwrap().len(), 3);
    assert_eq!(out["aggregateResults"][0], by_customer);
    let unshared = sharing(
        &["commerce"],
        &["conversation_text", "business_facts"],
        &[],
        true,
    );
    assert_eq!(
        unshared.project(&context)["aggregateResults"],
        json!([refused])
    );
    // Offers: only what the selection lets the provider receive.
    let sources = json!({"sources":[{"module":"billing","available":true},{"module":"commerce","available":true},{"module":"purchasing","available":false},{"module":"crm","available":true}]});
    let offers = super::super::aggregate::offers(&facts, &sources);
    assert_eq!(
        offers,
        json!([{"module":"billing","collection":"invoices","measures":["count"],"sumFields":[],"groupBy":["status","month"],"filters":["status","dateFrom","dateTo","companyId"],"dateField":"issued","filterValues":{"status":["draft","issued","settled","void"]}}])
            .as_array()
            .unwrap()
            .clone()
    );
    let mut wide = all.clone();
    wide.policy.modules.insert("commerce".into());
    wide.policy.modules.insert("purchasing".into());
    let offers = super::super::aggregate::offers(&wide, &sources);
    assert_eq!(offers.len(), 2, "an unavailable module offers nothing");
    assert_eq!(offers[0]["module"], "billing");
    assert_eq!(offers[0]["measures"], json!(["count", "sum"]));
    assert_eq!(
        offers[0]["sumFields"],
        json!(["gross", "outstanding", "tax"])
    );
    assert_eq!(offers[0]["groupBy"], json!(["status", "customer", "month"]));
    assert_eq!(offers[1]["module"], "commerce");
    assert_eq!(offers[1]["sumFields"], json!(["total"]));
    let silent = sharing(
        &["billing"],
        &["conversation_text", "names", "financial_amounts"],
        &[],
        true,
    );
    assert!(super::super::aggregate::offers(&silent, &sources).is_empty());
}
#[test]
fn the_selected_companys_trade_unit_is_shared_as_a_business_fact_only() {
    let context = json!({"sources":[],"companyId":"00000000-0000-4000-8000-000000000001","tradeCurrency":{"currency":"USD","unit":"minor","unitLabel":"minor units of USD"}});
    let facts = sharing(
        &["commerce"],
        &["conversation_text", "business_facts"],
        &[],
        true,
    );
    assert_eq!(facts.project(&context)["tradeCurrency"]["currency"], "USD");
    let names = sharing(&["commerce"], &["conversation_text", "names"], &[], true);
    assert!(names.project(&context).get("tradeCurrency").is_none());
}
