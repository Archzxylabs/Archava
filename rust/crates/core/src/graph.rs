//! Page-awareness context graph (§16) — port of `packages/core/src/context-graph.ts`.
//!
//! The graph is a *pure* value: it is built only by applying the reducer below
//! to a seed, plus context events an SDK emits on the host page. There is no
//! hidden ambient state, which keeps the whole thing serialisable and testable.
//! The primary source of truth is structured SDK/DOM/application state; vision
//! would be an optional enrichment layer and is never modelled as authoritative.
//!
//! What this port carries and what it leaves on the TypeScript side:
//!
//! - carried: the shape (with the baseline's validation constraints), the seed,
//!   the pure reducer and event fold, and `parse` of an untrusted graph — the
//!   pieces the turn loop consumes server-side (a turn projects and masks a
//!   graph it did not build);
//! - the SDK-side *hosting* of the reducer (receivers, page observers) stays on
//!   the TypeScript side of the R09 SDK/widget slice, exactly as it does today;
//!   only the value travels.
//!
//! Validation parity (`parse`): the baseline's `contextGraphSchema` constrains
//! several fields beyond "a string exists" — route/kind/locale minimum
//! non-emptiness, 3-character currency codes, non-negative counts, positive
//! totals. The port refuses the same malformed inputs; the message text is this
//! port's own (readable paths, not zod's), because nothing consumes the exact
//! wording — notably not the SDK, which never sees a server-side refusal.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::Value::Object;

/// Absolute path of the current route, e.g. `/rooms`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPage {
    pub route: String,
    /// Human page kind derived from the route/template page vocabulary.
    pub kind: String,
    /// Stable identifier for the current section within the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub locale: String,
}

/// One entity the page contains. `kind` is `product` | `service` | `resource`
/// | `offer` | `location` in practice; the graph does not close it so a hosting
/// application can extend the vocabulary without a migration here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextEntityRef {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextComparison {
    pub entity_ids: Vec<String>,
    #[serde(rename = "metric", default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
}

/// A form the visitor is working through. The three field lists carry *names*
/// and never values — the graph has no place to put a form value, which is why
/// the projection (`crates/context`) can be checked by construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextForm {
    /// Schema-stable form identifier, e.g. `booking_request`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(rename = "completedFields", default)]
    pub completed_fields: Vec<String>,
    /// Never a value from the client: field *names* only.
    #[serde(rename = "pendingFields", default)]
    pub pending_fields: Vec<String>,
    /// Fields present in the form that must not reach the model.
    #[serde(rename = "maskedFields", default)]
    pub masked_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextCart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(rename = "lineCount")]
    pub line_count: u32,
    /// Subtotal is authoritative only when the host supplies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtotal: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextCheckout {
    pub step: String,
    /// Ordinal + total, e.g. `{ index: 1, total: 3 }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSession {
    /// True only when the host page has authenticated the visitor.
    pub authenticated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Opaque, host-supplied identifier. Never contains profile data.
    #[serde(
        rename = "customerRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub customer_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextError {
    /// Snapshot-stable message, e.g. `booking_slot_unavailable`.
    pub code: String,
    #[serde(rename = "occurredAt")]
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextActionAvailability {
    pub name: String,
    pub enabled: bool,
}

/// The one graph value the whole SDK/turn pipeline shares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextGraph {
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    pub page: ContextPage,
    /// Entities the page carries. Ids, names and kinds only — never a value,
    /// which is what makes the projection (downstream, in `archava-context`)
    /// checkable by construction.
    #[serde(default)]
    pub entities: Vec<ContextEntityRef>,
    #[serde(
        rename = "comparison",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub comparison: Option<ContextComparison>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<ContextForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cart: Option<ContextCart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout: Option<ContextCheckout>,
    pub session: ContextSession,
    #[serde(rename = "availableActions", default)]
    pub available_actions: Vec<ContextActionAvailability>,
    #[serde(default)]
    pub errors: Vec<ContextError>,
    #[serde(
        rename = "activePanel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub active_panel: Option<String>,
}

