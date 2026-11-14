//! Aggregate lookups: a count or a sum over a whole filtered collection.
//!
//! A page lookup returns bounded rows and can never answer "how much in
//! total". An aggregate lookup can, for the closed list of collections,
//! amount fields, dimensions and filters the module registry declares
//! (`agent.aggregates` in `config/modules.v1.json`). The model names an entry
//! of that list; it never supplies SQL, a column or a permission.
//!
//! Each request is decided in this order, and the first failure is answered
//! to the planner as a refusal with its own reason — never as partial data:
//!
//! 1. the module is one of the turn's available sources;
//! 2. the names are in the allowlist (registry and adapter SQL table);
//! 3. the owner's AI sharing selection permits this shape (module, data
//!    categories of the measure and of the group labels, company scope);
//! 4. the adapter computes it inside the module's tenant transaction, under
//!    the permission and row scope of the collection's page read.
//!
//! The result is evidence: the filter as applied, the number of rows covered,
//! the time of the read and the folded remainder, so an answer can state
//! exactly what it covers.
use super::{disclosure::Permit, lookups::Lookup, *};
use archava_adapters::erp::aggregates::{self as store, AggregateError, Outcome};
use archava_domain::erp::{purchasing::PurchasePrincipal, trade_money};
use std::collections::BTreeMap;

/// The aggregate member of a lookup, as the model writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Aggregate {
    /// `count` or `sum`.
    pub measure: String,
    /// The amount field of a sum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub filters: BTreeMap<String, String>,
}
impl Aggregate {
    /// Structural limits only. Whether the names are allowed is answered to
    /// the planner as a refusal, not as a malformed turn.
    pub(super) fn well_formed(&self) -> bool {
        let short = |s: &str| !s.is_empty() && s.len() <= 64 && s.trim() == s;
        short(&self.measure)
            && self.field.as_deref().is_none_or(short)
            && self.group_by.as_deref().is_none_or(short)
            && self.filters.len() <= 8
            && self.filters.iter().all(|(k, v)| short(k) && short(v))
    }
    /// Structured output often fills an unused optional member with an empty
    /// string. That means "absent", for the optional members only.
    pub(super) fn normalize(&mut self) {
        if self.field.as_deref() == Some("") {
            self.field = None;
        }
        if self.group_by.as_deref() == Some("") {
            self.group_by = None;
        }
        self.filters.retain(|_, value| !value.is_empty());
    }
}

fn message(reason: &str) -> &'static str {
    match reason {
        "aggregate_module_unavailable" => "This business area is not among the sources available to this turn under the current access and configuration.",
        "aggregate_collection_not_listed" => "No aggregate is offered for this module and collection. Use one listed in readExpansion.aggregates.",
        "aggregate_measure_not_listed" => "The measure must be count, or sum with one of the collection's listed sumFields.",
        "aggregate_dimension_not_listed" => "This collection cannot be grouped by that dimension. Use one of its listed groupBy values or none.",
        "aggregate_filter_not_listed" => "This collection does not offer that filter. Use only its listed filters, each at most once.",
        "aggregate_filter_invalid" => "A filter value is not valid: dates are YYYY-MM-DD with dateFrom not after dateTo, the state must be one of the listed values and companyId an observed company id.",
        "aggregate_module_not_shared" => "The workspace owner's AI data sharing selection does not include this business area.",
        "aggregate_data_class_not_shared" => "The workspace owner's AI data sharing selection does not include a data category this aggregate would disclose. Choose a measure or dimension that needs only shared categories.",
        "aggregate_company_out_of_scope" => "This turn is scoped to one company; an aggregate cannot name another.",
        "aggregate_company_not_shared" => "That company is not among the companies approved for AI data sharing.",
        "aggregate_company_not_readable" => "That company is not readable under the current access.",
        "aggregate_not_permitted" => "The current user does not hold the read permission of this collection.",
        "aggregate_too_many_currencies" => "The filtered rows span more currencies than one aggregate reports. Narrow the filter to one company.",
        "aggregate_timed_out" => "The aggregate did not finish within its time limit. Narrow the filter. No figure was produced.",
        _ => "The aggregate was refused.",
    }
}

