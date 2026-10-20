//! Behavioural witnesses for the owner-configuration engine core.
//!
//! These map to the OWN acceptance semantic that must hold regardless of which
//! surface (dashboard or assistant) produced the request. They deliberately do
//! NOT claim the product lifecycle is delivered: inspect UI, persistence,
//! grants and the browser journey are host/product concerns and are proven
//! elsewhere.

use archava_own::{
    ApplyError, BlockedReason, Catalogue, Change, Confirmation, CostClass, ModuleSpec, OwnerConfig,
    Plan, PlanError, Readiness, Request, RestoreError, Store, Terminology,
};
use std::collections::{BTreeMap, BTreeSet};

fn module(
    id: &str,
    requires: &[&str],
    entitled: bool,
    installed: bool,
    readiness: Readiness,
    cost: CostClass,
) -> ModuleSpec {
    ModuleSpec {
        id: id.to_string(),
        requires: requires.iter().map(|id| id.to_string()).collect(),
        entitled,
        installed,
        mandatory: false,
        readiness,
        cost,
    }
}

fn catalogue() -> Catalogue {
    let mut catalogue = Catalogue {
        modules: vec![
            module(
                "core",
                &[],
                true,
                true,
                Readiness::Ready,
                CostClass::Included,
            ),
            module(
                "restaurant",
                &["core"],
                true,
                true,
                Readiness::Ready,
                CostClass::Recurring,
            ),
            module(
                "laundry",
                &["restaurant"],
                true,
                true,
                Readiness::NeedsSetup,
                CostClass::Usage,
            ),
            module(
                "events",
                &["core"],
                false,
                true,
                Readiness::Ready,
                CostClass::Review,
            ),
            module(
                "reports",
                &["core"],
                true,
                false,
                Readiness::Ready,
                CostClass::Included,
            ),
            module(
                "ai-desk",
                &["core"],
                true,
                true,
                Readiness::Unavailable,
                CostClass::Review,
            ),
        ],
    };
    catalogue.modules[0].mandatory = true;
    catalogue
}

fn base() -> OwnerConfig {
    let mut labels = BTreeMap::new();
    labels.insert("new".to_string(), "New".to_string());
    labels.insert("won".to_string(), "Won".to_string());
    OwnerConfig {
        revision: 1,
        enabled: BTreeSet::from(["core".to_string()]),
        terminology: Terminology {
            pipeline_labels: labels,
        },
    }
}

fn store() -> Store {
    Store::new(catalogue(), base())
}

fn request(base_revision: i64, enable: &[&str], disable: &[&str]) -> Request {
    Request {
        base_revision,
        enable: enable.iter().map(|id| id.to_string()).collect(),
        disable: disable.iter().map(|id| id.to_string()).collect(),
        terminology: None,
    }
}

fn confirm(plan: &Plan) -> Confirmation {
    Confirmation {
        plan_hash: plan.plan_hash.clone(),
        base_revision: plan.base_revision,
    }
}

#[test]
fn t02_the_same_typed_request_has_the_same_plan_identity() {
    let store = store();
    let left = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    let right = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    assert_eq!(left.plan_hash, right.plan_hash);
    assert_eq!(left, right);
    // A different proposal is a different identity, even at the same base.
    let other = store
        .plan(&request(1, &["laundry", "restaurant"], &[]))
        .unwrap();
    assert_ne!(left.plan_hash, other.plan_hash);
}

#[test]
fn t03_the_plan_carries_readiness_and_cost_for_every_change() {
    let store = store();
    let plan = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    assert_eq!(
        plan.effects,
        vec![archava_own::Effect {
            module: "restaurant".to_string(),
            readiness: Readiness::Ready,
            cost: CostClass::Recurring,
        }]
    );
    assert_eq!(
        plan.changes,
        vec![Change::Enable {
            module: "restaurant".to_string()
        }]
    );
}

#[test]
fn t05_a_missing_dependency_is_an_explicit_block_and_never_a_partial_apply() {
    let mut store = store();
    let plan = store.plan(&request(1, &["laundry"], &[])).unwrap();
    let blocked = plan.blocked.as_ref().expect("laundry needs restaurant");
    assert_eq!(blocked.reason, BlockedReason::MissingDependency);
    assert_eq!(blocked.module, "laundry");
    assert!(blocked.detail.contains("restaurant"));
    // A blocked plan proposes no new configuration at all.
    assert_eq!(plan.config, *store.current());

    let refused = store
        .apply(&plan, Some(&confirm(&plan)), "own-request-blocked")
        .unwrap_err();
    match refused {
        ApplyError::Blocked(blocked) => {
            assert_eq!(blocked.reason, BlockedReason::MissingDependency)
        }
        other => panic!("blocked plan applied: {other:?}"),
    }
    assert_eq!(store.current().revision, 1);
    assert_eq!(store.history().len(), 1);
}