/// Context events. Every mutation to the graph is one of these, so the graph is
/// fully replayable from (seed + event list) — which is what the tests and the
/// assistant's turn-scoped snapshots rely on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContextEvent {
    #[serde(rename = "page/route")]
    PageRoute {
        route: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    #[serde(rename = "page/section")]
    PageSection { section: String },
    #[serde(rename = "page/locale")]
    PageLocale { locale: String },
    #[serde(rename = "page/activePanel")]
    PageActivePanel { panel: Option<String> },
    #[serde(rename = "entities/set")]
    EntitiesSet { entities: Vec<ContextEntityRef> },
    #[serde(rename = "entities/select")]
    EntitiesSelect { entity_id: String },
    #[serde(rename = "entities/deselect")]
    EntitiesDeselect,
    #[serde(rename = "comparison/set")]
    ComparisonSet {
        entity_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metric: Option<String>,
    },
    #[serde(rename = "form/set")]
    FormSet { form: Option<ContextForm> },
    #[serde(rename = "form/fieldCompleted")]
    FormFieldCompleted { field: String },
    #[serde(rename = "cart/set")]
    CartSet { cart: Option<ContextCart> },
    #[serde(rename = "checkout/step")]
    CheckoutStep {
        step: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<u32>,
    },
    #[serde(rename = "session/authenticated")]
    SessionAuthenticated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(
            rename = "customerRef",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        customer_ref: Option<String>,
    },
    #[serde(rename = "session/anonymous")]
    SessionAnonymous,
    #[serde(rename = "action/set")]
    ActionSet {
        actions: Vec<ContextActionAvailability>,
    },
    #[serde(rename = "action/enabled")]
    ActionEnabled { name: String, enabled: bool },
    #[serde(rename = "error/recorded")]
    ErrorRecorded { code: String, occurred_at: String },
    #[serde(rename = "error/cleared")]
    ErrorCleared,
}

/// Seed a graph for a tenant.
///
/// The baseline seeds from a resolved client config; in Rust the two fields it
/// reads (`tenantId`, `primaryLanguage`) are given directly so this crate does
/// not have to pull the config port into core. Same values, same shape.
pub fn seed_context_graph(tenant_id: &str, primary_language: &str, route: &str) -> ContextGraph {
    ContextGraph {
        tenant_id: tenant_id.to_string(),
        page: ContextPage {
            route: route.to_string(),
            kind: "home".to_string(),
            section: None,
            locale: primary_language.to_string(),
        },
        entities: Vec::new(),
        comparison: Some(ContextComparison {
            entity_ids: Vec::new(),
            metric: None,
        }),
        form: None,
        cart: None,
        checkout: None,
        session: ContextSession {
            authenticated: false,
            role: None,
            customer_ref: None,
        },
        available_actions: Vec::new(),
        errors: Vec::new(),
        active_panel: None,
    }
}

/// Pure reducer: (graph, event) => graph. Never mutates its input.
pub fn reduce_context_graph(graph: &ContextGraph, event: &ContextEvent) -> ContextGraph {
    let mut next = graph.clone();
    match event {
        ContextEvent::PageRoute { route, kind } => {
            next.page.route = route.clone();
            if let Some(kind) = kind {
                next.page.kind = kind.clone();
            }
        }
        ContextEvent::PageSection { section } => next.page.section = Some(section.clone()),
        ContextEvent::PageLocale { locale } => next.page.locale = locale.clone(),
        ContextEvent::PageActivePanel { panel } => next.active_panel = panel.clone(),
        ContextEvent::EntitiesSet { entities } => next.entities = entities.clone(),
        // Baseline semantics: a select for an entity already on the page is a
        // no-op — a visitor re-reading the same offer is one fact, not two.
        ContextEvent::EntitiesSelect { entity_id } => {
            if !next.entities.iter().any(|entity| *entity.id == *entity_id) {
                next.entities.push(ContextEntityRef {
                    id: entity_id.clone(),
                    name: entity_id.clone(),
                    kind: "unknown".to_string(),
                });
            }
        }
        ContextEvent::EntitiesDeselect => next.entities = Vec::new(),
        ContextEvent::ComparisonSet { entity_ids, metric } => {
            next.comparison = Some(ContextComparison {
                entity_ids: entity_ids.clone(),
                metric: metric.clone(),
            });
        }
        ContextEvent::FormSet { form } => next.form = form.clone(),
        ContextEvent::FormFieldCompleted { field } => {
            // Absent form: a completed-field event names no form the graph
            // holds, so the graph changes no other field either.
            let Some(form) = next.form.as_mut() else {
                return next;
            };
            if !form.completed_fields.iter().any(|f| f == field) {
                form.completed_fields.push(field.clone());
            }
            form.pending_fields.retain(|f| f != field);
        }
        ContextEvent::CartSet { cart } => next.cart = cart.clone(),
        ContextEvent::CheckoutStep { step, index, total } => {
            next.checkout = Some(ContextCheckout {
                step: step.clone(),
                index: *index,
                total: *total,
            });
        }
        ContextEvent::SessionAuthenticated { role, customer_ref } => {
            next.session = ContextSession {
                authenticated: true,
                role: role.clone(),
                customer_ref: customer_ref.clone(),
            };
        }
        ContextEvent::SessionAnonymous => {
            next.session = ContextSession {
                authenticated: false,
                role: None,
                customer_ref: None,
            };
        }
        ContextEvent::ActionSet { actions } => next.available_actions = actions.clone(),
        ContextEvent::ActionEnabled { name, enabled } => {
            for action in &mut next.available_actions {
                if action.name == *name {
                    action.enabled = *enabled;
                }
            }
        }
        ContextEvent::ErrorRecorded { code, occurred_at } => {
            next.errors.push(ContextError {
                code: code.clone(),
                occurred_at: occurred_at.clone(),
            });
        }
        ContextEvent::ErrorCleared => next.errors = Vec::new(),
    }
    next
}