/// A refusal the planner sees. It echoes only registry vocabulary, never a
/// value the model or a record supplied.
fn refusal(lookup: &Lookup, reason: &str, class: Option<&str>) -> Value {
    let mut out =
        json!({"kind":"aggregate","refused":true,"reason":reason,"message":message(reason)});
    if let Some(module) = archava_domain::modules::get(&lookup.module) {
        out["module"] = json!(module.id);
        if let Some(collection) = module.collections.iter().find(|c| **c == lookup.collection) {
            out["collection"] = json!(collection);
        }
    }
    if let Some(class) = class {
        out["dataClass"] = json!(class);
    }
    out
}

fn money(currency: &str, amount: i128) -> (Value, Value) {
    match i64::try_from(amount) {
        Ok(value) => (
            json!(value),
            json!(trade_money::format_trade(currency, value)),
        ),
        // Beyond the stored integer: keep every digit, as text.
        Err(_) => (
            json!(amount.to_string()),
            u128::try_from(amount)
                .ok()
                .and_then(|v| trade_money::format_trade_digits(currency, &v.to_string()))
                .map_or(Value::Null, Value::String),
        ),
    }
}

/// The sentence an answer can quote for what the aggregate covers.
fn coverage(lookup: &Lookup, request: &Aggregate, outcome: &Outcome, turn_company: bool) -> String {
    let filter = &outcome.filter;
    let label = archava_domain::modules::get(&lookup.module).map_or("", |m| m.label);
    let measure = match &request.field {
        Some(field) => format!("sum of {field}"),
        None => "count".into(),
    };
    let by = request
        .group_by
        .as_ref()
        .map(|d| format!(" by {d}"))
        .unwrap_or_default();
    let mut conditions = Vec::new();
    if let Some((name, value)) = &filter.state {
        conditions.push(format!("{name} {value}"));
    }
    match (&filter.date_from, &filter.date_to) {
        (Some(from), Some(to)) => conditions.push(format!(
            "{} {from}..{to} (UTC calendar dates, inclusive)",
            filter.date
        )),
        (Some(from), None) => conditions.push(format!(
            "{} on or after {from} (UTC calendar date)",
            filter.date
        )),
        (None, Some(to)) => conditions.push(format!(
            "{} on or before {to} (UTC calendar date)",
            filter.date
        )),
        (None, None) => {}
    }
    let conditions = if conditions.is_empty() {
        "no state or date filter".into()
    } else {
        conditions.join(", ")
    };
    let companies = filter.scope.companies.as_ref().map(Vec::len);
    let mut scope = match (filter.company.is_some() || turn_company, companies) {
        (true, _) => "records of the selected company".to_string(),
        (false, Some(0)) => "no company records".to_string(),
        (false, Some(1)) => "records of the 1 company approved for AI sharing".to_string(),
        (false, Some(n)) => format!("records of the {n} companies approved for AI sharing"),
        (false, None) => "records of every readable company".to_string(),
    };
    if filter.scope.unassigned {
        scope.push_str(" and records without a company");
    }
    if !filter.every_owner {
        scope.push_str("; only records owned by the current user");
    }
    format!(
        "{label} {}: {measure}{by} over all {} matching rows ({conditions}); scope: {scope}; read at {}.",
        lookup.collection, outcome.rows, outcome.observed_at
    )
}

