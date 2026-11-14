//! A refused step keeps the reason the module answered, in catalogue words only.
use super::*;

async fn billing(app: &Router, s: &Value, mutation: Value) -> Value {
    post(
        app,
        "/erp/billing/commands",
        json!({"tenant":"acme-retail","mutation":mutation}),
        s,
    )
    .await
}
/// Nothing a database or a driver would say may reach a person through Ava.
fn assert_no_database_text(value: &Value) {
    let text = value.to_string();
    for needle in [
        "SQLSTATE",
        "23514",
        "23505",
        "ERROR",
        "violates",
        "constraint",
        "archava_billing",
        "PgDatabaseError",
        "RAISE",
    ] {
        assert!(!text.contains(needle), "{needle} leaked: {text}");
    }
}
async fn issued_invoice(app: &Router, s: &Value, code: &str) -> String {
    let created = billing(app,s,json!({"type":"create_invoice","commandId":Uuid::new_v4(),"invoiceCode":code,"customerRef":"agent-customer","currency":"IDR","consumerRef":"agent-owner","lines":[{"description":"Agent service","quantity":1,"unitMinor":150000}]})).await;
    let id = created["receipt"]["invoiceId"]
        .as_str()
        .or_else(|| created["invoiceId"].as_str())
        .unwrap_or_else(|| panic!("{created}"))
        .to_string();
    billing(app,s,json!({"type":"issue_invoice","commandId":Uuid::new_v4(),"invoiceId":id,"expectedRevision":1,"reason":"Approved for settlement"})).await;
    id
}
fn approval(plan: &Value) -> Value {
    json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true})
}
#[tokio::test]
async fn a_catalogued_refusal_keeps_its_code_and_sentences_and_the_report_names_the_step() {
    let (f, _state, app, alice, _) = setup().await;
    let invoice = issued_invoice(&app, &alice, "AGENT-REFUSAL-1").await;
    let today: String = sqlx::query_scalar("SELECT (now() AT TIME ZONE 'UTC')::date::text")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    let mut input = turn("Record the transfer the customer says they sent.");
    input["action"] = json!({"tool":"billing.record_manual_receipt","values":{"invoiceId":invoice,"expectedRevision":2,"amountMinor":150001,"method":"bank_transfer","receivedOn":today,"reference":"TRF-AGENT-1","reason":"Seen on the statement"}});
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    let thread = plan["threadId"].clone();
    let done = post(&app, "/api/agent/approve", approval(&plan), &alice).await;
    let goal = &done["goal"];
    assert_eq!(goal["state"], "needs_review", "{done}");
    let entry =
        archava_domain::erp::refusals::for_code("manual_receipt_exceeds_outstanding").unwrap();
    assert_eq!(
        goal["failureDetail"],
        json!({"stepIndex":0,"tool":"billing.record_manual_receipt","action":goal["failureDetail"]["action"],"code":"manual_receipt_exceeds_outstanding","message":entry.message,"messageId":entry.message_id}),
        "{done}"
    );
    assert!(goal["failureDetail"]["action"]
        .as_str()
        .is_some_and(|label| !label.is_empty()));
    let failure = goal["failure"].as_str().unwrap();
    assert!(failure.starts_with("Step 1 ("), "{failure}");
    assert!(failure.contains(&entry.message), "{failure}");
    assert!(!failure.contains("The current domain rules refused this step"));
    assert!(
        goal["outcome"].get("agentRefusal").is_none(),
        "the reason is published once, as failureDetail"
    );
    // The stored row holds the same catalogue text and nothing else.
    let stored: (String, Value) = sqlx::query_as(
        "SELECT failure,outcome->'agentRefusal' FROM archava_owner.agent_goals WHERE id=$1::uuid",
    )
    .bind(plan["goalId"].as_str().unwrap())
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(stored.0, failure);
    assert_eq!(stored.1["code"], "manual_receipt_exceeds_outstanding");
    assert_eq!(stored.1["messageId"], entry.message_id);
    assert_no_database_text(&json!(stored.0));
    assert_no_database_text(&stored.1);
    // The receipt-backed report states which step was refused and why.
    let mut ask = turn("Laporkan hasil misi ini.");
    ask["threadId"] = thread;
    ask["goalId"] = plan["goalId"].clone();
    let report = post(&app, "/api/agent/turns", ask, &alice).await;
    assert_eq!(report["data"]["kind"], "agent_report", "{report}");
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("Goal status: needs_review"), "{text}");
    assert!(text.contains("0 business steps have verified completion records"));
    assert!(text.contains("Step 1 ("), "{text}");
    assert!(text.contains(&entry.message), "{text}");
    assert_eq!(
        report["data"]["refusal"]["code"],
        "manual_receipt_exceeds_outstanding"
    );
    assert_eq!(report["data"]["refusal"]["messageId"], entry.message_id);
    assert_no_database_text(&json!(text));
    assert_no_database_text(&report["data"]);
    // Nothing was settled and nothing retried.
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_billing.payment_events")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(events, 0);
}
#[tokio::test]
async fn an_uncatalogued_refusal_keeps_the_fixed_sentence_without_a_reason() {
    let (f, _state, app, alice, _) = setup().await;
    let invoice = issued_invoice(&app, &alice, "AGENT-REFUSAL-2").await;
    // A stale revision is a plain conflict: decided in the adapter, no entry.
    let mut input = turn("Void the invoice I reviewed earlier.");
    input["action"] = json!({"tool":"billing.void_invoice","values":{"invoiceId":invoice,"expectedRevision":7,"reason":"Reviewed withdrawal"}});
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    let done = post(&app, "/api/agent/approve", approval(&plan), &alice).await;
    let goal = &done["goal"];
    assert_eq!(goal["state"], "needs_review", "{done}");
    assert_eq!(
        goal["failure"],
        "The current domain rules refused this step. Review the recorded partial outcome before changing the plan."
    );
    assert!(goal.get("failureDetail").is_none(), "{done}");
    let stored: (String, Option<Value>) = sqlx::query_as(
        "SELECT failure,outcome->'agentRefusal' FROM archava_owner.agent_goals WHERE id=$1::uuid",
    )
    .bind(plan["goalId"].as_str().unwrap())
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert!(stored.1.is_none());
    assert_no_database_text(&json!(stored.0));
    let mut ask = turn("Laporkan hasil misi ini.");
    ask["threadId"] = plan["threadId"].clone();
    ask["goalId"] = plan["goalId"].clone();
    let report = post(&app, "/api/agent/turns", ask, &alice).await;
    assert!(report["text"]
        .as_str()
        .unwrap()
        .contains("The current domain rules refused this step"));
    assert!(report["data"]["refusal"].is_null(), "{report}");
    assert_no_database_text(&report["data"]);
    let state: String =
        sqlx::query_scalar("SELECT status FROM archava_billing.invoices WHERE id=$1::uuid")
            .bind(&invoice)
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(state, "issued");
}