/// Apply an ordered event list to a seed graph.
pub fn fold_context_events(graph: &ContextGraph, events: &[ContextEvent]) -> ContextGraph {
    let mut current = graph.clone();
    for event in events {
        current = reduce_context_graph(&current, event);
    }
    current
}

/// A `Value` the graph validation refused, with the reason a reader can act on.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextGraphError(String);

impl ContextGraphError {
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContextGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ContextGraphError {}

fn invalid(path: &str, problem: &'static str) -> ContextGraphError {
    ContextGraphError(format!("graph.{path}: {problem}"))
}

fn non_empty_string(value: &Value) -> bool {
    matches!(value, Value::String(text) if !text.is_empty())
}

fn required<'a>(
    path: &'static str,
    value: Option<&'a Value>,
) -> Result<&'a Value, ContextGraphError> {
    value
        .filter(|value| non_empty_string(value))
        .ok_or_else(|| match value {
            Some(Value::String(_)) => invalid(path, "must not be empty"),
            _ => invalid(path, "must be a non-empty string"),
        })
}

fn optional_string(
    path: &'static str,
    value: Option<&Value>,
) -> Result<Option<String>, ContextGraphError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if !text.is_empty() => Ok(Some(text.clone())),
        Some(Value::String(_)) => Err(invalid(path, "must not be empty")),
        Some(_) => Err(invalid(path, "must be a string")),
    }
}

fn string_list(
    path: &'static str,
    value: Option<&Value>,
) -> Result<Vec<String>, ContextGraphError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut fields = Vec::new();
            for item in items {
                if !non_empty_string(item) {
                    return Err(invalid(path, "entries must be non-empty strings"));
                }
                fields.push(item.as_str().unwrap_or_default().to_string());
            }
            Ok(fields)
        }
        Some(_) => Err(invalid(path, "must be a list of strings")),
    }
}

fn non_negative(path: &'static str, value: &Value) -> Result<u32, ContextGraphError> {
    let Some(number) = value.as_f64() else {
        return Err(invalid(path, "must be a non-negative integer"));
    };
    if number < 0.0 || number.fract() != 0.0 {
        return Err(invalid(path, "must be a non-negative integer"));
    }
    let whole = number as i64;
    if whole as f64 != number {
        return Err(invalid(path, "must be an exact integer"));
    }
    u32::try_from(whole).map_err(|_| invalid(path, "exceeds 32 bits"))
}