fn render(lookup: &Lookup, request: &Aggregate, outcome: &Outcome, turn_company: bool) -> Value {
    let filter = &outcome.filter;
    let sum = request.field.is_some();
    let sections = outcome
        .sections
        .iter()
        .map(|section| {
            let mut out = json!({"rows":section.rows});
            let currency = section.currency.as_deref();
            if let (Some(currency), Some(total)) = (currency, section.total) {
                let (amount, formatted) = money(currency, total);
                out["currency"] = json!(currency);
                out["unit"] = json!(super::units::label(currency));
                out["total"] = amount;
                out["totalFormatted"] = formatted;
            }
            if request.group_by.is_some() {
                out["groupCount"] = json!(section.group_count);
                out["groups"] = json!(section
                    .groups
                    .iter()
                    .map(|group| {
                        let mut row = json!({"label":group.label.as_deref().map(archava_context::decision::turn::credential_text),"rows":group.rows});
                        if let (Some(currency), Some(total)) = (currency, group.total) {
                            let (amount, formatted) = money(currency, total);
                            row["amount"] = amount;
                            row["formatted"] = formatted;
                        }
                        row
                    })
                    .collect::<Vec<_>>());
                out["other"] = match &section.other {
                    None => Value::Null,
                    Some(fold) => {
                        let mut row = json!({"groups":fold.groups,"rows":fold.rows});
                        if let (Some(currency), Some(total)) = (currency, fold.total) {
                            let (amount, formatted) = money(currency, total);
                            row["amount"] = amount;
                            row["formatted"] = formatted;
                        }
                        row
                    }
                };
            }
            out
        })
        .collect::<Vec<_>>();
    let mut applied = json!({"dateField":filter.date,"dateFrom":filter.date_from,"dateTo":filter.date_to,"calendar":"UTC calendar dates, both bounds inclusive","companyId":filter.company});
    if let Some((name, value)) = &filter.state {
        applied[*name] = json!(value);
    }
    json!({
        "kind":"aggregate",
        "module":lookup.module,
        "collection":lookup.collection,
        "measure":{"kind":if sum {"sum"} else {"count"},"field":request.field},
        "groupBy":request.group_by,
        "filter":applied,
        "scope":{
            "companies":if filter.company.is_some() || turn_company {json!("selected company")} else {json!({"approvedForAiSharing":filter.scope.companies.as_ref().map(Vec::len)})},
            "recordsWithoutCompany":filter.scope.unassigned,
            "ownRecordsOnly":!filter.every_owner,
        },
        "rowsCovered":outcome.rows,
        "complete":true,
        "incompleteReason":null,
        "observedAt":outcome.observed_at,
        "groupLimit":store::GROUP_LIMIT,
        "sections":sections,
        "coverage":coverage(lookup, request, outcome, turn_company),
        "note":if sum {"The whole filtered set was aggregated in the database. Amounts of different currencies are separate sections and must never be added together; each section states its unit."} else {"The whole filtered set was counted in the database."},
    })
}

fn tenant_ledger(module: &str) -> Option<(&'static str, &'static str)> {
    // The namespace and ledger each collection's page read opens.
    Some(match module {
        "billing" => ("archava_billing", "billing"),
        "commerce" => ("archava_commerce", "commerce"),
        "purchasing" => ("archava_purchasing", "purchasing"),
        _ => return None,
    })
}

