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
fn manual_cash_matches_use_exact_signed_remainders_in_a_cold_consumer() {
    use archava_domain::erp::accounting::bank::{allocate, signed_minor};
    let partial = allocate("-15000", "-20000", "5000").unwrap();
    assert_eq!(partial.statement_remaining_minor, "-10000");
    assert_eq!(partial.cash_leg_remaining_minor, "-15000");
    assert!(allocate("-15000", "20000", "5000").is_err());
    assert!(allocate("-15000", "-20000", "15001").is_err());
    for ambiguous in ["-0", "+5000", "05000", "5e3", "5000 "] {
        assert!(signed_minor(ambiguous).is_err());
    }
}

const TENANT: &str = "agency-k05";
fn label(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::New => "Discovery",
        PipelineStage::Qualified => "Scope confirmed",
        PipelineStage::Proposal => "Proposal review",
        PipelineStage::Won => "Project accepted",
        PipelineStage::Lost => "Declined",
    }
}
fn extension() {
    use archava_domain::erp::projects::{
        parse_command, project_transition_allowed, task_transition_allowed,
    };
    let project = parse_command(&json!({"operation":"project.create","title":"Website delivery","scopeSummary":"Confirmed deliverables; no unlimited maintenance promise"})).unwrap();
    assert_eq!(
        serde_json::to_value(project).unwrap()["operation"],
        "project.create"
    );
    assert!(project_transition_allowed("draft", "active"));
    assert!(!project_transition_allowed("draft", "delivered"));
    assert!(task_transition_allowed("doing", "done"));
    assert!(!task_transition_allowed("todo", "invoiced"));
    assert!(parse_command(&json!({"operation":"project.transition","projectId":"00000000-0000-4000-8000-000000000001","projectRevision":1,"state":"cancelled"})).is_err());
}
#[test]
fn agency_extends_won_crm_with_project_rules() {
    assert_eq!(label(customer_journey()), "Project accepted");
    extension();
}
fn main() {
    assert_eq!(label(customer_journey()), "Project accepted");
    extension();
    println!("K05 agency: shared contacts/pipeline plus project rules passed");
}