/// Validate an untrusted graph (e.g. arriving from the SDK over the wire).
pub fn parse_context_graph(raw: &Value) -> Result<ContextGraph, ContextGraphError> {
    let Object(map) = raw else {
        return Err(ContextGraphError("graph must be an object".to_string()));
    };
    let tenant_id = required("tenantId", map.get("tenantId"))?
        .as_str()
        .unwrap_or_default()
        .to_string();
    let page_route = required("page.route", map.get("page").and_then(|p| p.get("route")))?
        .as_str()
        .unwrap_or_default()
        .to_string();
    let page_kind = required("page.kind", map.get("page").and_then(|p| p.get("kind")))?
        .as_str()
        .unwrap_or_default()
        .to_string();
    let page_locale = required("page.locale", map.get("page").and_then(|p| p.get("locale")))?
        .as_str()
        .unwrap_or_default()
        .to_string();
    let page_section = optional_string(
        "page.section",
        map.get("page").and_then(|p| p.get("section")),
    )?;

    let entities = match map.get("entities") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            let mut entities = Vec::new();
            for item in items {
                let Object(object) = item else {
                    return Err(invalid("entities", "entries must be objects"));
                };
                let id = required("entities.id", object.get("id"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let name = required("entities.name", object.get("name"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let kind = required("entities.kind", object.get("kind"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                entities.push(ContextEntityRef { id, name, kind });
            }
            entities
        }
        Some(_) => return Err(invalid("entities", "must be a list")),
    };

    let comparison = match map.get("comparison") {
        None => None,
        Some(Object(object)) => Some(ContextComparison {
            entity_ids: string_list("comparison.entityIds", object.get("entityIds"))?,
            metric: optional_string("comparison.metric", object.get("metric"))?,
        }),
        Some(_) => return Err(invalid("comparison", "must be an object")),
    };

    let form = match map.get("form") {
        None => None,
        Some(Object(object)) => Some(ContextForm {
            id: required("form.id", object.get("id"))?
                .as_str()
                .unwrap_or_default()
                .to_string(),
            step: optional_string("form.step", object.get("step"))?,
            completed_fields: string_list("form.completedFields", object.get("completedFields"))?,
            pending_fields: string_list("form.pendingFields", object.get("pendingFields"))?,
            masked_fields: string_list("form.maskedFields", object.get("maskedFields"))?,
        }),
        Some(_) => return Err(invalid("form", "must be an object")),
    };

    let cart = match map.get("cart") {
        None => None,
        Some(Object(object)) => {
            let currency = match object.get("currency") {
                None | Some(Value::Null) => None,
                Some(Value::String(currency)) if currency.len() == 3 => Some(currency.clone()),
                Some(Value::String(_)) => {
                    return Err(invalid("cart.currency", "must be 3 characters"))
                }
                Some(_) => return Err(invalid("cart.currency", "must be a string")),
            };
            // `line_count` is required (the baseline gives it no default, so a
            // cart without a count is the same shape error as a negative one).
            let line_count = non_negative(
                "cart.lineCount",
                object.get("lineCount").unwrap_or(&Value::Null),
            )?;
            let subtotal = match object.get("subtotal") {
                None | Some(Value::Null) => None,
                Some(Value::Number(number)) => {
                    let value = number.as_f64().unwrap_or_default();
                    if value < 0.0 {
                        return Err(invalid("cart.subtotal", "must be non-negative"));
                    }
                    Some(value)
                }
                Some(_) => return Err(invalid("cart.subtotal", "must be a number")),
            };
            Some(ContextCart {
                currency,
                line_count,
                subtotal,
            })
        }
        Some(_) => return Err(invalid("cart", "must be an object")),
    };

    let checkout = match map.get("checkout") {
        None => None,
        Some(Object(object)) => {
            let step = required("checkout.step", object.get("step"))?
                .as_str()
                .unwrap_or_default()
                .to_string();
            let index = match object.get("index") {
                None | Some(Value::Null) => None,
                Some(value) => Some(non_negative("checkout.index", value)?),
            };
            let total = match object.get("total") {
                None | Some(Value::Null) => None,
                Some(Value::Number(number)) if number.as_f64().unwrap_or_default() > 0.0 => Some(
                    non_negative("checkout.total", object.get("total").unwrap())?,
                ),
                Some(_) => return Err(invalid("checkout.total", "must be a positive integer")),
            };
            Some(ContextCheckout { step, index, total })
        }
        Some(_) => return Err(invalid("checkout", "must be an object")),
    };

    let session = match map.get("session") {
        Some(Object(object)) => {
            let Some(Value::Bool(authenticated)) = object.get("authenticated") else {
                return Err(invalid("session.authenticated", "must be a boolean"));
            };
            ContextSession {
                authenticated: *authenticated,
                role: optional_string("session.role", object.get("role"))?,
                customer_ref: optional_string("session.customerRef", object.get("customerRef"))?,
            }
        }
        _ => return Err(invalid("session", "must be an object")),
    };

    let available_actions = match map.get("availableActions") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            let mut actions = Vec::new();
            for item in items {
                let Object(object) = item else {
                    return Err(invalid("availableActions", "entries must be objects"));
                };
                let name = required("availableActions.name", object.get("name"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let Some(Value::Bool(enabled)) = object.get("enabled") else {
                    return Err(invalid("availableActions.enabled", "must be a boolean"));
                };
                actions.push(ContextActionAvailability {
                    name,
                    enabled: *enabled,
                });
            }
            actions
        }
        Some(_) => return Err(invalid("availableActions", "must be a list")),
    };

    let errors = match map.get("errors") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            let mut errors = Vec::new();
            for item in items {
                let Object(object) = item else {
                    return Err(invalid("errors", "entries must be objects"));
                };
                let code = required("errors.code", object.get("code"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let occurred_at = required("errors.occurredAt", object.get("occurredAt"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                errors.push(ContextError { code, occurred_at });
            }
            errors
        }
        Some(_) => return Err(invalid("errors", "must be a list")),
    };

    let active_panel = optional_string("activePanel", map.get("activePanel"))?;

    Ok(ContextGraph {
        tenant_id,
        page: ContextPage {
            route: page_route,
            kind: page_kind,
            section: page_section,
            locale: page_locale,
        },
        entities,
        comparison,
        form,
        cart,
        checkout,
        session,
        available_actions,
        errors,
        active_panel,
    })
}
