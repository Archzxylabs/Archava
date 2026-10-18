//! Turn context: the projection to what a model may see, and the §16 mask at
//! the boundary. Port of the two sections of `packages/assistant/src/turn.ts`
//! that own this seam — `projectContextGraph` and `maskAtBoundary` — with
//! `enabledPageActions`, the helper both consume.
//!
//! The projection is the reason the masker is a second line of defence rather
//! than the only one: it emits ids, labels, counts, and flags — never a form
//! value, because the graph holds field *names* and not field values. A value
//! cannot be projected because it was never carried. The masker then masks the
//! projection itself: belt-and-braces for a host that later learns a field it
//! has been echoing is sensitive, without that host having to remember to stop
//! echoing it.
//!
//! Port difference: the source resolves an action's sensitive fields through an
//! injected `ActionPolicy` (`definitionFor(actionId, request.policy)`). This
//! port asks [`archava_core::actions::ActionRegistry`] — the
//! R03-pinned registry, the same table the policy gate reads. A host that wants
//! tenant-viewed definitions provides the turn loop with its own policy seam at
//! the action gate (a later slice), exactly as the source does; the projection
//! and the mask rule are unchanged.

use archava_core::actions::ActionRegistry;
use archava_core::ContextGraph;
use serde_json::{json, Value};

use crate::mask::{mask_sensitive_fields, resolve_mask_rules, MaskOptions, MaskedResult};

/// Render an optional section as a value or `null`.
///
/// `null` rather than omission: an absent cart is a fact worth stating ("there
/// is no cart"), and an absent key would force every consumer to distinguish
/// "missing" from "present but empty".
fn section<T>(value: Option<&T>, present: impl Fn(&T) -> Value) -> Value {
    match value {
        Some(value) => present(value),
        None => Value::Null,
    }
}

/// Project the graph to what a model may see (PRD §16).
pub fn project_context_graph(graph: &ContextGraph) -> Value {
    json!({
        "route": graph.page.route,
        "pageKind": graph.page.kind,
        "section": graph.page.section,
        "locale": graph.page.locale,
        "entities": graph
            .entities
            .iter()
            .map(|entity| {
                json!({
                    "id": entity.id,
                    "name": entity.name,
                    "entityKind": entity.kind,
                })
            })
            .collect::<Vec<Value>>(),
        "comparison": section(graph.comparison.as_ref(), |comparison| {
            json!({
                "entityIds": comparison.entity_ids,
                "metric": comparison.metric,
            })
        }),
        "form": section(graph.form.as_ref(), |form| {
            json!({
                "id": form.id,
                "step": form.step,
                "completedFields": form.completed_fields,
                "pendingFields": form.pending_fields,
                "withheldFields": form.masked_fields,
            })
        }),
        "cart": section(graph.cart.as_ref(), |cart| {
            json!({
                "currency": cart.currency,
                "lineCount": cart.line_count,
                "subtotal": cart.subtotal,
            })
        }),
        "checkout": section(graph.checkout.as_ref(), |checkout| {
            json!({
                "step": checkout.step,
                "index": checkout.index,
                "total": checkout.total,
            })
        }),
        "errors": graph
            .errors
            .iter()
            .map(|error| {
                json!({
                    "code": error.code,
                    "occurredAt": error.occurred_at,
                })
            })
            .collect::<Vec<Value>>(),
        "availableActions": graph
            .available_actions
            .iter()
            .map(|action| {
                json!({
                    "name": action.name,
                    "enabled": action.enabled,
                })
            })
            .collect::<Vec<Value>>(),
        "activePanel": graph.active_panel,
        "session": {
            "authenticated": graph.session.authenticated,
            "role": graph.session.role,
            "customerRef": graph.session.customer_ref,
        },
    })
}

/// The enabled page actions, as identities the policy gate understands.
pub fn enabled_page_actions(graph: &ContextGraph) -> Vec<String> {
    graph
        .available_actions
        .iter()
        .filter(|action| action.enabled)
        .map(|action| action.name.clone())
        .collect()
}

/// What the boundary hands the model: the projection, masked, with a notice
/// for every field the mask touched.
///
/// Mirrors the source's `{ value, notices }` return from `maskAtBoundary`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedGraph {
    pub value: Value,
    pub notices: Vec<String>,
}

/// Apply §16 at the boundary: mask the projected context.
///
/// The rules combine what the enabled actions declare as sensitive with
/// whatever the tenant adds. Given the projection above this is belt-and-
/// braces — it is what catches a host that later learns a field it has been
/// echoing is sensitive, without that host having to remember to stop echoing
/// it.
pub fn mask_at_boundary(
    graph: &ContextGraph,
    client_sensitive_fields: &[String],
) -> ProjectedGraph {
    let declared: Vec<String> = enabled_page_actions(graph)
        .iter()
        .flat_map(|action_id| {
            ActionRegistry::try_get(action_id)
                .map(|definition| {
                    definition
                        .sensitive_fields
                        .iter()
                        .map(|field| field.to_string())
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default()
        })
        .collect();
    let rules = resolve_mask_rules(&declared, client_sensitive_fields);
    let masked: MaskedResult = mask_sensitive_fields(
        &project_context_graph(graph),
        &MaskOptions {
            rules,
            preserve: Vec::new(),
        },
    );
    ProjectedGraph {
        value: masked.value,
        notices: masked.notices,
    }
}