async fn one(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    permit: &Permit,
    context: &Value,
    lookup: &Lookup,
    request: &Aggregate,
) -> Result<Value, Response> {
    let available = context["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|s| s["module"] == lookup.module && s["available"] == true);
    if !available {
        return Ok(refusal(lookup, "aggregate_module_unavailable", None));
    }
    let sum = match (request.measure.as_str(), request.field.as_deref()) {
        ("count", None) => None,
        ("sum", Some(field)) => Some(field),
        _ => {
            // An unknown collection outranks an unknown measure.
            let reason = if archava_domain::modules::aggregate(&lookup.module, &lookup.collection)
                .is_none()
            {
                "aggregate_collection_not_listed"
            } else {
                "aggregate_measure_not_listed"
            };
            return Ok(refusal(lookup, reason, None));
        }
    };
    let turn_company = context["companyId"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok());
    let filters = request
        .filters
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let query = store::Request {
        module: &lookup.module,
        collection: &lookup.collection,
        sum,
        group_by: request.group_by.as_deref(),
        filters: &filters,
        scope: permit.aggregate_scope(turn_company),
    };
    if let Err(refused) = store::check(&query) {
        return Ok(refusal(lookup, refused.code(), None));
    }
    // The allowlist passed, so the registry entry exists and names the class
    // of this dimension's labels.
    let label_class = request.group_by.as_deref().and_then(|dimension| {
        archava_domain::modules::aggregate(&lookup.module, &lookup.collection)?
            .dimensions
            .iter()
            .find(|d| d.id == dimension)
            .map(|d| d.class)
    });
    if request.group_by.is_some() && label_class.is_none() {
        return Ok(refusal(lookup, "aggregate_dimension_not_listed", None));
    }
    if let Some((reason, class)) =
        permit.aggregate_refusal(&lookup.module, sum.is_some(), label_class)
    {
        return Ok(refusal(lookup, reason, class));
    }
    if let Some(company) = request
        .filters
        .get("companyId")
        .and_then(|id| Uuid::parse_str(id).ok())
    {
        if turn_company.is_some_and(|selected| selected != company) {
            return Ok(refusal(lookup, "aggregate_company_out_of_scope", None));
        }
        if query
            .scope
            .companies
            .as_ref()
            .is_some_and(|shared| !shared.contains(&company))
        {
            return Ok(refusal(lookup, "aggregate_company_not_shared", None));
        }
    }
    let Some((namespace, ledger)) = tenant_ledger(&lookup.module) else {
        return Ok(refusal(lookup, "aggregate_collection_not_listed", None));
    };
    let mut tx = TenantTransaction::begin(state.pool(), namespace, ledger, tenant.clone())
        .await
        .map_err(unavailable)?;
    // Disabled in the workspace configuration: the page read answers 403.
    match crate::own::require_erp_module(&mut tx, namespace).await {
        Ok(()) => {}
        Err(refused) => {
            let (status, _) = refused.response();
            if status.is_server_error() {
                return Err(unavailable("aggregate module check failed"));
            }
            return Ok(refusal(lookup, "aggregate_module_unavailable", None));
        }
    }
    let outcome = store::run(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        &PurchasePrincipal::human(actor),
        &query,
    )
    .await;
    match outcome {
        Ok(outcome) => {
            tx.commit().await.map_err(unavailable)?;
            Ok(render(lookup, request, &outcome, turn_company.is_some()))
        }
        Err(AggregateError::Refused(refused)) => Ok(refusal(lookup, refused.code(), None)),
        Err(AggregateError::Storage(error)) => Err(unavailable(error)),
    }
}

/// Runs every aggregate lookup of the turn and records the results as
/// evidence. Results are recomputed on each planning round, like page reads.
pub(super) async fn expand(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    actor: &str,
    permit: &Permit,
    context: &mut Value,
    lookups: &[Lookup],
) -> Result<(), Response> {
    if !lookups.iter().any(|q| q.aggregate.is_some()) {
        return Ok(());
    }
    // The aggregate is read as the signed-in person, like the page reads the
    // same turn makes through their handlers.
    let session = authenticate(state, headers, false)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    if session.principal_id != actor {
        return Err(refused(StatusCode::FORBIDDEN, "agent_goal_scope_refused"));
    }
    let mut results = Vec::new();
    for lookup in lookups {
        if let Some(request) = &lookup.aggregate {
            results.push(one(state, tenant, actor, permit, context, lookup, request).await?);
        }
    }
    context["aggregateResults"] = json!(results);
    Ok(())
}

/// What a provider request carries for one aggregate result. A refusal is
/// vocabulary only. A result is re-checked against the current selection and
/// loses its company identifier unless record references are shared.
pub(super) fn provider_view(permit: &Permit, result: &Value) -> Option<Value> {
    let module = result["module"].as_str();
    if result["refused"] == true {
        return Some(result.clone());
    }
    let module = module?;
    let collection = result["collection"].as_str()?;
    let label_class = match result["groupBy"].as_str() {
        None => None,
        Some(dimension) => Some(
            archava_domain::modules::aggregate(module, collection)?
                .dimensions
                .iter()
                .find(|d| d.id == dimension)?
                .class,
        ),
    };
    if permit
        .aggregate_refusal(module, result["measure"]["kind"] == "sum", label_class)
        .is_some()
    {
        return None;
    }
    let mut out = result.clone();
    if !permit.shares_references() {
        if let Some(filter) = out["filter"].as_object_mut() {
            let selected = filter.remove("companyId").is_some_and(|id| !id.is_null());
            filter.insert("companyFilterApplied".into(), json!(selected));
        }
    }
    Some(archava_context::mask::mask_sensitive_fields(&out, &Default::default()).value)
}

/// The aggregates the planner may request this round: allowlisted, on an
/// available shared module, and of a shape the sharing selection permits.
pub(super) fn offers(permit: &Permit, outbound: &Value) -> Vec<Value> {
    let mut offers = Vec::new();
    for source in outbound["sources"].as_array().into_iter().flatten() {
        if source["available"] != true {
            continue;
        }
        let Some(module) = source["module"]
            .as_str()
            .and_then(archava_domain::modules::get)
        else {
            continue;
        };
        for aggregate in module.aggregates {
            if permit.aggregate_refusal(module.id, false, None).is_some() {
                continue;
            }
            let sums: &[&str] = if permit.aggregate_refusal(module.id, true, None).is_none() {
                aggregate.sums
            } else {
                &[]
            };
            let group_by = aggregate
                .dimensions
                .iter()
                .filter(|d| {
                    permit
                        .aggregate_refusal(module.id, false, Some(d.class))
                        .is_none()
                })
                .map(|d| d.id)
                .collect::<Vec<_>>();
            let mut offer = json!({"module":module.id,"collection":aggregate.collection,"measures":["count"],"sumFields":sums,"groupBy":group_by,"filters":aggregate.filters,"dateField":aggregate.date});
            if !sums.is_empty() {
                offer["measures"] = json!(["count", "sum"]);
            }
            if let Some((name, values)) = store::states(module.id, aggregate.collection) {
                offer["filterValues"] = json!({name:values});
            }
            offers.push(offer);
        }
    }
    offers
}

/// Coverage sentences of the aggregates behind an answer, for the reply.
pub(super) fn evidence(context: &Value) -> Vec<String> {
    context["aggregateResults"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["coverage"].as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use archava_adapters::erp::aggregates::{AppliedFilter, CompanyScope, Fold, Group, Section};

    fn lookup(module: &str, collection: &str, aggregate: Aggregate) -> Lookup {
        Lookup {
            module: module.into(),
            collection: collection.into(),
            query: String::new(),
            id: None,
            aggregate: Some(aggregate),
        }
    }
    fn sum_by_customer() -> Aggregate {
        Aggregate {
            measure: "sum".into(),
            field: Some("outstanding".into()),
            group_by: Some("customer".into()),
            filters: BTreeMap::from([
                ("status".to_string(), "issued".to_string()),
                ("dateFrom".to_string(), "2026-11-01".to_string()),
                ("dateTo".to_string(), "2026-11-30".to_string()),
            ]),
        }
    }
    fn outcome() -> Outcome {
        Outcome {
            filter: AppliedFilter {
                state: Some(("status", "issued".into())),
                date: "issued",
                date_from: Some("2026-11-01".into()),
                date_to: Some("2026-11-30".into()),
                company: None,
                every_owner: true,
                scope: CompanyScope {
                    companies: Some(vec![Uuid::nil(), Uuid::max()]),
                    unassigned: true,
                },
            },
            rows: 237,
            observed_at: "2026-11-14T03:04:05Z".into(),
            sections: vec![
                Section {
                    currency: Some("IDR".into()),
                    rows: 200,
                    total: Some(4_500_000),
                    group_count: 3,
                    groups: vec![
                        Group {
                            label: Some("PT Sinar".into()),
                            rows: 150,
                            total: Some(4_000_000),
                        },
                        Group {
                            label: Some("CV Maju".into()),
                            rows: 30,
                            total: Some(400_000),
                        },
                    ],
                    other: Some(Fold {
                        groups: 1,
                        rows: 20,
                        total: Some(100_000),
                    }),
                },
                Section {
                    currency: Some("USD".into()),
                    rows: 37,
                    total: Some(1_999),
                    group_count: 1,
                    groups: vec![Group {
                        label: Some("Acme Inc".into()),
                        rows: 37,
                        total: Some(1_999),
                    }],
                    other: None,
                },
            ],
        }
    }

    #[test]
    fn a_result_states_its_filter_coverage_units_and_fold_and_never_mixes_currencies() {
        let request = sum_by_customer();
        let lookup = lookup("billing", "invoices", request.clone());
        let value = render(&lookup, &request, &outcome(), false);
        assert_eq!(value["rowsCovered"], 237);
        assert_eq!(value["complete"], true);
        assert_eq!(value["observedAt"], "2026-11-14T03:04:05Z");
        assert_eq!(
            value["filter"],
            json!({"status":"issued","dateField":"issued","dateFrom":"2026-11-01","dateTo":"2026-11-30","calendar":"UTC calendar dates, both bounds inclusive","companyId":null})
        );
        assert_eq!(
            value["coverage"],
            "Billing invoices: sum of outstanding by customer over all 237 matching rows (status issued, issued 2026-11-01..2026-11-30 (UTC calendar dates, inclusive)); scope: records of the 2 companies approved for AI sharing and records without a company; read at 2026-11-14T03:04:05Z."
        );
        let sections = value["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0]["unit"], "whole IDR");
        assert_eq!(sections[0]["total"], 4_500_000);
        assert_eq!(sections[0]["totalFormatted"], "Rp 4.500.000");
        assert_eq!(
            sections[0]["other"],
            json!({"groups":1,"rows":20,"amount":100000,"formatted":"Rp 100.000"})
        );
        assert_eq!(sections[1]["unit"], "minor units of USD");
        assert_eq!(sections[1]["total"], 1_999);
        assert_eq!(sections[1]["totalFormatted"], "USD 19.99");
        assert_eq!(sections[1]["other"], Value::Null);
        assert!(
            value.get("total").is_none(),
            "there is no cross-currency total"
        );
    }

    #[test]
    fn a_count_has_no_currency_unit_or_amount() {
        let request = Aggregate {
            measure: "count".into(),
            field: None,
            group_by: Some("state".into()),
            filters: BTreeMap::new(),
        };
        let lookup = lookup("commerce", "orders", request.clone());
        let mut counted = outcome();
        counted.filter.state = None;
        counted.filter.date = "created";
        counted.filter.date_from = None;
        counted.filter.date_to = None;
        counted.filter.every_owner = false;
        counted.filter.scope = CompanyScope {
            companies: Some(vec![Uuid::nil()]),
            unassigned: false,
        };
        counted.sections = vec![Section {
            currency: None,
            rows: 237,
            total: None,
            group_count: 2,
            groups: vec![
                Group {
                    label: Some("draft".into()),
                    rows: 200,
                    total: None,
                },
                Group {
                    label: Some("fulfilled".into()),
                    rows: 37,
                    total: None,
                },
            ],
            other: None,
        }];
        let value = render(&lookup, &request, &counted, true);
        let section = &value["sections"][0];
        assert_eq!(
            section,
            &json!({"rows":237,"groupCount":2,"groups":[{"label":"draft","rows":200},{"label":"fulfilled","rows":37}],"other":null})
        );
        assert_eq!(
            value["coverage"],
            "Commerce orders: count by state over all 237 matching rows (no state or date filter); scope: records of the selected company; only records owned by the current user; read at 2026-11-14T03:04:05Z."
        );
    }

    #[test]
    fn a_refusal_echoes_registry_vocabulary_only() {
        let mut request = sum_by_customer();
        request.field = Some("Budi Santoso salary".into());
        let refused = refusal(
            &lookup("billing", "Budi Santoso", request),
            "aggregate_collection_not_listed",
            None,
        );
        assert_eq!(
            refused,
            json!({"kind":"aggregate","refused":true,"reason":"aggregate_collection_not_listed","message":message("aggregate_collection_not_listed"),"module":"billing"})
        );
        assert!(!refused.to_string().contains("Budi"));
        let unknown = refusal(
            &lookup("payroll-secrets", "x", sum_by_customer()),
            "aggregate_module_unavailable",
            None,
        );
        assert!(unknown.get("module").is_none());
    }

    #[test]
    fn empty_optional_members_mean_absent_and_oversized_requests_are_malformed() {
        let mut request: Aggregate = serde_json::from_value(
            json!({"measure":"count","field":"","groupBy":"","filters":{"status":"","dateFrom":"2026-11-01"}}),
        )
        .unwrap();
        request.normalize();
        assert_eq!(
            request,
            Aggregate {
                measure: "count".into(),
                field: None,
                group_by: None,
                filters: BTreeMap::from([("dateFrom".to_string(), "2026-11-01".to_string())]),
            }
        );
        assert!(request.well_formed());
        assert!(
            serde_json::from_value::<Aggregate>(json!({"measure":"count","sql":"select 1"}))
                .is_err()
        );
        request.measure = "x".repeat(65);
        assert!(!request.well_formed());
    }

    #[test]
    fn amounts_beyond_the_stored_integer_keep_every_digit() {
        let (amount, formatted) = money("USD", i128::from(i64::MAX) + 1);
        assert_eq!(amount, "9223372036854775808");
        assert_eq!(formatted, "USD 92233720368547758.08");
        let (amount, formatted) = money("IDR", -150_000);
        assert_eq!(amount, -150_000);
        assert_eq!(formatted, "−Rp 150.000");
    }
}
