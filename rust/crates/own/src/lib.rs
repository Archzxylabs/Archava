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
//! The plan hash is a stable FNV-1a identity over a canonical string. It is a
//! product-level plan identity, not a security boundary: authority and
//! confirmation are re-checked by the host against the current session.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub modules: Vec<ModuleSpec>,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Change {
    Enable { module: String },
    Disable { module: String },
    RelabelStage { stage: String, label: String },
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
        if request.base_revision != self.current.revision {
            return Err(PlanError::StaleBase {
                current: self.current.revision,
            });
        }
        Ok(self.build_plan(request))
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
            terminology: Some(target.terminology.clone()),
        };
        self.plan(&request).map_err(RestoreError::Plan)
    }

    fn build_plan(&self, request: &Request) -> Plan {
        let mut enabled = self.current.enabled.clone();
        let mut changes = Vec::new();
        let mut effects = Vec::new();
        let mut blocked: Option<Blocked> = None;

        for module_id in &request.disable {
            if self.catalogue.module(module_id).is_none() {
                blocked.get_or_insert_with(|| Blocked {
                    reason: BlockedReason::UnknownModule,
                    module: module_id.clone(),
                    detail: "The catalogue does not contain this module.".to_string(),
                });
                continue;
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
            match self.catalogue.module(module_id) {
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
                self.catalogue.module(id).and_then(|module| {
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

        let mut terminology = self.current.terminology.clone();
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

        changes.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
        // A blocked plan proposes NOTHING: the config it carries is the
        // current one, revision included. Only an applicable plan names the
        // next revision.
        let config = if blocked.is_some() {
            self.current.clone()
        } else {
            OwnerConfig {
                revision: self.current.revision + 1,
                enabled,
                terminology,
            }
        };
        let plan_hash = hash(&canonical(
            &self.current,
            &config,
            &changes,
            blocked.as_ref(),
        ));
        Plan {
            plan_hash,
            base_revision: request.base_revision,
            changes,
            effects,
            blocked,
            config,
        }
    }
}

/// A canonical, stable rendering of the proposal — the plan's identity input.
fn canonical(
    base: &OwnerConfig,
    config: &OwnerConfig,
    changes: &[Change],
    blocked: Option<&Blocked>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("base={}\n", base.revision));
    out.push_str(&format!("revision={}\n", config.revision));
    for module in &config.enabled {
        out.push_str(&format!("module={module}\n"));
    }
    for (stage, label) in &config.terminology.pipeline_labels {
        out.push_str(&format!("label={stage}={label}\n"));
    }
    for change in changes {
        out.push_str(&format!("change={change:?}\n"));
    }
    if let Some(blocked) = blocked {
        out.push_str(&format!(
            "blocked={:?}:{}:{}\n",
            blocked.reason, blocked.module, blocked.detail
        ));
    }
    out
}

fn hash(canonical: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
