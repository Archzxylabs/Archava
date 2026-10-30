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
        catalogue_version: "test-catalogue@1".to_string(),
        pricing_version: "test-pricing@1".to_string(),
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
        replace_terminology: None,
    }
}

fn confirm(plan: &Plan) -> Confirmation {
    Confirmation {
        plan_hash: plan.plan_hash.clone(),
        base_revision: plan.base_revision,
        catalogue_version: None,
        pricing_version: None,
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
        replace_terminology: None,
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

/// **A2/F04 — the plan identity binds the catalogue's readiness/cost.**
///
/// Two catalogues that agree on every module id and disagree only on the
/// classification the owner is shown must be two plan identities. Otherwise a
/// confirmation given for `Ready/Included` keeps applying after the catalogue
/// moved to `NeedsSetup/Usage`.
#[test]
fn a2_a_catalogue_change_is_a_plan_identity_change() {
    let cheap = catalogue();
    let mut dear = catalogue();
    for module in &mut dear.modules {
        match module.id.as_str() {
            // The cost of an ALREADY-ENABLED module moved: no enable, no
            // change, no effect — and still a different answer to the owner.
            "core" => module.cost = CostClass::Review,
            "restaurant" => {
                module.readiness = Readiness::NeedsSetup;
                module.cost = CostClass::Usage;
            }
            _ => {}
        }
    }

    let idle = request(1, &[], &[]);
    let cheap_idle = cheap.plan(&base(), &idle).unwrap();
    let dear_idle = dear.plan(&base(), &idle).unwrap();
    assert_ne!(
        cheap_idle.plan_hash, dear_idle.plan_hash,
        "the readiness/cost of the enabled set must be part of the plan identity"
    );

    let enable = request(1, &["restaurant"], &[]);
    let cheap_plan = cheap.plan(&base(), &enable).unwrap();
    let dear_plan = dear.plan(&base(), &enable).unwrap();
    assert_ne!(
        cheap_plan.plan_hash, dear_plan.plan_hash,
        "two catalogues that differ only in effects cannot share a plan hash"
    );

    // The confirmation the owner gave for the cheap preview must not commit
    // the dear catalogue: the identity moved, so apply refuses it.
    let mut store = Store::new(dear, base());
    let stale = confirm(&cheap_plan);
    assert_eq!(
        store.apply(&cheap_plan, Some(&stale), "own-request-stale-catalogue"),
        Err(ApplyError::PlanMismatch),
        "a plan previewed under another catalogue was applied"
    );
    assert_eq!(store.current().revision, 1);
    assert_eq!(store.history().len(), 1);
}

/// **A3/F06 — restore returns the label set to the target SNAPSHOT.**
///
/// A label the target revision never had must be removed by the restore plan,
/// and the removal must be part of the plan's identity. Merging labels into
/// the current set leaves newer terminology stuck forever.
#[test]
fn a3_restore_removes_the_labels_the_target_never_had() {
    let mut store = store();
    let mut labels = BTreeMap::new();
    labels.insert("qualified".to_string(), "Bid".to_string());
    let relabel = Request {
        base_revision: 1,
        enable: Vec::new(),
        disable: Vec::new(),
        terminology: Some(Terminology {
            pipeline_labels: labels,
        }),
        replace_terminology: None,
    };
    let plan = store.plan(&relabel).unwrap();
    store
        .apply(&plan, Some(&confirm(&plan)), "own-request-relabel")
        .unwrap();
    assert_eq!(store.current().terminology.pipeline_labels.len(), 3);

    let target = store.history()[0].clone();
    let restored = store.restore(1).unwrap();
    assert_eq!(
        restored.config.terminology.pipeline_labels, target.terminology.pipeline_labels,
        "restore must land the target snapshot's labels, not a merge"
    );
    let removed: Vec<String> = restored
        .changes
        .iter()
        .filter_map(|change| {
            let value = serde_json::to_value(change).ok()?;
            (value["kind"] == "removeStage")
                .then(|| value["stage"].as_str().unwrap_or_default().to_string())
        })
        .collect();
    assert_eq!(
        removed,
        vec!["qualified".to_string()],
        "restore must propose deleting the label the target never had; got {:?}",
        restored.changes
    );

    // The removal is part of the identity: a no-op plan at the same base is a
    // different plan, so a confirmation for one cannot apply the other.
    let noop = store.plan(&request(2, &[], &[])).unwrap();
    assert_ne!(
        restored.plan_hash, noop.plan_hash,
        "a restore that only deletes a label cannot share a no-op's identity"
    );
    store
        .apply(
            &restored,
            Some(&confirm(&restored)),
            "own-request-restore-labels",
        )
        .unwrap();
    assert_eq!(
        store.current().terminology.pipeline_labels,
        target.terminology.pipeline_labels
    );
}

/// **A2/F04 — the identity is a function of the ANSWER, not of the order the
/// client typed it in.** Two requests that differ only in the order of their
/// module list must produce the same identity and the same ordered changes;
/// nothing may ride on `HashMap` iteration.
#[test]
fn a2_plan_identity_does_not_depend_on_request_order() {
    let engine = store();
    let typed = engine
        .plan(&request(1, &["restaurant", "laundry"], &[]))
        .unwrap();
    let reversed = engine
        .plan(&request(1, &["laundry", "restaurant"], &[]))
        .unwrap();
    assert_eq!(typed.plan_hash, reversed.plan_hash);
    assert_eq!(typed.changes, reversed.changes);
    assert_eq!(typed.effects, reversed.effects);
    // ... and the identity is reproducible byte-for-byte across instances.
    assert_eq!(
        typed,
        store()
            .plan(&request(1, &["laundry", "restaurant"], &[]))
            .unwrap()
    );
}

/// **A2/F04 — a plan minted under another catalogue/pricing identity is not an
/// answer this store can apply, whatever its hash says about itself.**
#[test]
fn a2_a_moved_catalogue_identity_refuses_the_stale_plan() {
    let mut moved = catalogue();
    moved.catalogue_version = "test-catalogue@2".to_string();
    moved.pricing_version = "test-pricing@2".to_string();
    let mut engine = Store::new(moved, base());
    let stale_plan = catalogue().plan(&base(), &request(1, &[], &[])).unwrap();

    assert_eq!(
        engine.apply(
            &stale_plan,
            Some(&confirm(&stale_plan)),
            "own-request-moved-catalogue",
        ),
        Err(ApplyError::CatalogueChanged {
            held: "test-catalogue@1".to_string(),
            live: "test-catalogue@2".to_string(),
        }),
        "a plan previewed under another catalogue was applied"
    );

    // Same store, same plan, but the client confirms the identity it was SHOWN.
    let mut engine = store();
    let plan = engine.plan(&request(1, &["restaurant"], &[])).unwrap();
    let mut stale = confirm(&plan);
    stale.catalogue_version = Some("test-catalogue@0".to_string());
    assert_eq!(
        engine.apply(&plan, Some(&stale), "own-request-stale-echo"),
        Err(ApplyError::CatalogueChanged {
            held: "test-catalogue@0".to_string(),
            live: "test-catalogue@1".to_string(),
        }),
        "a confirmation naming another catalogue was executed"
    );

    let mut stale = confirm(&plan);
    stale.pricing_version = Some("test-pricing@0".to_string());
    assert_eq!(
        engine.apply(&plan, Some(&stale), "own-request-stale-pricing-echo"),
        Err(ApplyError::PricingChanged {
            held: "test-pricing@0".to_string(),
            live: "test-pricing@1".to_string(),
        }),
        "a confirmation naming another pricing sheet was executed"
    );

    // The honest echo applies — the binding is a gate, not a wall.
    let mut honest = confirm(&plan);
    honest.catalogue_version = Some("test-catalogue@1".to_string());
    honest.pricing_version = Some("test-pricing@1".to_string());
    let receipt = engine
        .apply(&plan, Some(&honest), "own-request-honest-echo")
        .expect("an honest confirmation must apply");
    assert_eq!(receipt.plan_hash, plan.plan_hash);
    assert_eq!(receipt.revision, plan.base_revision + 1);
}