#[test]
fn t05_entitlement_install_and_readiness_are_distinct_refusals() {
    let store = store();
    for (module, reason) in [
        ("events", BlockedReason::NotEntitled),
        ("reports", BlockedReason::NotInstalled),
        ("ai-desk", BlockedReason::ModuleUnavailable),
        ("does-not-exist", BlockedReason::UnknownModule),
    ] {
        let plan = store.plan(&request(1, &[module], &[])).unwrap();
        assert_eq!(
            plan.blocked.as_ref().map(|blocked| blocked.reason),
            Some(reason),
            "{module}"
        );
    }
}

#[test]
fn t06_a_stale_base_is_refused_and_demands_a_replan() {
    let store = store();
    let error = store.plan(&request(0, &["restaurant"], &[])).unwrap_err();
    assert_eq!(error, PlanError::StaleBase { current: 1 });
}

#[test]
fn t06_a_confirmation_for_another_plan_is_refused() {
    let mut store = store();
    let approved = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    let other = store
        .plan(&request(1, &["restaurant", "laundry"], &[]))
        .unwrap();
    let refusal = store
        .apply(&approved, Some(&confirm(&other)), "own-request-mismatch")
        .unwrap_err();
    assert_eq!(refusal, ApplyError::PlanMismatch);
    // No confirmation at all is also refused.
    let refusal = store
        .apply(&approved, None, "own-request-unconfirmed")
        .unwrap_err();
    assert_eq!(refusal, ApplyError::MissingConfirmation);
    assert_eq!(store.current().revision, 1);
}

#[test]
fn t06_the_same_request_id_yields_one_effect_and_the_same_receipt() {
    let mut store = store();
    let plan = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    let first = store
        .apply(&plan, Some(&confirm(&plan)), "own-request-1")
        .unwrap();
    let second = store
        .apply(&plan, Some(&confirm(&plan)), "own-request-1")
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.revision, 2);
    assert_eq!(store.current().revision, 2);
    assert_eq!(store.history().len(), 2);
    assert!(store.current().enabled.contains("restaurant"));

    // Replaying the SAME plan under a NEW id is a stale apply, not a second
    // effect: the configuration moved on.
    let refusal = store
        .apply(&plan, Some(&confirm(&plan)), "own-request-2")
        .unwrap_err();
    assert_eq!(refusal, ApplyError::StaleBase { current: 2 });
    assert_eq!(store.current().revision, 2);
}

#[test]
fn t07_disabling_a_dependency_of_an_enabled_module_is_blocked() {
    let mut store = store();
    let enable = store
        .plan(&request(1, &["restaurant", "laundry"], &[]))
        .unwrap();
    store
        .apply(&enable, Some(&confirm(&enable)), "own-request-enable")
        .unwrap();
    assert_eq!(store.current().revision, 2);

    let disable = store.plan(&request(2, &[], &["restaurant"])).unwrap();
    let blocked = disable
        .blocked
        .as_ref()
        .expect("laundry still requires restaurant");
    assert_eq!(blocked.reason, BlockedReason::MissingDependency);
    assert_eq!(blocked.module, "laundry");
    assert_eq!(disable.config, *store.current());
}

#[test]
fn t08_restore_creates_a_new_revision_and_never_rewrites_history() {
    let mut store = store();
    let enable = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    store
        .apply(&enable, Some(&confirm(&enable)), "own-request-enable")
        .unwrap();
    let add_laundry = store.plan(&request(2, &["laundry"], &[])).unwrap();
    store
        .apply(
            &add_laundry,
            Some(&confirm(&add_laundry)),
            "own-request-laundry",
        )
        .unwrap();
    assert_eq!(store.current().revision, 3);
    assert_eq!(store.history().len(), 3);

    // Restoring revision 1 (core only) is a NEW proposal against revision 3.
    let restored = store.restore(1).unwrap();
    assert_eq!(restored.base_revision, 3);
    assert_eq!(restored.config.revision, 4);
    assert_eq!(
        restored.config.enabled,
        BTreeSet::from(["core".to_string()])
    );
    assert_eq!(
        restored.changes,
        vec![
            Change::Disable {
                module: "laundry".to_string()
            },
            Change::Disable {
                module: "restaurant".to_string()
            },
        ]
    );
    let receipt = store
        .apply(&restored, Some(&confirm(&restored)), "own-request-restore")
        .unwrap();
    assert_eq!(receipt.revision, 4);
    // Every version is still readable; the restore appended version 4.
    let revisions: Vec<i64> = store
        .history()
        .iter()
        .map(|config| config.revision)
        .collect();
    assert_eq!(revisions, vec![1, 2, 3, 4]);

    assert_eq!(
        store.restore(99).unwrap_err(),
        RestoreError::UnknownRevision(99)
    );
}

