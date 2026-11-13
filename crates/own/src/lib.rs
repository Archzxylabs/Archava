//! The deterministic owner-configuration engine — the shared core OWN-03,
//! OWN-05, OWN-06 and OWN-08 require.
//!
//! The owner PRD's lifecycle is inspect → discover → plan → preview → confirm →
//! apply → recover → history/restore. This crate owns the parts that must be
//! **deterministic and shared**: resolving a typed request against a trusted
//! catalogue, producing an immutable plan with an identity and an explicit
//! blocked result, binding a confirmation to the plan/base revision, applying
//! under idempotency and a writer fence, and restoring a prior configuration as
//! a NEW version after the same compatibility checks.
//!
//! What is deliberately NOT here: HTTP, persistence, identity/grants and the
//! browser. Those are the host's/product surface's jobs; this crate is the
//! seam both the dashboard and the assistant call, so a chat plan and a
//! dashboard plan cannot drift apart.
//!
//! ## Semantics that are testable here
//!
//! - **T02**: the same typed request produces the same plan identity.
//! - **T03**: the plan carries readiness/cost classification for every change.
//! - **T05**: an unavailable/incompatible request is an explicit `blocked`
//!   plan — never a silent partial apply.
//! - **T06**: a duplicate apply under the same request id yields ONE effect; a
//!   stale base refuses to plan/apply and demands a replan; a plan hash is
//!   bound to the confirmation.
//! - **T07**: disabling a module another enabled module requires is blocked.
//! - **T08**: restore reads history and produces a NEW configuration revision;
//!   canonical stage IDs never change with a label.
//!
//! The plan hash is a stable FNV-1a identity over a canonical string that
//! includes the catalogue/pricing identity and every module's readiness/cost.
//! It is a product-level plan identity, not a security boundary: authority and
//! confirmation are re-checked by the host against the current session.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod disclosure;
pub use disclosure::{DataClass, DisclosureChange, DisclosurePolicy};

/// A module's install/entitlement/readiness facts, from the trusted catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleSpec {
    pub id: String,
    /// Module ids this one requires to be enabled.
    #[serde(default)]
    pub requires: Vec<String>,
    /// The tenant's entitlement covers the module.
    pub entitled: bool,
    /// The module's artifact is installed on this host.
    pub installed: bool,
    /// A mandatory module cannot be disabled: the product cannot serve the
    /// tenant without it. (Optional modules can, subject to dependents.)
    #[serde(default)]
    pub mandatory: bool,
    pub readiness: Readiness,
    pub cost: CostClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    NeedsSetup,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Included,
    Recurring,
    Usage,
    Review,
}

/// The trusted catalogue. A module absent from it cannot be requested.
///
/// The catalogue and pricing identities are part of every plan identity: an
/// owner who approved `Ready/Included` under one catalogue is never executed
/// against another, and a re-priced module moves the plan hash even when the
/// enabled set does not change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub catalogue_version: String,
    pub pricing_version: String,
    pub modules: Vec<ModuleSpec>,
    /// Trusted data-consumer IDs can differ from management switch IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disclosure_modules: Option<BTreeSet<String>>,
}

impl Catalogue {
    fn module(&self, id: &str) -> Option<&ModuleSpec> {
        self.modules.iter().find(|module| module.id == id)
    }
}

/// Business terminology. The keys are CANONICAL stage ids; only the labels are
/// owner-facing, so relabelling never changes permissions or history meaning.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Terminology {
    #[serde(default)]
    pub pipeline_labels: BTreeMap<String, String>,
}

/// The persisted configuration: one revision, the enabled set and terminology.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerConfig {
    pub revision: i64,
    pub enabled: BTreeSet<String>,
    #[serde(default)]
    pub terminology: Terminology,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_disclosure: Option<DisclosurePolicy>,
}

