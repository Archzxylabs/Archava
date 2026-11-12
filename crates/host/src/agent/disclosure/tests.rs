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
