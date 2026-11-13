use archava_core::tenant::{assert_tenant, scoped, TenantId};
use archava_domain::erp::crm::{
    text_len, transition_allowed, valid_email, ContactFields, ContactPatch, PipelineStage,
};
use serde_json::json;

fn contact() -> ContactFields {
    serde_json::from_value(json!({"fullName": "Budi & Sons 日本", "email": "buyer@example.test", "organization": "Owned synthetic business"})).unwrap()
}
fn advance(from: PipelineStage, to: PipelineStage) -> Result<PipelineStage, &'static str> {
    if transition_allowed(from, to) {
        Ok(to)
    } else {
        Err("Shared CRM rule refused transition")
    }
}
fn customer_journey() -> PipelineStage {
    let c = contact();
    assert!(text_len(&c.full_name, 1, 160));
    assert!(valid_email(c.email.as_deref().unwrap()));
    [
        PipelineStage::Qualified,
        PipelineStage::Proposal,
        PipelineStage::Won,
    ]
    .into_iter()
    .try_fold(PipelineStage::New, advance)
    .unwrap()
}
#[test]
fn shared_contacts_keep_unicode_and_refuse_forged_fields() {
    assert_eq!(contact().full_name, "Budi & Sons 日本");
    assert!(serde_json::from_value::<ContactFields>(
        json!({"fullName":"Buyer","actorId":"administrator"})
    )
    .is_err());
    assert!(!valid_email("not an email"));
    assert!(!text_len("", 1, 160));
}
#[test]
fn contact_patch_keeps_clear_and_replace_distinct() {
    let keep: ContactPatch = serde_json::from_value(json!({})).unwrap();
    let clear: ContactPatch = serde_json::from_value(json!({"email":null})).unwrap();
    let replace: ContactPatch =
        serde_json::from_value(json!({"email":"new@example.test"})).unwrap();
    assert!(keep.is_empty());
    assert_eq!(clear.email, Some(None));
    assert_eq!(replace.email, Some(Some("new@example.test".into())));
    assert_eq!(serde_json::to_value(clear).unwrap(), json!({"email":null}));
}
#[test]
fn local_labels_do_not_override_shared_pipeline_rules() {
    assert_eq!(customer_journey(), PipelineStage::Won);
    assert!(advance(PipelineStage::New, PipelineStage::Won).is_err());
    assert!(advance(PipelineStage::Won, PipelineStage::Qualified).is_err());
    assert!(PipelineStage::parse(label(PipelineStage::Won)).is_none());
}
#[test]
fn core_tenant_boundary_rejects_other_business_and_missing_rows() {
    let tenant = TenantId::new(TENANT).unwrap();
    let other = TenantId::new("unrelated-business").unwrap();
    assert!(assert_tenant(Some(scoped(&tenant)), &tenant).is_ok());
    assert!(assert_tenant(Some(scoped(&other)), &tenant).is_err());
    assert!(assert_tenant::<archava_core::tenant::TenantScope>(None, &tenant).is_err());
}

#[test]
fn complete_bank_controls_survive_packaging_without_rounding_or_omitting_rows() {
    use archava_domain::erp::accounting::bank::{statement_totals, Statement};
    let mut statement: Statement = serde_json::from_value(json!({
        "reference":"COLD-NOV", "fromDate":"2026-11-01", "toDate":"2026-11-30",
        "currency":"USD", "openingMinor":"0", "closingMinor":"1", "rows":[
            {"externalRef":"deposit", "date":"2026-11-01", "amountMinor":"9000000000000000"},
            {"externalRef":"withdrawal", "date":"2026-11-30", "amountMinor":"-8999999999999999"}
        ]
    })).unwrap();
    let controls = statement_totals(&statement, "USD").unwrap();
    assert_eq!(controls.row_count, 2);
    assert_eq!(controls.inflow_minor, "9000000000000000");
    assert_eq!(controls.net_minor, "1");
    assert!(statement_totals(&statement, "IDR").is_err());
    statement.rows[1].external_ref = "deposit".into();
    assert!(statement_totals(&statement, "USD").is_err());
}

const TENANT: &str = "wholesale-k04";
fn label(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::New => "Inbound enquiry",
        PipelineStage::Qualified => "Trade qualified",
        PipelineStage::Proposal => "Quote sent",
        PipelineStage::Won => "Order accepted",
        PipelineStage::Lost => "Declined",
    }
}
fn extension() {
    use archava_domain::erp::commerce::CommerceCommand;
    let order = json!({"operation":"order.create","customerName":contact().full_name,"lines":[{"productId":"00000000-0000-4000-8000-000000000001","quantity":12}]});
    let parsed: CommerceCommand = serde_json::from_value(order.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(parsed.operation(), "order.create");
    assert!(parsed.permissions().contains(&"orders.write"));
    for quantity in [0, -1] {
        let mut invalid = order.clone();
        invalid["lines"][0]["quantity"] = json!(quantity);
        assert!(serde_json::from_value::<CommerceCommand>(invalid)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut forged = order;
    forged["approvedBy"] = json!("owner");
    assert!(serde_json::from_value::<CommerceCommand>(forged).is_err());
}
#[test]
fn wholesale_extends_won_crm_with_order_rules() {
    assert_eq!(label(customer_journey()), "Order accepted");
    extension();
}
fn main() {
    assert_eq!(label(customer_journey()), "Order accepted");
    extension();
    println!("K04 wholesale: shared contacts/pipeline plus commerce rules passed");
}