#[test]
fn t08_a_restore_is_revalidated_against_the_current_catalogue() {
    let mut store = store();
    let enable = store.plan(&request(1, &["restaurant"], &[])).unwrap();
    store
        .apply(&enable, Some(&confirm(&enable)), "own-request-enable")
        .unwrap();

    // The release now reports restaurant as unavailable. Disabling it is
    // still possible; restoring the version that HAD it must be explicit.
    let mut moved = catalogue();
    for module in &mut moved.modules {
        if module.id == "restaurant" {
            module.readiness = Readiness::Unavailable;
        }
    }
    let mut moved_store = Store::new(moved, store.current().clone());
    let disable = moved_store.plan(&request(2, &[], &["restaurant"])).unwrap();
    assert!(disable.blocked.is_none());
    moved_store
        .apply(&disable, Some(&confirm(&disable)), "own-request-disable")
        .unwrap();
    assert_eq!(moved_store.current().revision, 3);

    let plan = moved_store.restore(2).unwrap();
    assert_eq!(
        plan.blocked.as_ref().map(|blocked| blocked.reason),
        Some(BlockedReason::ModuleUnavailable)
    );
    assert_eq!(plan.config, *moved_store.current());
}

#[test]
fn t01_relabelling_changes_the_label_but_not_the_canonical_stage_key() {
    let store = store();
    let mut labels = BTreeMap::new();
    labels.insert("new".to_string(), "Inquiry".to_string());
    let request = Request {
        base_revision: 1,
        enable: Vec::new(),
        disable: Vec::new(),
        terminology: Some(Terminology {
            pipeline_labels: labels,
        }),
    };
    let plan = store.plan(&request).unwrap();
    assert_eq!(
        plan.changes,
        vec![Change::RelabelStage {
            stage: "new".to_string(),
            label: "Inquiry".to_string(),
        }]
    );
    // The key set is canonical; only the value moved.
    let labels = &plan.config.terminology.pipeline_labels;
    assert_eq!(labels.get("new").map(String::as_str), Some("Inquiry"));
    assert_eq!(labels.get("won").map(String::as_str), Some("Won"));
    assert_eq!(labels.len(), 2);
    assert_eq!(plan.config.enabled, store.current().enabled);
}

#[test]
fn t09_a_request_cannot_invent_readiness_cost_or_authority() {
    // The request is CLOSED: readiness, cost, price and authority are
    // catalogue/host facts, never owner-supplied fields. A forged body dies
    // at deserialization instead of being echoed into a plan.
    let forged = serde_json::json!({
        "baseRevision": 1,
        "enable": ["restaurant"],
        "readiness": "ready",
        "cost": "included",
        "price": 0,
        "role": "owner"
    });
    assert!(serde_json::from_value::<Request>(forged).is_err());

    // Effects are copied from the trusted catalogue spec, not from the
    // request. Laundry is NeedsSetup/Usage regardless of what a caller wants.
    let store = store();
    let plan = store
        .plan(&request(1, &["laundry", "restaurant"], &[]))
        .unwrap();
    let laundry = plan
        .effects
        .iter()
        .find(|effect| effect.module == "laundry")
        .expect("laundry effect");
    assert_eq!(laundry.readiness, Readiness::NeedsSetup);
    assert_eq!(laundry.cost, CostClass::Usage);
}

#[test]
fn t07_a_mandatory_module_cannot_be_disabled() {
    let store = store();
    let plan = store.plan(&request(1, &[], &["core"])).unwrap();
    let blocked = plan.blocked.as_ref().expect("core is mandatory");
    assert_eq!(blocked.reason, BlockedReason::Mandatory);
    assert_eq!(blocked.module, "core");
    assert_eq!(plan.config, *store.current());
}
