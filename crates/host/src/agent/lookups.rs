//! Finite, authorized read expansion. Models select a typed record query, never a host or SQL.
use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Lookup {
    pub module: String,
    pub collection: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}
impl Lookup {
    fn key(&self) -> &str {
        if self.module == "crm" && self.collection == "contacts" {
            "items"
        } else {
            &self.collection
        }
    }
    pub(super) fn validate(&self, context: &Value) -> Result<(), Response> {
        if self.query.chars().count() > 160
            || self.query.trim() != self.query
            || self.id.is_some() != self.query.is_empty()
        {
            return Err(refused(StatusCode::BAD_GATEWAY, "invalid_agent_lookup"));
        }
        if let Some(id) = &self.id {
            if id.is_empty()
                || id.chars().count() > 128
                || (matches!(
                    self.module.as_str(),
                    "crm" | "support" | "commerce" | "purchasing" | "billing"
                ) && Uuid::parse_str(id).is_err())
            {
                return Err(refused(StatusCode::BAD_GATEWAY, "invalid_agent_lookup"));
            }
        }
        if let Some(id) = &self.id {
            fn contains(v: &Value, id: &str) -> bool {
                match v {
                    Value::String(s) => s == id,
                    Value::Object(o) => o.values().any(|v| contains(v, id)),
                    Value::Array(a) => a.iter().any(|v| contains(v, id)),
                    _ => false,
                }
            }
            if !contains(&context["sources"], id)
                && !contains(&context["requestedReferenceIds"], id)
            {
                return Err(refused(
                    StatusCode::BAD_GATEWAY,
                    "agent_lookup_id_not_grounded",
                ));
            }
        }
        let source = context["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["module"] == self.module && s["available"] == true)
            .ok_or_else(|| refused(StatusCode::FORBIDDEN, "agent_lookup_module_unavailable"))?;
        if matches!(self.key(), "permissions" | "collections")
            || !source["data"][self.key()].is_array()
        {
            return Err(refused(
                StatusCode::FORBIDDEN,
                "agent_lookup_collection_unavailable",
            ));
        }
        Ok(())
    }
    fn normalize(&mut self) {
        if matches!(
            self.module.as_str(),
            "crm" | "support" | "commerce" | "purchasing" | "billing"
        ) {
            if let Some(id) = self.id.as_ref().and_then(|id| Uuid::parse_str(id).ok()) {
                self.id = Some(id.to_string());
            }
        }
    }
    fn path(&self, tenant: &TenantId, company: Option<Uuid>) -> String {
        let filter = if let Some(id) = &self.id {
            format!("id={}", encode(id))
        } else {
            format!("search={}", encode(&self.query))
        };
        let mut path = match self.module.as_str() {
            "crm" => {
                if let Some(id) = &self.id {
                    format!("/crm/records?tenant={tenant}&contact={}", encode(id))
                } else {
                    format!("/contacts?tenant={tenant}&limit=25&{filter}")
                }
            }
            "support" => {
                if let Some(id) = &self.id {
                    format!(
                        "/erp/support/cases/detail?tenant={tenant}&id={}",
                        encode(id)
                    )
                } else {
                    format!("/erp/support/cases?tenant={tenant}&limit=25&{filter}")
                }
            }
            "commerce" | "purchasing" | "billing" => {
                format!("/erp/{}/workspace?tenant={tenant}&{filter}", self.module)
            }
            _ => format!(
                "/erp/{}/workspace?tenant={tenant}&collection={}&limit=25&{filter}",
                self.module,
                encode(&self.collection)
            ),
        };
        if let Some(company) = company.filter(|_| {
            matches!(
                self.module.as_str(),
                "commerce" | "purchasing" | "billing" | "accounting"
            )
        }) {
            path.push_str(&format!("&companyId={company}"))
        }
        path
    }
}
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char)
        } else {
            out.push_str(&format!("%{b:02X}"))
        }
    }
    out
}
pub(super) fn requests(value: &Value) -> Result<Vec<Lookup>, Response> {
    let mut rows: Vec<Lookup> = serde_json::from_value(value.clone())
        .map_err(|_| refused(StatusCode::BAD_GATEWAY, "invalid_agent_lookup"))?;
    if rows.len() > 8 {
        return Err(refused(
            StatusCode::BAD_GATEWAY,
            "agent_lookup_budget_exceeded",
        ));
    }
    for q in &mut rows {
        q.normalize();
    }
    Ok(rows)
}
/// Only business reference fields are looked up; new record IDs remain caller chosen.
pub(super) fn action_requests(action: &ActionInput) -> Vec<Lookup> {
    let Some((module, operation)) = crate::business::tool(&action.tool) else {
        return vec![];
    };
    fn walk(value: &Value, module: &str, operation: &str, out: &mut Vec<Lookup>) {
        if let Some(o) = value.as_object() {
            for (k, v) in o {
                let fresh = matches!(
                    (module, operation, k.as_str()),
                    ("field", "create_job", "jobId")
                        | ("education", "enroll_student", "enrollmentId")
                        | ("education", "admit_applicant", "enrollmentId")
                        | ("accommodation", "room.create", "roomId")
                );
                if !fresh && k != "assigneeId" {
                    let target = match k.as_str() {
                        "partyId" => Some(("organizations", "parties")),
                        "orderId" if module == "billing" => Some(("commerce", "orders")),
                        "id" if module == "crm" && operation == "contact.update" => {
                            Some(("crm", "contacts"))
                        }
                        _ => archava_domain::operator::reference(k, operation, module),
                    };
                    if let (Some((target, collection)), Some(id)) = (
                        target,
                        v.as_str()
                            .filter(|s| !s.is_empty() && s.chars().count() <= 128),
                    ) {
                        let mut lookup = Lookup {
                            module: target.into(),
                            collection: collection.into(),
                            query: String::new(),
                            id: Some(id.to_owned()),
                        };
                        lookup.normalize();
                        if !out.contains(&lookup) {
                            out.push(lookup)
                        }
                    }
                }
                walk(v, module, operation, out)
            }
        } else if let Some(a) = value.as_array() {
            for v in a {
                walk(v, module, operation, out)
            }
        }
    }
    let mut out = Vec::new();
    walk(&action.values, module, operation, &mut out);
    out
}
/// Merge selected records before the provider page limit; unselected rows do not masquerade as search matches.
pub(super) async fn expand(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    context: &mut Value,
    lookups: &[Lookup],
) -> Result<(), Response> {
    expand_bounded(state, headers, tenant, context, lookups, 8, 25).await
}
async fn expand_bounded(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    context: &mut Value,
    lookups: &[Lookup],
    max_queries: usize,
    max_rows: usize,
) -> Result<(), Response> {
    if lookups.len() > max_queries {
        return Err(refused(
            StatusCode::BAD_GATEWAY,
            "agent_lookup_budget_exceeded",
        ));
    }
    let company = context["companyId"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok());
    let mut replacements = std::collections::BTreeMap::<(String, String), Vec<Value>>::new();
    let mut evidence = Vec::new();
    for lookup in lookups {
        lookup.validate(context)?;
        let path = lookup.path(tenant, company);
        let (status, Json(data)) =
            read::invoke(state, headers, axum::http::Method::GET, &path, None).await?;
        if status.is_server_error() {
            return Err((status, Json(data)));
        }
        let mut rows = if status.is_success() {
            match (lookup.module.as_str(), lookup.id.as_deref()) {
                ("crm", Some(_)) => vec![data["contact"].clone()],
                ("support", Some(_)) => vec![data["record"].clone()],
                _ => data[lookup.key()].as_array().cloned().unwrap_or_default(),
            }
        } else {
            vec![]
        };
        if lookup.module == "accounting" && lookup.collection == "journals" && lookup.id.is_some() {
            for row in &mut rows {
                if row["id"] == data["detail"]["record"]["id"] {
                    if let Some(lines) = data["selected"]["lines"].as_array() {
                        row["lines"]=json!(lines.iter().map(|l|json!({"accountId":l["account_id"],"accountRevision":l["account_revision"],"accountCode":l["account_code"],"accountName":l["account_name"],"debitMinor":l["debit_minor"],"creditMinor":l["credit_minor"],"memo":l["memo"]})).collect::<Vec<_>>());
                    }
                }
            }
        }
        let valid: Vec<Value> = rows
            .into_iter()
            .filter(|r| {
                r["id"].is_string()
                    && lookup
                        .id
                        .as_ref()
                        .is_none_or(|id| r["id"].as_str() == Some(id.as_str()))
            })
            .collect();
        let observed = valid.len();
        let entry = replacements
            .entry((lookup.module.clone(), lookup.key().to_string()))
            .or_default();
        for row in valid {
            if !entry.iter().any(|r| r["id"] == row["id"]) {
                entry.push(row)
            }
        }
        evidence.push(json!({"module":lookup.module,"collection":lookup.collection,"query":lookup.query,"id":lookup.id,"path":path,"httpStatus":status.as_u16(),"matchingRowsInSourcePage":observed,"bounded":true,"exactId":lookup.id.is_some(),"note":"A bounded authorized query result is not a workspace-wide aggregate."}));
    }
    for ((module, key), mut rows) in replacements {
        let matching = rows.len();
        rows.truncate(max_rows);
        let source = context["sources"]
            .as_array_mut()
            .and_then(|s| s.iter_mut().find(|s| s["module"] == module))
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_agent_lookup"))?;
        source["data"][&key] = json!(rows);
        source["coverage"][&key] = json!({"included":rows.len(),"rowsInLookupResults":matching,"moreRowsInLookupResults":matching>rows.len(),"limit":max_rows,"selection":"explicit_lookups","wholeCollection":false});
        let protected = lookups
            .iter()
            .filter(|q| q.module == module)
            .filter_map(|q| q.id.as_ref().map(|id| (q.key().to_string(), id.clone())))
            .collect();
        read::bound_source(source, &protected)?;
    }
    context["lookupRequests"] = json!(lookups);
    context["lookupResults"] = json!(evidence);
    Ok(())
}
pub(super) async fn refresh_guards(
    state: &AppState,
    tenant: &TenantId,
    context: &mut Value,
    goal: &Value,
    step: &Value,
) -> Result<(), Response> {
    let mut queries = Vec::new();
    for guard in step["guards"].as_array().into_iter().flatten() {
        if let (Some(module), Some(collection), Some(id)) = (
            guard["module"].as_str(),
            guard["collection"].as_str(),
            guard["id"].as_str(),
        ) {
            let q = Lookup {
                module: module.into(),
                collection: collection.into(),
                query: String::new(),
                id: Some(id.into()),
            };
            if !queries.contains(&q) {
                queries.push(q)
            }
        }
    }
    if let Some(id) = step["observation"]["productId"].as_str() {
        let q = Lookup {
            module: "commerce".into(),
            collection: "products".into(),
            query: String::new(),
            id: Some(id.into()),
        };
        if !queries.contains(&q) {
            queries.push(q)
        }
    }
    if queries.is_empty() {
        queries = saved(&goal["context"])?;
    }
    if !queries.is_empty() {
        context["requestedReferenceIds"] = json!(queries
            .iter()
            .filter_map(|q| q.id.clone())
            .collect::<Vec<_>>());
        expand_bounded(state, &HeaderMap::new(), tenant, context, &queries, 64, 64).await?;
    }
    Ok(())
}
pub(super) fn saved(context: &Value) -> Result<Vec<Lookup>, Response> {
    match context.get("lookupRequests") {
        None => Ok(vec![]),
        Some(v) => requests(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_are_typed_and_new_ids_do_not_request_existing_records() {
        let id = Uuid::new_v4();
        let action = ActionInput {
            tool: "purchasing.document.capture".into(),
            values: json!({"orderId":id,"supplierId":id,"lines":[{"lineId":id,"quantity":2,"unitCost":7000}]}),
        };
        let q = action_requests(&action);
        assert_eq!(q.len(), 2);
        assert!(q.iter().any(|q| q.collection == "orders"));
        assert!(q.iter().any(|q| q.collection == "suppliers"));
        assert!(action_requests(&ActionInput {
            tool: "field.create_job".into(),
            values: json!({"jobId":id})
        })
        .is_empty());
        assert_eq!(encode("A & B/100%"), "A%20%26%20B%2F100%25");
    }
    #[test]
    fn lookup_rejects_arbitrary_network_authority_and_unreadable_collections() {
        assert!(requests(&json!([{"module":"crm","collection":"contacts","query":"Alice","url":"https://example.com"}])).is_err());
        let q = Lookup {
            module: "crm".into(),
            collection: "contacts".into(),
            query: "Alice".into(),
            id: None,
        };
        let mut c = json!({"sources":[{"module":"crm","available":true,"data":{"items":[]}}]});
        assert!(q.validate(&c).is_ok());
        c["sources"][0]["available"] = json!(false);
        assert!(q.validate(&c).is_err());
    }
}
