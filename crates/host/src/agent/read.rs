use super::*;
use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;
/// Finite same-process invocation; neither model nor browser chooses a network host.
pub(super) fn invoke<'a>(
    state: &'a AppState,
    headers: &'a HeaderMap,
    method: Method,
    path: &'a str,
    body: Option<Value>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, Response>> + Send + 'a>> {
    Box::pin(async move {
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let request = request
            .header("content-type", "application/json")
            .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
            .map_err(unavailable)?;
        let response = crate::router(state.clone())
            .oneshot(request)
            .await
            .map_err(unavailable)?;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .map_err(unavailable)?;
        let value = serde_json::from_slice(&bytes).map_err(unavailable)?;
        Ok((status, Json(value)))
    })
}
pub(super) const SOURCE_BYTE_LIMIT: usize = 32_768;
/// Keep complete authorized rows within the existing byte allowance. Omitted
/// tails stay explicit in coverage; exact lookup/selected-company rows are kept.
pub(super) fn bound_source(
    source: &mut Value,
    protected: &std::collections::BTreeSet<(String, String)>,
) -> Result<(), Response> {
    let mut bytes = source["data"].to_string().len();
    let mut costs: std::collections::BTreeMap<String, Vec<usize>> = source["data"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| source["coverage"].get(*key).is_some())
        .filter_map(|(key, value)| {
            value.as_array().map(|rows| {
                (
                    key.clone(),
                    rows.iter().map(|row| row.to_string().len()).collect(),
                )
            })
        })
        .collect();
    let mut limited = source["byteBudget"]["limited"] == true;
    while bytes > SOURCE_BYTE_LIMIT {
        let candidate = costs
            .iter()
            .filter_map(|(key, sizes)| {
                let rows = source["data"][key].as_array()?;
                let index = rows.iter().rposition(|row| {
                    !protected.iter().any(|(collection, id)| {
                        collection == key && row["id"].as_str() == Some(id.as_str())
                    })
                })?;
                Some((
                    sizes.iter().sum::<usize>() + sizes.len().saturating_sub(1),
                    key.clone(),
                    index,
                    sizes[index],
                ))
            })
            .max();
        let Some((_, key, index, row_bytes)) = candidate else {
            // Oversized metadata or required exact records cannot be silently
            // abbreviated and then represented as reviewed facts.
            return Err(refused(StatusCode::BAD_REQUEST, "agent_context_too_large"));
        };
        let sizes = costs.get_mut(&key).expect("selected collection exists");
        let comma = usize::from(sizes.len() > 1);
        sizes.remove(index);
        let included = sizes.len();
        source["data"][&key]
            .as_array_mut()
            .expect("selected rows exist")
            .remove(index);
        bytes -= row_bytes + comma;
        let coverage = &mut source["coverage"][&key];
        coverage["included"] = json!(included);
        if coverage["selection"] == "explicit_lookups" {
            coverage["moreRowsInLookupResults"] = json!(true);
        } else {
            coverage["moreRowsInSourcePage"] = json!(true);
        }
        coverage["wholeCollection"] = json!(false);
        coverage["limitedByBytes"] = json!(true);
        limited = true;
    }
    source["byteBudget"] = json!({"limit":SOURCE_BYTE_LIMIT,"dataBytes":bytes,"limited":limited});
    Ok(())
}
pub(super) async fn context(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    actor: &str,
    modules: &[String],
    company: Option<Uuid>,
) -> Result<Value, Response> {
    let before = fingerprint(state, tenant, actor, modules).await?;
    let mut sources = Vec::new();
    let mut selected_company = None;
    if let Some(company) = company {
        let mut tx = begin(state, tenant, actor).await?;
        let stamp = archava_adapters::erp::company_trade::resolve(
            tx.checked_executor().await.map_err(unavailable)?,
            tenant.as_str(),
            &archava_domain::erp::purchasing::PurchasePrincipal::human(actor),
            company,
        )
        .await
        .map_err(|_| refused(StatusCode::FORBIDDEN, "agent_company_unavailable"))?;
        selected_company = Some(
            json!({"id":stamp.id,"revision":stamp.revision,"code":stamp.code,"legalName":stamp.name,"active":stamp.active}),
        );
        tx.commit().await.map_err(unavailable)?;
    }
    for module in modules {
        let path = match module.as_str() {
            "crm" => format!("/contacts?tenant={tenant}&limit=25"),
            "support" => format!("/erp/support/cases?tenant={tenant}&limit=25"),
            _ => format!("/erp/{module}/workspace?tenant={tenant}"),
        };
        let path = if let Some(company) = company.filter(|_| {
            matches!(
                module.as_str(),
                "commerce" | "purchasing" | "billing" | "accounting"
            )
        }) {
            format!("{path}&companyId={company}")
        } else {
            path
        };
        let (status, Json(mut data)) =
            invoke(state, headers, axum::http::Method::GET, &path, None).await?;
        if status == StatusCode::SERVICE_UNAVAILABLE {
            return Err((status, Json(data)));
        }
        if !status.is_success() {
            sources.push(json!({"module":module,"available":false,"reason":"Not available under the current grants or configuration"}));
            continue;
        }
        if module == "organizations" {
            if let Some(selected) = &selected_company {
                let mut rows = data["companies"].as_array().cloned().unwrap_or_default();
                rows.retain(|r| r["id"] != selected["id"]);
                rows.insert(0, selected.clone());
                data["companies"] = json!(rows);
            }
        }
        // Report precisely which portion of each authorized collection reaches the planner.
        let mut coverage = serde_json::Map::new();
        let page_limit = data["pageLimit"].as_u64().unwrap_or(25) as usize;
        let metadata = data["collections"].as_array().cloned().unwrap_or_default();
        let next = data.get("next").cloned();
        if let Some(object) = data.as_object_mut() {
            for (key, value) in object {
                if matches!(key.as_str(), "permissions" | "collections") {
                    continue;
                }
                if let Some(rows) = value.as_array_mut() {
                    let received = rows.len();
                    let included = received.min(25);
                    let has_more = metadata
                        .iter()
                        .find(|m| m["id"] == key.as_str())
                        .and_then(|m| m["hasMore"].as_bool())
                        .or_else(|| next.as_ref().map(|n| !n.is_null()))
                        .unwrap_or(received >= page_limit);
                    coverage.insert(key.clone(),json!({"included":included,"rowsInSourcePage":received,"sourcePageLimit":page_limit,"moreRowsInSourcePage":received>25,"sourceHasMore":has_more,"wholeCollection":received<=25&&!has_more}));
                    rows.truncate(25);
                }
            }
        }
        let mut source = json!({"module":module,"available":true,"path":path,"bounded":true,"coverage":coverage,"data":data});
        let protected = selected_company
            .as_ref()
            .and_then(|c| c["id"].as_str())
            .map(|id| std::collections::BTreeSet::from([("companies".into(), id.into())]))
            .unwrap_or_default();
        bound_source(&mut source, &protected)?;
        sources.push(source);
    }
    let mut tx = begin(state, tenant, actor).await?;
    let fingerprint = store::access_fingerprint(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        modules,
    )
    .await
    .map_err(unavailable)?;
    let observed: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
        .fetch_one(tx.checked_executor().await.map_err(unavailable)?)
        .await
        .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    if fingerprint != before {
        return Err(refused(
            StatusCode::CONFLICT,
            "agent_context_access_changed",
        ));
    }
    Ok(
        json!({"sources":sources,"accessHash":fingerprint,"observedAt":observed,"bounded":true,"companyId":company,"note":"These are bounded authorized records in the selected company or legacy scope, not complete workspace totals."}),
    )
}
pub(super) async fn fingerprint(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    modules: &[String],
) -> Result<String, Response> {
    let mut tx = begin(state, tenant, actor).await?;
    let value = store::access_fingerprint(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        modules,
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(value)
}
pub(super) fn rows<'a>(context: &'a Value, module: &str, collection: &str) -> Vec<&'a Value> {
    context["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["module"] == module && s["available"] == true)
        .map(|s| {
            s["data"][collection]
                .as_array()
                .into_iter()
                .flatten()
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn byte_bounding_retains_complete_rows_and_reports_partial_unicode_collections() {
        let rows = (0..25)
            .map(|n| json!({"id":format!("record-{n}"),"revision":1,"name":"界".repeat(1000)}))
            .collect::<Vec<_>>();
        let mut source = json!({"data":{"organizations":rows,"permissions":["organizations.read"]},"coverage":{"organizations":{"included":25,"rowsInSourcePage":25,"sourcePageLimit":25,"sourceHasMore":false,"wholeCollection":true}}});
        bound_source(&mut source, &Default::default()).unwrap();
        assert!(source["data"].to_string().len() <= SOURCE_BYTE_LIMIT);
        let rows = source["data"]["organizations"].as_array().unwrap();
        assert!(!rows.is_empty() && rows.len() < 25);
        assert!(rows
            .iter()
            .all(|r| r["name"].as_str().unwrap().chars().count() == 1000));
        assert_eq!(source["coverage"]["organizations"]["included"], rows.len());
        assert_eq!(
            source["coverage"]["organizations"]["wholeCollection"],
            false
        );
        assert_eq!(source["coverage"]["organizations"]["rowsInSourcePage"], 25);
        assert_eq!(source["coverage"]["organizations"]["limitedByBytes"], true);
        assert_eq!(source["data"]["permissions"], json!(["organizations.read"]));
    }
    #[test]
    fn exact_rows_survive_competing_pages_and_unrepresentable_required_rows_refuse() {
        let mut source = json!({"data":{"parties":[{"id":"needed","revision":7,"name":"Kept exact record"}],"organizations":(0..25).map(|n|json!({"id":n.to_string(),"description":"x".repeat(3000)})).collect::<Vec<_>>()},"coverage":{"parties":{"included":1,"wholeCollection":false,"selection":"explicit_lookups"},"organizations":{"included":25,"wholeCollection":true}}});
        let protected = std::collections::BTreeSet::from([("parties".into(), "needed".into())]);
        bound_source(&mut source, &protected).unwrap();
        assert_eq!(source["data"]["parties"][0]["revision"], 7);
        assert_eq!(source["coverage"]["parties"]["included"], 1);
        assert_eq!(
            source["coverage"]["parties"]["selection"],
            "explicit_lookups"
        );
        source["data"]["parties"][0]["name"] = json!("x".repeat(SOURCE_BYTE_LIMIT));
        assert!(bound_source(&mut source, &protected).is_err());
    }
    #[test]
    fn small_complete_collection_keeps_its_coverage_and_metadata_only_overflow_refuses() {
        let coverage = json!({"included":1,"wholeCollection":true});
        let mut source = json!({"data":{"items":[{"id":"a"}]},"coverage":{"items":coverage}});
        bound_source(&mut source, &Default::default()).unwrap();
        assert_eq!(source["coverage"]["items"], coverage);
        assert_eq!(source["byteBudget"]["limited"], false);
        source["data"]["notice"] = json!("x".repeat(SOURCE_BYTE_LIMIT));
        assert!(bound_source(&mut source, &Default::default()).is_err());
    }
}