/// A typed request. Both the dashboard and the assistant produce THIS shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub base_revision: i64,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub terminology: Option<Terminology>,
    /// Replace the WHOLE label set with this snapshot: a stage the snapshot
    /// does not name is DELETED, not merged. This is what restore means; a
    /// patch (`terminology`) only ever adds or relabels.
    #[serde(default)]
    pub replace_terminology: Option<Terminology>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_disclosure: Option<DisclosureChange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Change {
    Enable {
        module: String,
    },
    Disable {
        module: String,
    },
    RelabelStage {
        stage: String,
        label: String,
    },
    /// The stage's label is removed because the target snapshot never had it.
    RemoveStage {
        stage: String,
    },
    AiDisclosure {
        policy: Option<DisclosurePolicy>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub module: String,
    pub readiness: Readiness,
    pub cost: CostClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedReason {
    UnknownModule,
    NotEntitled,
    NotInstalled,
    ModuleUnavailable,
    MissingDependency,
    Mandatory,
    InvalidDisclosure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    pub reason: BlockedReason,
    pub module: String,
    pub detail: String,
}

/// An immutable proposal. `config` is the configuration that WOULD exist at
/// `base_revision + 1`; a blocked plan carries the unchanged current config and
/// must never be applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub plan_hash: String,
    pub catalogue_version: String,
    pub pricing_version: String,
    pub base_revision: i64,
    pub changes: Vec<Change>,
    pub effects: Vec<Effect>,
    pub blocked: Option<Blocked>,
    pub config: OwnerConfig,
}

/// A server-verifiable confirmation: it names the plan and the base it was
/// shown for. The authority itself is checked by the host, never by this body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Confirmation {
    pub plan_hash: String,
    pub base_revision: i64,
    /// The catalogue identity the confirming client holds. Optional — a client
    /// with only the hash is still bound, because the hash covers the catalogue
    /// — but when present it must name the SAME catalogue, so a stale screen is
    /// refused with a precise reason instead of a bare hash mismatch.
    #[serde(default)]
    pub catalogue_version: Option<String>,
    /// The pricing identity the confirming client holds; same contract.
    #[serde(default)]
    pub pricing_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub request_id: String,
    pub revision: i64,
    pub plan_hash: String,
    pub config: OwnerConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The request was planned against a configuration that has moved.
    StaleBase { current: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyError {
    Blocked(Blocked),
    MissingConfirmation,
    /// The confirmation names another plan/base than the one being applied.
    PlanMismatch,
    /// The plan (or the confirmation) was computed under another catalogue.
    CatalogueChanged {
        held: String,
        live: String,
    },
    /// The plan (or the confirmation) was computed under another pricing sheet.
    PricingChanged {
        held: String,
        live: String,
    },
    StaleBase {
        current: i64,
    },
    /// The plan's proposed revision is not exactly the next one.
    RevisionCollision {
        proposed: i64,
        expected: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreError {
    UnknownRevision(i64),
    Plan(PlanError),
}

/// The in-memory engine state: the catalogue, the current configuration, the
/// history (actual configurations only, oldest first) and the apply receipts.
pub struct Store {
    catalogue: Catalogue,
    current: OwnerConfig,
    history: Vec<OwnerConfig>,
    receipts: BTreeMap<String, Receipt>,
}

impl Store {
    pub fn new(catalogue: Catalogue, current: OwnerConfig) -> Self {
        let history = vec![current.clone()];
        Self {
            catalogue,
            current,
            history,
            receipts: BTreeMap::new(),
        }
    }

    pub fn current(&self) -> &OwnerConfig {
        &self.current
    }

    /// Every applied configuration, oldest first. Restore reads this; it never
    /// rewrites it.
    pub fn history(&self) -> &[OwnerConfig] {
        &self.history
    }

    pub fn plan(&self, request: &Request) -> Result<Plan, PlanError> {
        self.catalogue.plan(&self.current, request)
    }

    /// Apply a plan under a request id. The SAME id returns the SAME receipt
    /// and produces no second effect; nothing is written before every check
    /// passes.
    pub fn apply(
        &mut self,
        plan: &Plan,
        confirmation: Option<&Confirmation>,
        request_id: &str,
    ) -> Result<Receipt, ApplyError> {
        if let Some(receipt) = self.receipts.get(request_id) {
            return Ok(receipt.clone());
        }
        if let Some(blocked) = &plan.blocked {
            return Err(ApplyError::Blocked(blocked.clone()));
        }
        let Some(confirmation) = confirmation else {
            return Err(ApplyError::MissingConfirmation);
        };
        // A plan minted under another catalogue is not an answer this store can
        // apply, whatever its hash says about itself.
        if plan.catalogue_version != self.catalogue.catalogue_version {
            return Err(ApplyError::CatalogueChanged {
                held: plan.catalogue_version.clone(),
                live: self.catalogue.catalogue_version.clone(),
            });
        }
        if plan.pricing_version != self.catalogue.pricing_version {
            return Err(ApplyError::PricingChanged {
                held: plan.pricing_version.clone(),
                live: self.catalogue.pricing_version.clone(),
            });
        }
        // The plan must be the plan THIS catalogue mints: its identity is
        // re-derived from its own fields rather than trusted. A preview taken
        // under another catalogue classifies the same config differently, so it
        // fails here even when its versions happen to match.
        let identity = hash(&canonical(
            &self.catalogue,
            plan.base_revision,
            &plan.config,
            &plan.changes,
            &plan.effects,
            plan.blocked.as_ref(),
        ));
        if identity != plan.plan_hash {
            return Err(ApplyError::PlanMismatch);
        }
        // ... and the client must echo the identity it was SHOWN.
        if let Some(held) = &confirmation.catalogue_version {
            if held != &plan.catalogue_version {
                return Err(ApplyError::CatalogueChanged {
                    held: held.clone(),
                    live: plan.catalogue_version.clone(),
                });
            }
        }
        if let Some(held) = &confirmation.pricing_version {
            if held != &plan.pricing_version {
                return Err(ApplyError::PricingChanged {
                    held: held.clone(),
                    live: plan.pricing_version.clone(),
                });
            }
        }
        if confirmation.plan_hash != plan.plan_hash
            || confirmation.base_revision != plan.base_revision
        {
            return Err(ApplyError::PlanMismatch);
        }
        if plan.base_revision != self.current.revision {
            return Err(ApplyError::StaleBase {
                current: self.current.revision,
            });
        }
        let expected = self.current.revision + 1;
        if plan.config.revision != expected {
            return Err(ApplyError::RevisionCollision {
                proposed: plan.config.revision,
                expected,
            });
        }
        let receipt = Receipt {
            request_id: request_id.to_string(),
            revision: plan.config.revision,
            plan_hash: plan.plan_hash.clone(),
            config: plan.config.clone(),
        };
        self.history.push(plan.config.clone());
        self.current = plan.config.clone();
        self.receipts
            .insert(request_id.to_string(), receipt.clone());
        Ok(receipt)
    }

    /// Restore a prior configuration: the target is re-validated against the
    /// CURRENT catalogue and configuration, and the result is a NEW revision.
    /// History keeps every version; nothing is rewritten.
    pub fn restore(&self, target_revision: i64) -> Result<Plan, RestoreError> {
        let target = self
            .history
            .iter()
            .find(|config| config.revision == target_revision)
            .ok_or(RestoreError::UnknownRevision(target_revision))?;
        let enable = target
            .enabled
            .iter()
            .filter(|module| !self.current.enabled.contains(*module))
            .cloned()
            .collect();
        let disable = self
            .current
            .enabled
            .iter()
            .filter(|module| !target.enabled.contains(*module))
            .cloned()
            .collect();
        let request = Request {
            base_revision: self.current.revision,
            enable,
            disable,
            terminology: None,
            // The target snapshot IS the answer: a stage it does not name is
            // removed. Passing it as a patch would leave newer labels stuck.
            replace_terminology: Some(target.terminology.clone()),
            ai_disclosure: Some(match &target.ai_disclosure {
                Some(policy) => DisclosureChange::Replace {
                    policy: policy.clone(),
                },
                None => DisclosureChange::Revoke {},
            }),
        };
        self.plan(&request).map_err(RestoreError::Plan)
    }
}

impl Catalogue {
    /// The deterministic, stateless planner. The same inputs produce the same
    /// plan identity whether the caller is the dashboard, the assistant or a
    /// test; the persisted store is a thin, transactional caller of this.
    pub fn plan(&self, current: &OwnerConfig, request: &Request) -> Result<Plan, PlanError> {
        if request.base_revision != current.revision {
            return Err(PlanError::StaleBase {
                current: current.revision,
            });
        }
        Ok(self.build_plan(current, request))
    }

    fn build_plan(&self, current: &OwnerConfig, request: &Request) -> Plan {
        let mut enabled = current.enabled.clone();
        let mut changes = Vec::new();
        let mut effects = Vec::new();
        let mut blocked: Option<Blocked> = None;

        for module_id in &request.disable {
            match self.module(module_id) {
                None => {
                    blocked.get_or_insert_with(|| Blocked {
                        reason: BlockedReason::UnknownModule,
                        module: module_id.clone(),
                        detail: "The catalogue does not contain this module.".to_string(),
                    });
                    continue;
                }
                Some(module) if module.mandatory => {
                    blocked.get_or_insert_with(|| Blocked {
                        reason: BlockedReason::Mandatory,
                        module: module.id.clone(),
                        detail: "A mandatory module cannot be disabled.".to_string(),
                    });
                    continue;
                }
                Some(_) => {}
            }
            if enabled.remove(module_id) {
                changes.push(Change::Disable {
                    module: module_id.clone(),
                });
            }
        }
        for module_id in &request.enable {
            if blocked.is_some() {
                // One explicit refusal is the answer; later requests must be
                // replanned against the same base, not partially applied.
                break;
            }
            match self.module(module_id) {
                None => {
                    blocked = Some(Blocked {
                        reason: BlockedReason::UnknownModule,
                        module: module_id.clone(),
                        detail: "The catalogue does not contain this module.".to_string(),
                    });
                }
                Some(module) if !module.entitled => {
                    blocked = Some(Blocked {
                        reason: BlockedReason::NotEntitled,
                        module: module.id.clone(),
                        detail: "The tenant's entitlement does not cover this module.".to_string(),
                    });
                }
                Some(module) if !module.installed => {
                    blocked = Some(Blocked {
                        reason: BlockedReason::NotInstalled,
                        module: module.id.clone(),
                        detail: "The module's artifact is not installed on this host.".to_string(),
                    });
                }
                Some(module) if module.readiness == Readiness::Unavailable => {
                    blocked = Some(Blocked {
                        reason: BlockedReason::ModuleUnavailable,
                        module: module.id.clone(),
                        detail: "The module is unavailable for this release.".to_string(),
                    });
                }
                Some(module) => {
                    if enabled.insert(module.id.clone()) {
                        changes.push(Change::Enable {
                            module: module.id.clone(),
                        });
                        effects.push(Effect {
                            module: module.id.clone(),
                            readiness: module.readiness,
                            cost: module.cost,
                        });
                    }
                }
            }
        }
        if blocked.is_none() {
            // Compatibility over the FINAL set: every enabled module's
            // dependencies must also be enabled. Enabling a dependent while
            // its dependency is off, or disabling a dependency still required,
            // are the same explicit refusal.
            if let Some(module) = enabled.iter().find_map(|id| {
                self.module(id).and_then(|module| {
                    module
                        .requires
                        .iter()
                        .find(|required| !enabled.contains(*required))
                        .map(|missing| (module, missing))
                })
            }) {
                let (module, missing) = module;
                blocked = Some(Blocked {
                    reason: BlockedReason::MissingDependency,
                    module: module.id.clone(),
                    detail: format!(
                        "{} requires {} to stay enabled (an enable or a disable left it off).",
                        module.id, missing
                    ),
                });
            }
        }

        let mut terminology = current.terminology.clone();
        if let Some(snapshot) = &request.replace_terminology {
            // Replace, don't merge: every stage the snapshot does not name is
            // deleted, and the deletion is a change like any other.
            let stale: Vec<String> = terminology
                .pipeline_labels
                .keys()
                .filter(|stage| !snapshot.pipeline_labels.contains_key(*stage))
                .cloned()
                .collect();
            for stage in stale {
                terminology.pipeline_labels.remove(&stage);
                changes.push(Change::RemoveStage { stage });
            }
            for (stage, label) in &snapshot.pipeline_labels {
                if terminology.pipeline_labels.get(stage) != Some(label) {
                    terminology
                        .pipeline_labels
                        .insert(stage.clone(), label.clone());
                    changes.push(Change::RelabelStage {
                        stage: stage.clone(),
                        label: label.clone(),
                    });
                }
            }
        }
        if let Some(requested) = &request.terminology {
            for (stage, label) in &requested.pipeline_labels {
                if terminology.pipeline_labels.get(stage) != Some(label) {
                    terminology
                        .pipeline_labels
                        .insert(stage.clone(), label.clone());
                    changes.push(Change::RelabelStage {
                        stage: stage.clone(),
                        label: label.clone(),
                    });
                }
            }
        }

        let mut ai_disclosure = current.ai_disclosure.clone();
        if let Some(change) = &request.ai_disclosure {
            let proposed = match change {
                DisclosureChange::Replace { policy } => Some(policy.clone()),
                DisclosureChange::Revoke {} => None,
            };
            let installed = self
                .modules
                .iter()
                .filter(|m| m.installed && m.entitled && m.readiness != Readiness::Unavailable)
                .map(|m| m.id.clone())
                .collect();
            let areas = self.disclosure_modules.as_ref().unwrap_or(&installed);
            if proposed
                .as_ref()
                .is_some_and(|policy| !policy.valid_for(areas))
            {
                blocked.get_or_insert(Blocked {
                    reason: BlockedReason::InvalidDisclosure,
                    module: "ai_data".into(),
                    detail: "Review the provider binding, installed business areas, data categories and company scope.".into(),
                });
            } else if proposed != ai_disclosure {
                ai_disclosure = proposed;
                changes.push(Change::AiDisclosure {
                    policy: ai_disclosure.clone(),
                });
            }
        }

        changes.sort_by_key(change_line);
        // Effects are part of the answer the client reads, so their order is
        // canonical too — a reordered request must serialize identically.
        effects.sort_by(|left, right| left.module.cmp(&right.module));
        // A blocked plan proposes NOTHING: the config it carries is the
        // current one, revision included. Only an applicable plan names the
        // next revision.
        let config = if blocked.is_some() {
            current.clone()
        } else {
            OwnerConfig {
                revision: current.revision + 1,
                enabled,
                terminology,
                ai_disclosure,
            }
        };
        let plan_hash = hash(&canonical(
            self,
            request.base_revision,
            &config,
            &changes,
            &effects,
            blocked.as_ref(),
        ));
        Plan {
            plan_hash,
            catalogue_version: self.catalogue_version.clone(),
            pricing_version: self.pricing_version.clone(),
            base_revision: request.base_revision,
            changes,
            effects,
            blocked,
            config,
        }
    }
}

/// A canonical, stable rendering of the proposal — the plan's identity input.
///
/// Everything the owner is SHOWN is in here: the catalogue and pricing identity,
/// the enabled set together with its readiness/cost classification, the label
/// set, every change (deletions included) and every effect. Ordering comes from
/// `BTreeSet`/`BTreeMap` or an explicit sort — never from `HashMap` iteration,
/// so the same answer always renders the same bytes.
fn canonical(
    catalogue: &Catalogue,
    base_revision: i64,
    config: &OwnerConfig,
    changes: &[Change],
    effects: &[Effect],
    blocked: Option<&Blocked>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("catalogue={}\n", catalogue.catalogue_version));
    out.push_str(&format!("pricing={}\n", catalogue.pricing_version));
    out.push_str(&format!("base={base_revision}\n"));
    out.push_str(&format!("revision={}\n", config.revision));
    for module in &config.enabled {
        out.push_str(&format!(
            "module={module}={}\n",
            classification(catalogue, module)
        ));
    }
    for (stage, label) in &config.terminology.pipeline_labels {
        out.push_str(&format!("label={stage}={label}\n"));
    }
    if let Some(policy) = &config.ai_disclosure {
        out.push_str(&format!(
            "ai-disclosure={}\n",
            serde_json::to_string(policy).expect("disclosure policy is serializable")
        ));
    }
    let mut changes: Vec<String> = changes.iter().map(change_line).collect();
    changes.sort();
    for change in changes {
        out.push_str(&format!("change={change}\n"));
    }
    let mut effects: Vec<&Effect> = effects.iter().collect();
    effects.sort_by(|left, right| left.module.cmp(&right.module));
    for effect in effects {
        out.push_str(&format!(
            "effect={}={}/{}\n",
            effect.module,
            readiness_token(effect.readiness),
            cost_token(effect.cost)
        ));
    }
    if let Some(blocked) = blocked {
        out.push_str(&format!(
            "blocked={}:{}:{}\n",
            blocked_reason_token(blocked.reason),
            blocked.module,
            blocked.detail
        ));
    }
    out
}

/// One change as a stable `kind:subject=detail` line, so the identity never
/// depends on `Debug` formatting and a deletion is as visible as an addition.
fn change_line(change: &Change) -> String {
    match change {
        Change::Enable { module } => format!("enable:{module}"),
        Change::Disable { module } => format!("disable:{module}"),
        Change::RelabelStage { stage, label } => format!("relabel:{stage}={label}"),
        Change::RemoveStage { stage } => format!("remove:{stage}"),
        Change::AiDisclosure { policy } => format!(
            "ai-disclosure:{}",
            serde_json::to_string(policy).expect("disclosure policy is serializable")
        ),
    }
}

/// The catalogue's readiness/cost for `module` in canonical form. A module the
/// catalogue no longer names is `unknown`, so dropping it from the catalogue
/// moves the identity too.
fn classification(catalogue: &Catalogue, module: &str) -> String {
    match catalogue.module(module) {
        Some(spec) => format!(
            "{}/{}",
            readiness_token(spec.readiness),
            cost_token(spec.cost)
        ),
        None => "unknown".to_string(),
    }
}

fn readiness_token(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::Ready => "ready",
        Readiness::NeedsSetup => "needs_setup",
        Readiness::Unavailable => "unavailable",
    }
}

fn cost_token(cost: CostClass) -> &'static str {
    match cost {
        CostClass::Included => "included",
        CostClass::Recurring => "recurring",
        CostClass::Usage => "usage",
        CostClass::Review => "review",
    }
}

fn blocked_reason_token(reason: BlockedReason) -> &'static str {
    match reason {
        BlockedReason::UnknownModule => "unknown_module",
        BlockedReason::NotEntitled => "not_entitled",
        BlockedReason::NotInstalled => "not_installed",
        BlockedReason::ModuleUnavailable => "module_unavailable",
        BlockedReason::MissingDependency => "missing_dependency",
        BlockedReason::Mandatory => "mandatory",
        BlockedReason::InvalidDisclosure => "invalid_disclosure",
    }
}

fn hash(canonical: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
