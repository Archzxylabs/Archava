use super::*;
use axum::http::Method;
fn provider_unavailable(error: archava_providers::agent::PlannerUnavailable) -> Response {
    tracing::warn!(
        failure = error.code(),
        upstream_status = error.upstream_status(),
        "business planner unavailable"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(
            json!({"reason":"agent_planner_unavailable","message":"AI planning is unavailable. Check the original request status, then retry or use the manual forms.","failure":error.code(),"upstreamStatus":error.upstream_status()}),
        ),
    )
}
#[derive(Debug)]
pub(super) struct Proposal {
    pub modules: Vec<String>,
    pub context: Value,
    pub plan: Value,
    pub planner: Value,
}
pub(super) fn route(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut found = Vec::new();
    // Keywords and their match order live in the module registry
    // (`config/modules.v1.json`), one descriptor per installed module.
    for module in archava_domain::modules::routing() {
        if module.keywords.iter().any(|w| lower.contains(w)) {
            found.push(module.id.into());
        }
    }
    found.truncate(6);
    found
}
pub(super) fn asks_memory(s: &str) -> bool {
    (s.contains("preferensi") || s.contains("preference") || s.contains("aturan persetujuan"))
        && (s.contains("apa") || s.contains("what") || s.contains("recall"))
}
pub(super) fn asks_report(s: &str) -> bool {
    [
        "hasil misi",
        "laporkan hasil",
        "status misi",
        "goal status",
        "mission status",
        "work have you completed",
        "pekerjaan yang sudah kamu",
        "hasil yang sudah tersimpan",
    ]
    .iter()
    .any(|p| s.contains(p))
}
pub(super) fn asks_control(s: &str) -> bool {
    let change = [
        "prioritaskan",
        "different",
        "berbeda",
        "instead",
        "rencana baru",
    ]
    .iter()
    .any(|w| s.contains(w));
    !change
        && [
            "batalkan rencana",
            "cancel goal",
            "cancel mission",
            "pause goal",
            "pause mission",
            "tunda misi",
            "hentikan sementara",
            "resume goal",
            "resume mission",
            "lanjutkan misi",
        ]
        .iter()
        .any(|w| s.contains(w))
}
fn route_schema(modules: &[String]) -> Value {
    json!({"type":"object","properties":{"modules":{"type":"array","items":{"type":"string","enum":modules},"maxItems":6}},"required":["modules"],"additionalProperties":false})
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn propose(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    actor: &str,
    utterance: &str,
    _history: &[Value],
    _preferences: &[Value],
    parent: Option<&Value>,
    action: Option<&ActionInput>,
    company_id: Option<Uuid>,
    request: Uuid,
) -> Result<Proposal, Response> {
    let action_company = action
        .and_then(|a| a.values["companyId"].as_str())
        .and_then(|v| Uuid::parse_str(v).ok());
    let parent_company = parent
        .and_then(|p| p["context"]["companyId"].as_str())
        .and_then(|v| Uuid::parse_str(v).ok());
    let company = company_id.or(action_company).or(parent_company);
    if company_id.zip(action_company).is_some_and(|(a, b)| a != b)
        || parent_company.zip(company).is_some_and(|(a, b)| a != b)
    {
        return Err(refused(
            StatusCode::CONFLICT,
            "agent_company_scope_mismatch",
        ));
    }
    let mut modules = action
        .and_then(|a| crate::business::tool(&a.tool).map(|(m, _)| vec![m.to_string()]))
        .unwrap_or_else(|| route(utterance));
    if let Some(parent) = parent {
        for module in super::modules(&parent["modules"]) {
            if !modules.contains(&module) {
                modules.push(module);
            }
        }
    }
    let mut lookups = if let Some(parent) = parent {
        super::lookups::saved(&parent["context"])?
    } else {
        Vec::new()
    };
    if let Some(action) = action {
        for lookup in super::lookups::action_requests(action) {
            if !lookups.contains(&lookup) {
                lookups.push(lookup)
            }
        }
    }
    for lookup in &lookups {
        if !modules.contains(&lookup.module) {
            modules.push(lookup.module.clone())
        }
    }
    let lower = utterance.to_lowercase();
    if modules.iter().any(|m| m == "commerce" || m == "purchasing")
        && ["stock", "stok", "replenish"]
            .iter()
            .any(|w| lower.contains(w))
    {
        for m in ["commerce", "purchasing"] {
            if !modules.iter().any(|s| s == m) {
                modules.push(m.into());
            }
        }
    }
    let sharing = if action.is_none() && state.agent_config().planner.is_some() {
        disclosure::load(state, tenant, actor).await?
    } else {
        None
    };
    let provider = state
        .agent_config()
        .planner
        .as_ref()
        .filter(|_| action.is_none() && sharing.as_ref().is_some_and(|p| p.can_plan(company)));
    if provider.is_some() {
        let mut tx = begin(state, tenant, actor).await?;
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("agent-budget:{}:{actor}", tenant.as_str()))
            .execute(&mut *conn)
            .await
            .map_err(unavailable)?;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_model_attempts WHERE tenant_id=$1 AND principal_id=$2 AND created_at>clock_timestamp()-interval '1 minute'").bind(tenant.as_str()).bind(actor).fetch_one(&mut *conn).await.map_err(unavailable)?;
        if count >= 6 {
            return Err(refused(
                StatusCode::TOO_MANY_REQUESTS,
                "agent_model_budget_exceeded",
            ));
        }
        let inserted=sqlx::query("INSERT INTO archava_owner.agent_model_attempts(tenant_id,principal_id,request_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(tenant.as_str()).bind(actor).bind(request).execute(&mut *conn).await.map_err(unavailable)?.rows_affected();
        if inserted == 0 {
            return Err(refused(
                StatusCode::CONFLICT,
                "agent_planning_attempt_needs_recovery",
            ));
        }
        tx.commit().await.map_err(unavailable)?;
    }

    let mut routing_calls = 0usize;
    if modules.is_empty() {
        if let Some(provider) = provider {
            routing_calls = 1;
            let permit = sharing
                .as_ref()
                .expect("provider requires a sharing permit");
            disclosure::recheck(
                state,
                headers,
                tenant,
                actor,
                permit,
                &modules,
                &Value::Null,
            )
            .await?;
            let allowed = permit.modules();
            let routed=provider.generate(&json!({"phase":"route","goal":archava_context::decision::turn::credential_text(utterance),"history":[],"installedModules":allowed.iter().filter_map(|id|archava_domain::operator::module(id)).map(|m|json!({"id":m["id"],"name":m["label"]})).collect::<Vec<_>>()}),&route_schema(&allowed)).await.map_err(provider_unavailable)?;
            if super::modules(&routed["modules"])
                .iter()
                .any(|id| !allowed.contains(id))
            {
                return Err(refused(
                    StatusCode::FORBIDDEN,
                    "agent_disclosure_module_unavailable",
                ));
            }
            modules = super::modules(&routed["modules"]);
        }
    }
    if company.is_some() && !modules.iter().any(|m| m == "organizations") {
        modules.push("organizations".into());
    }
    modules.sort();
    modules.dedup();
    if modules.len() > 6
        || modules
            .iter()
            .any(|m| archava_domain::operator::module(m).is_none())
    {
        return Err(refused(StatusCode::BAD_GATEWAY, "invalid_planner_modules"));
    }
    let mut context = read::context(state, headers, tenant, actor, &modules, company).await?;
    let mut reference_ids = utterance
        .split(|c: char| c.is_whitespace() || "\"'`.,:;?!()[]{}".contains(c))
        .filter(|s| !s.is_empty() && s.chars().count() <= 128)
        .take(256)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    context["userReferenceIds"] = json!(reference_ids
        .iter()
        .filter(|v| archava_context::decision::turn::credential_text(v) == **v)
        .collect::<Vec<_>>());
    reference_ids.extend(lookups.iter().filter_map(|q| q.id.clone()));
    context["requestedReferenceIds"] = json!(reference_ids);
    if !lookups.is_empty() {
        super::lookups::expand(state, headers, tenant, &mut context, &lookups).await?;
        if action.is_some()
            && context["lookupResults"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|r| r["exactId"] == true && r["matchingRowsInSourcePage"] == 0)
        {
            return Err(refused(
                StatusCode::NOT_FOUND,
                "agent_reference_not_visible",
            ));
        }
    }
    let mut model_rounds = 0usize;
    // The same total three-call budget includes optional model routing. Local
    // routing leaves room for search -> exact detail -> plan, without a fourth call.
    let planning_limit = 3 - routing_calls;
    let mut format_corrections = 0usize;
    let mut available: Vec<String> = context["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["available"] == true)
        .filter_map(|s| s["module"].as_str().map(str::to_owned))
        .collect();
    let mut plan = if let Some(action) = action {
        let Some((module, _)) = crate::business::tool(&action.tool) else {
            return Err(refused(StatusCode::BAD_REQUEST, "unknown_agent_action"));
        };
        if !available.iter().any(|m| m == module) {
            return Err(refused(
                StatusCode::FORBIDDEN,
                "agent_action_module_unavailable",
            ));
        }
        json!({"summary":"Review the action details before authorizing this goal.","questions":[],"steps":[{"title":action.tool,"tool":action.tool,"input":action.values,"dependsOn":[]}]})
    } else if modules.is_empty() {
        json!({"summary":"I need to identify the work you want me to handle.","steps":[],"questions":["Which business area or existing goal should I work on, and what outcome do you want?"]})
    } else if let Some(provider) = provider {
        let mut permit = sharing
            .as_ref()
            .expect("provider requires a sharing permit")
            .clone();
        context["providerDisclosure"] = permit.stamp();
        let allowed = permit.modules();
        available.retain(|id| allowed.contains(id));
        let catalogue = super::schema::contracts(&available);
        let mut schema = super::schema::plan(&catalogue);
        let mut proposed;
        let mut disclosed_context;
        let mut correction = None::<Value>;
        let planning_access_hash = context["accessHash"].clone();
        loop {
            disclosure::recheck(state, headers, tenant, actor, &permit, &modules, &context).await?;
            if model_rounds > 0 {
                // Row-level access can change without changing memberships,
                // for example a Support assignment while the model is in flight.
                // A later disclosure must start from fresh authorized reads.
                let mut fresh =
                    read::context(state, headers, tenant, actor, &modules, company).await?;
                if fresh["accessHash"] != planning_access_hash {
                    return Err(refused(
                        StatusCode::FORBIDDEN,
                        "agent_context_access_changed",
                    ));
                }
                for field in ["userReferenceIds", "requestedReferenceIds"] {
                    fresh[field] = context[field].clone();
                }
                // These exact IDs were grounded when the typed lookup was
                // accepted. They authorize a scoped re-read, never access to a
                // row or permission to disclose it after current access is lost.
                let mut references = fresh["requestedReferenceIds"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for id in lookups.iter().filter_map(|q| q.id.as_ref()) {
                    if !references.iter().any(|v| v == id) {
                        references.push(json!(id));
                    }
                }
                fresh["requestedReferenceIds"] = json!(references);
                if !lookups.is_empty() {
                    super::lookups::expand(state, headers, tenant, &mut fresh, &lookups).await?;
                }
                context = fresh;
            }
            context["providerDisclosure"] = permit.stamp();
            permit
                .refresh_company_sources(state, tenant, actor, &context)
                .await?;
            let outbound = permit.project(&context);
            let mut model_input = json!({"phase":"plan","goal":archava_context::decision::turn::credential_text(utterance),"previousGoal":permit.prior(parent),"history":[],"preferences":[],"authorizedContext":outbound,"actionContracts":catalogue,"readExpansion":{"maximumRequests":8,"maximumPlanningRounds":planning_limit,"currentPlanningRound":model_rounds+1,"canRequestAnotherRead":model_rounds+1<planning_limit && lookups.len()<8,"remainingReadRequests":8usize.saturating_sub(lookups.len()),"allowedCollections":outbound["sources"].as_array().into_iter().flatten().filter(|s|s["available"]==true).map(|s|json!({"module":s["module"],"collections":s["data"].as_object().into_iter().flatten().filter(|(k,v)|v.is_array()&&!matches!(k.as_str(),"permissions"|"collections")).map(|(k,_)|k.clone()).collect::<Vec<_>>()})).collect::<Vec<_>>()},"instructions":"Choose the registered action whose documented meaning matches the requested change before selecting reads. Read only facts required by that action. User-supplied literal text for a free-text field is new input, not an entity identifier that needs a lookup or an existing row. When the target record is outside the bounded authorized page, request lookups with exact module/collection and either a literal search query or exact observed/user-supplied id. A read-only lookup request must have steps=[] and questions=[]. A lookup is not a business action or an authorization grant. Use the authorized lookup rows before requesting more. A distinct search or exact detail read is allowed only when canRequestAnotherRead is true. Never repeat an identical read. On the last planning round, return a grounded plan/answer or targeted clarification; do not request another lookup. Never calculate full totals from a bounded collection; coverage and lookupResults explain what was included. Use only supplied registered <module>.<action> tools and exact contract field names. Do not repeat completed effects from the previous goal. Return targeted questions with steps=[] when required business input is missing. For a complete proposed plan, return questions=[]; asking for approval is the host UI responsibility and must never be put in questions. No mutation may execute until the host records human approval. To reference an earlier verified result use an object {\"$from\":{\"step\":0,\"path\":[\"receipt\",\"receipt\",\"id\"]}} only for IDs or revision fields; the earlier zero-based array index must be in dependsOn. The first step index is 0. To use the supplier created by the first step, supplierId must be exactly {\"$from\":{\"step\":0,\"path\":[\"receipt\",\"receipt\",\"id\"]}} and dependsOn must include 0. Never substitute an object with a from key or an invented identifier. input is a JSON object matching the selected action schema, not a quoted JSON string or an HTTP envelope. Automatic request keys are generated by the host and must be omitted. No model text can assert that work committed."});
            if let Some(correction) = &correction {
                model_input["formatCorrection"] = correction.clone();
            }
            disclosure::bound_payload(&mut model_input)?;
            disclosed_context = model_input["authorizedContext"].clone();
            // The last call the budget allows cannot be followed by a read, so
            // its schema does not offer one. `readExpansion` states the same
            // rule to the model; the schema makes it a constraint.
            let can_read = model_rounds + 1 < planning_limit && lookups.len() < 8;
            let closed;
            let call_schema = if can_read {
                &schema
            } else {
                closed = super::schema::without_reads(&schema);
                &closed
            };
            proposed = provider
                .generate(&model_input, call_schema)
                .await
                .map_err(provider_unavailable)?;
            disclosure::recheck(state, headers, tenant, actor, &permit, &modules, &context).await?;
            model_rounds += 1;
            let more = match proposed.get("lookups") {
                None => Vec::new(),
                Some(v) => super::lookups::requests(v)?,
            };
            if more.is_empty() {
                let checked = (|| -> Result<Value, Response> {
                    let mut candidate = normalize(proposed.clone())?;
                    for step in candidate["steps"].as_array().into_iter().flatten() {
                        let tool = step["tool"].as_str().ok_or_else(|| {
                            refused(StatusCode::BAD_GATEWAY, "invalid_agent_tool")
                        })?;
                        let contract =
                            catalogue
                                .iter()
                                .find(|c| c["tool"] == tool)
                                .ok_or_else(|| {
                                    refused(StatusCode::BAD_GATEWAY, "unregistered_agent_tool")
                                })?;
                        require_model_fields(&step["input"], &contract["inputSchema"])?;
                    }
                    validate_model_references(&candidate, &disclosed_context)?;
                    validate(&mut candidate, tenant, &available, &context)?;
                    Ok(candidate)
                })();
                match checked {
                    Ok(candidate) => {
                        proposed = candidate;
                        break;
                    }
                    Err(mut error) => {
                        let reason = error.1 .0["reason"].as_str().unwrap_or_default();
                        if model_rounds < planning_limit
                            && format_corrections == 0
                            && (error.1 .0["contractViolation"] == true
                                || matches!(
                                    reason,
                                    "invalid_agent_output_binding" | "invalid_agent_dependencies"
                                ))
                        {
                            let selected = selected_contracts(&proposed, &catalogue);
                            if selected.is_empty() {
                                return Err(error);
                            }
                            schema = super::schema::correction_plan(&selected);
                            let mut details = binding_correction(reason, &proposed);
                            details["remainingPlanningRounds"] =
                                json!(planning_limit - model_rounds);
                            if error.1 .0["contractViolation"] == true {
                                details["instruction"] = json!("The proposal did not match its selected action contract. Return a fresh valid proposal; do not repeat the rejected input or invent missing business facts.");
                            }
                            // These are installed public schemas, never failed
                            // input keys/values, rejected prose or record IDs.
                            details["selectedActionContracts"] = json!(selected);
                            details["contractInstruction"] = json!("Return a fresh complete proposal using only the selected registered contracts. Include every required field and no other fields. Respect primitive JSON types and nested objects. Do not invent or change business values to repair a format error; ask clarification with steps=[] when required business facts are missing. No proposal was approved or executed.");
                            correction = Some(details);
                            format_corrections += 1;
                            continue;
                        }
                        if matches!(
                            reason,
                            "invalid_agent_output_binding" | "invalid_agent_dependencies"
                        ) {
                            error.1 .0["formatIssues"] =
                                binding_correction(reason, &proposed)["bindings"].clone();
                        }
                        return Err(error);
                    }
                }
            }
            if model_rounds >= planning_limit || more.iter().all(|q| lookups.contains(q)) {
                let mut error = refused(StatusCode::BAD_GATEWAY, "agent_lookup_budget_exceeded");
                // Counts and validated public collection names only. Never
                // reflect a search term, record ID or failed model text.
                let reads = |queries: &[super::lookups::Lookup]| {
                    queries.iter().filter(|q|q.validate(&disclosed_context).is_ok()).map(|q|json!({"module":q.module,"collection":q.collection,"byId":q.id.is_some()})).collect::<Vec<_>>()
                };
                error.1 .0["planningBudget"] = json!({"planningRounds":model_rounds,"routingCalls":routing_calls,"providerCalls":routing_calls+model_rounds,"maximumProviderCalls":3,"formatCorrections":format_corrections,"completedReads":reads(&lookups),"requestedReads":reads(&more),"newRequests":more.iter().filter(|q|!lookups.contains(q)).count()});
                return Err(error);
            }
            if proposed["steps"].as_array().is_none_or(|s| !s.is_empty())
                || proposed["questions"]
                    .as_array()
                    .is_none_or(|s| !s.is_empty())
            {
                return Err(refused(
                    StatusCode::BAD_GATEWAY,
                    "agent_lookup_plan_must_only_read",
                ));
            }
            for query in more {
                query.validate(&disclosed_context)?;
                if !lookups.contains(&query) {
                    lookups.push(query)
                }
            }
            // The next round re-reads all sources and current typed lookups;
            // retaining unrelated old source rows would bypass record revocation.
        }
        proposed
    } else {
        local_plan(
            state,
            headers,
            tenant,
            &parent
                .map(|p| {
                    format!(
                        "{}\nUpdate: {utterance}",
                        p["objective"].as_str().unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| utterance.to_string()),
            &modules,
            &context,
        )
        .await?
    };
    let mut financial_reads = false;
    for step in plan["steps"].as_array().into_iter().flatten() {
        if matches!(
            step["tool"].as_str(),
            Some(
                "accounting.journal.post"
                    | "accounting.journal.reverse"
                    | "accounting.journal.void"
            )
        ) {
            let action = ActionInput {
                tool: step["tool"].as_str().unwrap_or_default().into(),
                values: step["input"].clone(),
            };
            for q in super::lookups::action_requests(&action) {
                if q.module == "accounting" && q.collection == "journals" && !lookups.contains(&q) {
                    lookups.push(q);
                    financial_reads = true;
                }
            }
        }
    }
    if financial_reads {
        super::lookups::expand(state, headers, tenant, &mut context, &lookups).await?;
    }
    if company.is_some() {
        validate_model_references(&plan, &context)?;
    }
    validate(&mut plan, tenant, &available, &context)?;
    let planner=provider.map(|p|json!({"providerId":"gemini","planningRounds":model_rounds,"routingCalls":routing_calls,"providerCalls":routing_calls+model_rounds,"maximumProviderCalls":3,"formatCorrections":format_corrections,"readQueries":lookups.len(),"model":p.model()})).unwrap_or_else(||json!({"providerId":"archava-agent-local","model":"bounded-business-planner-v1","limitation":"AI planning is unavailable for the current sharing selection or provider setup. Local reads, supported replenishment plans and explicit registered actions remain available; review AI data sharing in Workspace settings to use the configured provider."}));
    Ok(Proposal {
        modules,
        context,
        plan,
        planner,
    })
}
async fn local_plan(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    text: &str,
    modules: &[String],
    context: &Value,
) -> Result<Value, Response> {
    let lower = text.to_lowercase();
    if context["companyId"].is_null()
        && modules.iter().any(|m| m == "commerce" || m == "purchasing")
        && ["replenish", "stok", "stock", "pembelian", "purchas"]
            .iter()
            .any(|w| lower.contains(w))
    {
        let (status, Json(cards)) = read::invoke(
            state,
            headers,
            Method::GET,
            &format!("/api/v1/feed/action-cards?tenant={tenant}"),
            None,
        )
        .await?;
        if status.is_success() {
            let mut steps = Vec::new();
            for card in cards["feed"]["cards"]
                .as_array()
                .into_iter()
                .flatten()
                .take(8)
            {
                let (Some(id), Some(hash)) = (
                    card["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()),
                    card["plan_hash"].as_str(),
                ) else {
                    continue;
                };
                // Server-bound feed values are the real authorized observation, never reconstructed from model text.
                let actor = authenticate(state, headers, false)
                    .await
                    .map_err(|(s, b)| (s, Json(json!(b.0))))?;
                let mut tx = begin(state, tenant, &actor.principal_id).await?;
                let review = workflows::load(
                    tx.checked_executor().await.map_err(unavailable)?,
                    tenant,
                    &actor.principal_id,
                    "feed",
                    id,
                    Some(hash),
                )
                .await?;
                tx.commit().await.map_err(unavailable)?;
                steps.push(json!({"title":card["title"],"tool":"purchasing.order.create","input":review.plan["values"],"dependsOn":[],"observation":{"productId":review.plan["productId"],"stockRevision":review.plan["stockRevision"],"quantity":review.plan["observedQuantity"]}}));
            }
            if !steps.is_empty() {
                return Ok(
                    json!({"summary":"I found low-stock items with current supplier offers. Review these draft purchase orders and costs; purchase confirmation remains separate.","questions":[],"steps":steps}),
                );
            }
        }
    }
    let mut findings = Vec::new();
    for source in context["sources"].as_array().into_iter().flatten() {
        if source["available"] != true {
            findings.push(format!(
                "{}: unavailable under your current access or module configuration.",
                source["module"].as_str().unwrap_or("Module")
            ));
            continue;
        }
        let counts = source["data"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| {
                value
                    .as_array()
                    .filter(|_| !matches!(key.as_str(), "permissions" | "collections"))
                    .map(|rows| format!("{key}: {} records in this bounded view", rows.len()))
            })
            .collect::<Vec<_>>();
        findings.push(format!(
            "{} — {}",
            source["module"].as_str().unwrap_or("Module"),
            counts.join(", ")
        ));
    }
    let question = if modules.len() == 1 {
        format!("For {}, which record and action should I prepare? Choose a registered action below or describe the details. I will show a plan before changing business data.",archava_domain::operator::module(&modules[0]).and_then(|m|m["label"].as_str()).unwrap_or(&modules[0]))
    } else {
        "Which outcome should I prioritize across these business areas, and what limits or records should the plan use?".into()
    };
    Ok(
        json!({"summary":findings.join("\n"),"steps":[],"questions":[question],"suggestedModules":modules}),
    )
}
/// Correction metadata contains only safe structural facts, never failed
/// proposal prose, business values, UUIDs or raw output payloads.
fn binding_correction(reason: &str, proposal: &Value) -> Value {
    fn walk(value: &Value, field: Option<&str>, step: usize, deps: &[Value], out: &mut Vec<Value>) {
        if out.len() >= 12 {
            return;
        }
        if let Some(binding) = value.get("$from") {
            let field = field
                .filter(|f| {
                    matches!(
                        *f,
                        "id" | "supplierId"
                            | "productId"
                            | "accountId"
                            | "bookId"
                            | "orderId"
                            | "expectedRevision"
                            | "expectedAccountRevision"
                    )
                })
                .unwrap_or("other");
            out.push(json!({"targetStep":step,"field":field,"bindingIsObject":binding.is_object(),"soleInputKey":value.as_object().is_some_and(|o|o.len()==1),"sourceStep":binding["step"].as_u64().filter(|n|*n<12),"dependsOn":deps.iter().filter_map(Value::as_u64).filter(|n|*n<12).take(12).collect::<Vec<_>>(),"pathIsArray":binding["path"].is_array(),"pathParts":binding["path"].as_array().map(Vec::len)}));
            return;
        }
        match value {
            Value::Object(o) => {
                for (k, v) in o {
                    walk(v, Some(k), step, deps, out)
                }
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, field, step, deps, out)
                }
            }
            _ => {}
        }
    }
    let mut bindings = Vec::new();
    for (index, step) in proposal["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .take(12)
        .enumerate()
    {
        walk(
            &step["input"],
            None,
            index,
            step["dependsOn"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default(),
            &mut bindings,
        )
    }
    json!({"reason":reason,"bindings":bindings,"remainingPlanningRounds":1,"instruction":"Return a new complete proposal with valid earlier ZERO-BASED dependencies. The first array step is 0. For a supplier created by step 0, the SECOND step must include dependsOn:[0] and supplierId:{\"$from\":{\"step\":0,\"path\":[\"receipt\",\"receipt\",\"id\"]}}. A path is an array of individual keys, never a dotted string. Do not place $from at the root input. Only IDs/revisions may bind; never prices or quantities. Keep business values consistent with the current user request and approved context. Ask clarification with steps=[] if required facts are missing. No work was approved by this rejected proposal."})
}
fn normalize(value: Value) -> Result<Value, Response> {
    if value["summary"].as_str().is_none_or(|s| s.len() > 4000) {
        return Err(refused(StatusCode::BAD_GATEWAY, "invalid_planner_proposal"));
    }
    let questions = value["questions"]
        .as_array()
        .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_planner_questions"))?;
    if questions.len() > 8
        || questions
            .iter()
            .any(|v| v.as_str().is_none_or(|s| s.is_empty() || s.len() > 1000))
    {
        return Err(refused(
            StatusCode::BAD_GATEWAY,
            "invalid_planner_questions",
        ));
    }
    // A model can include a draft beside missing-input questions. Keep only the
    // clarification; no draft action may become executable before it is answered.
    if !questions.is_empty() {
        return Ok(json!({"summary":value["summary"],"questions":questions,"steps":[]}));
    }
    let mut steps = Vec::new();
    for step in value["steps"]
        .as_array()
        .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_planner_steps"))?
    {
        let input = step["input"]
            .as_object()
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_planner_input"))?;
        let input = json!(input);
        steps.push(json!({"title":step["title"],"tool":step["tool"],"input":input,"dependsOn":step["dependsOn"]}));
    }
    Ok(json!({"summary":value["summary"],"questions":questions,"steps":steps}))
}
pub(super) fn validate(
    plan: &mut Value,
    tenant: &TenantId,
    available: &[String],
    context: &Value,
) -> Result<(), Response> {
    let unanswered = plan["questions"].as_array().is_some_and(|q| !q.is_empty());
    let steps = plan["steps"]
        .as_array_mut()
        .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_agent_plan"))?;
    if steps.len() > 12 {
        return Err(refused(
            StatusCode::BAD_GATEWAY,
            "agent_step_budget_exceeded",
        ));
    }
    if !steps.is_empty() && unanswered {
        return Err(refused(
            StatusCode::BAD_GATEWAY,
            "agent_unanswered_questions_with_actions",
        ));
    }
    for (index, step) in steps.iter_mut().enumerate() {
        let tool = step["tool"]
            .as_str()
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_agent_tool"))?;
        let (module, action) = crate::business::tool(tool)
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "unregistered_agent_tool"))?;
        if !available.iter().any(|m| m == module) {
            return Err(refused(
                StatusCode::FORBIDDEN,
                "agent_tool_context_unavailable",
            ));
        }
        if step["title"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty() || s.len() > 200)
            || !step["input"].is_object()
        {
            return Err(refused(StatusCode::BAD_GATEWAY, "invalid_agent_step"));
        }
        let deps = step["dependsOn"]
            .as_array()
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_agent_dependencies"))?;
        if deps
            .iter()
            .any(|d| d.as_u64().is_none_or(|i| i as usize >= index))
        {
            return Err(refused(
                StatusCode::BAD_GATEWAY,
                "invalid_agent_dependencies",
            ));
        }
        if let Some(company) = context["companyId"].as_str() {
            if matches!(
                (module, action),
                (
                    "commerce",
                    "product.create"
                        | "stock.adjust"
                        | "order.create"
                        | "order.create_from_contact"
                        | "order.create_from_party"
                ) | ("purchasing", "order.create")
                    | ("billing", "create_invoice")
            ) {
                return Err(refused(
                    StatusCode::BAD_GATEWAY,
                    "agent_company_action_required",
                ));
            }
            if step["input"]
                .get("companyId")
                .is_some_and(|v| v.as_str() != Some(company))
            {
                return Err(refused(
                    StatusCode::BAD_GATEWAY,
                    "agent_company_scope_mismatch",
                ));
            }
        }
        let mut preview = step["input"].clone();
        let contract = archava_domain::operator::module(module)
            .and_then(|m| m["actions"].as_array())
            .and_then(|actions| actions.iter().find(|a| a["id"] == action))
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "unregistered_agent_action"))?;
        validate_fields(&preview, &contract["fields"])?;
        validate_bindings(&mut preview, None, index, deps)?;
        archava_domain::operator::envelope(
            module,
            tenant.as_str(),
            "00000000-0000-4000-8000-000000000001",
            action,
            &preview,
        )
        .map_err(|_| refused(StatusCode::BAD_GATEWAY, "agent_action_fields_invalid"))?;
        let guards = guards(&step["input"], context);
        step["guards"] = json!(guards);
    }
    Ok(())
}
/// Model proposals must state every required field, including reviewed defaults.
/// Native typed forms retain their existing explicit default construction.
fn model_field_error(reason: &str) -> Response {
    let mut error = refused(StatusCode::BAD_GATEWAY, reason);
    error.1 .0["contractViolation"] = json!(true);
    error
}
fn selected_contracts(proposal: &Value, catalogue: &[Value]) -> Vec<Value> {
    let mut selected = Vec::new();
    for step in proposal["steps"].as_array().into_iter().flatten().take(12) {
        if let Some(contract) = catalogue.iter().find(|c| c["tool"] == step["tool"]) {
            if !selected.contains(contract) {
                selected.push(contract.clone());
            }
        }
    }
    selected
}
fn require_model_fields(value: &Value, schema: &Value) -> Result<(), Response> {
    if value.get("$from").is_some() {
        return Ok(());
    }
    if let Some(variants) = schema["anyOf"].as_array() {
        return if variants
            .iter()
            .any(|s| require_model_fields(value, s).is_ok())
        {
            Ok(())
        } else {
            Err(model_field_error("agent_action_fields_invalid"))
        };
    }
    let valid_type = match schema["type"].as_str() {
        Some("null") => value.is_null(),
        Some("string") => value.as_str().is_some_and(|s| {
            schema["maxLength"]
                .as_u64()
                .is_none_or(|max| s.chars().count() <= max as usize)
        }),
        Some("integer") => value.as_i64().is_some(),
        Some("number") => value.as_f64().is_some_and(f64::is_finite),
        Some("boolean") => value.is_boolean(),
        Some("object") => value.is_object(),
        Some("array") => value.as_array().is_some_and(|a| {
            schema["minItems"]
                .as_u64()
                .is_none_or(|min| a.len() >= min as usize)
                && schema["maxItems"]
                    .as_u64()
                    .is_none_or(|max| a.len() <= max as usize)
        }),
        _ => false,
    };
    if !valid_type
        || schema["enum"]
            .as_array()
            .is_some_and(|options| !options.contains(value))
    {
        return Err(model_field_error("agent_action_fields_invalid"));
    }
    if schema["type"] == "object" {
        let object = value
            .as_object()
            .ok_or_else(|| model_field_error("agent_action_fields_invalid"))?;
        let missing: Vec<&str> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|key| !object.contains_key(*key))
            .collect();
        if !missing.is_empty() {
            // Only installed contract names are returned, never model values.
            let mut error = model_field_error("agent_action_fields_invalid");
            error.1 .0["missingFields"] = json!(missing);
            return Err(error);
        }
        for (key, value) in object {
            let Some(field) = schema["properties"].get(key) else {
                return Err(model_field_error("unknown_agent_action_field"));
            };
            require_model_fields(value, field)?;
        }
    } else if schema["type"] == "array" {
        for value in value
            .as_array()
            .ok_or_else(|| model_field_error("agent_action_fields_invalid"))?
        {
            require_model_fields(value, &schema["items"])?;
        }
    }
    Ok(())
}
fn validate_fields(value: &Value, fields: &Value) -> Result<(), Response> {
    let fields = fields
        .as_array()
        .ok_or_else(|| unavailable("invalid action contract"))?;
    let object = value
        .as_object()
        .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "invalid_agent_fields"))?;
    for (key, v) in object {
        let field = fields
            .iter()
            .find(|f| f["name"] == *key && f["automatic"] != "requestId")
            .ok_or_else(|| refused(StatusCode::BAD_GATEWAY, "unknown_agent_action_field"))?;
        if v.get("$from").is_some() {
            continue;
        }
        if field["kind"] == "object" && !v.is_null() {
            validate_fields(v, &field["fields"])?;
        }
        if field["kind"] == "array" && field["items"]["kind"] == "object" {
            for item in v.as_array().into_iter().flatten() {
                validate_fields(item, &field["items"]["fields"])?;
            }
        }
    }
    Ok(())
}
// A model may select observed records or bind a verified prior result, never invent references.
fn validate_model_references(plan: &Value, context: &Value) -> Result<(), Response> {
    fn observed(value: &Value, wanted: &str) -> bool {
        match value {
            Value::String(s) => s == wanted,
            Value::Object(o) => o.values().any(|v| observed(v, wanted)),
            Value::Array(a) => a.iter().any(|v| observed(v, wanted)),
            _ => false,
        }
    }
    fn walk(v: &Value, key: Option<&str>, tool: &str, context: &Value) -> Result<(), Response> {
        if v.get("$from").is_some() {
            return Ok(());
        }
        // These contracts intentionally take a caller-chosen ID for a new record.
        let fresh = matches!(
            (tool, key),
            ("field.create_job", Some("jobId"))
                | ("education.enroll_student", Some("enrollmentId"))
                | ("accommodation.room.create", Some("roomId"))
        );
        if !fresh
            && key.is_some_and(|s| s == "id" || s.ends_with("Id"))
            && v.as_str()
                .is_some_and(|s| Uuid::parse_str(s).is_ok() && !observed(&context["sources"], s))
        {
            return Err(refused(
                StatusCode::BAD_GATEWAY,
                "agent_reference_not_in_authorized_context",
            ));
        }
        match v {
            Value::Object(o) => {
                for (k, v) in o {
                    walk(v, Some(k), tool, context)?;
                }
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, key, tool, context)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for step in plan["steps"].as_array().into_iter().flatten() {
        walk(
            &step["input"],
            None,
            step["tool"].as_str().unwrap_or_default(),
            context,
        )?;
    }
    Ok(())
}
fn validate_bindings(
    value: &mut Value,
    field: Option<&str>,
    index: usize,
    deps: &[Value],
) -> Result<(), Response> {
    if let Some(binding) = value.get("$from") {
        let allowed = field.is_some_and(|s| {
            s == "id" || s.ends_with("Id") || s == "revision" || s.ends_with("Revision")
        });
        let step = binding["step"].as_u64();
        let path = binding["path"].as_array();
        if !allowed
            || value.as_object().is_none_or(|o| o.len() != 1)
            || step
                .is_none_or(|s| s as usize >= index || !deps.iter().any(|v| v.as_u64() == Some(s)))
            || path.is_none_or(|p| {
                p.is_empty()
                    || p.len() > 6
                    || p.iter().any(|v| {
                        v.as_str().is_none_or(|s| {
                            s.len() > 80
                                || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                        })
                    })
            })
        {
            return Err(refused(
                StatusCode::BAD_GATEWAY,
                "invalid_agent_output_binding",
            ));
        }
        *value = if field.is_some_and(|f| f == "revision" || f.ends_with("Revision")) {
            json!(1)
        } else {
            json!("00000000-0000-4000-8000-000000000001")
        };
        return Ok(());
    }
    match value {
        Value::Object(o) => {
            for (k, v) in o {
                validate_bindings(v, Some(k), index, deps)?
            }
        }
        Value::Array(a) => {
            for v in a {
                validate_bindings(v, field, index, deps)?
            }
        }
        _ => {}
    }
    Ok(())
}
fn guards(input: &Value, context: &Value) -> Vec<Value> {
    fn ids(v: &Value, result: &mut Vec<String>) {
        match v {
            Value::String(s) if Uuid::parse_str(s).is_ok() => result.push(s.clone()),
            Value::Object(o) => {
                for v in o.values() {
                    ids(v, result)
                }
            }
            Value::Array(a) => {
                for v in a {
                    ids(v, result)
                }
            }
            _ => {}
        }
    }
    let mut references = Vec::new();
    ids(input, &mut references);
    let mut guards = Vec::new();
    for source in context["sources"].as_array().into_iter().flatten() {
        for (collection, rows) in source["data"].as_object().into_iter().flatten() {
            for row in rows.as_array().into_iter().flatten() {
                if row["id"]
                    .as_str()
                    .is_some_and(|s| references.iter().any(|id| id == s))
                {
                    let mut fields = serde_json::Map::new();
                    for key in [
                        "revision",
                        "stockRevision",
                        "price",
                        "unitCost",
                        "active",
                        "state",
                        "status",
                        "quantity",
                        "companyId",
                        "bookId",
                        "currency",
                        "minorUnits",
                        "policyId",
                        "policyRevision",
                        "sourceDate",
                        "sourceRevision",
                        "sourceState",
                        "amountMinor",
                        "goodsMinor",
                        "landedFeeMinor",
                        "legs",
                        "policy",
                        "lines",
                    ] {
                        if let Some(v) = row.get(key) {
                            fields.insert(key.into(), v.clone());
                        }
                    }
                    guards.push(json!({"module":source["module"],"collection":collection,"id":row["id"],"fields":fields}));
                }
            }
        }
    }
    guards
}
pub(super) fn proposal_text(plan: &Value, state: &str) -> String {
    let summary = plan["summary"]
        .as_str()
        .unwrap_or("I reviewed the available business context.");
    match state{"planned"=>format!("{summary}\nThe proposed actions are waiting for your review and approval. No business action has executed."),"clarifying"=>format!("{summary}\n{}",plan["questions"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")),_=>summary.into()}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_contracts_require_reviewed_defaults_and_reject_extra_fields() {
        let contracts = super::super::schema::contracts(&["commerce".into()]);
        let schema = &contracts
            .iter()
            .find(|c| c["tool"] == "commerce.product.create")
            .unwrap()["inputSchema"];
        let mut input =
            json!({"sku":"MODEL-VALID","title":"Reviewed product","unit":"unit","price":1000});
        assert!(
            require_model_fields(&input, schema).is_err(),
            "The model cannot silently omit a required tracking choice"
        );
        input["trackStock"] = json!(false);
        assert!(require_model_fields(&input, schema).is_ok());
        for invalid in [json!("1000"), json!(1000.5), json!(true), Value::Null] {
            input["price"] = invalid;
            assert!(
                require_model_fields(&input, schema).is_err(),
                "Model money must use the registered primitive type, without native-form coercion"
            );
        }
        input["price"] = json!(1000);
        let mut plan = json!({"summary":"Explicit reviewed work","questions":[],"steps":[{"title":"Create reviewed product","tool":"commerce.product.create","input":input,"dependsOn":[]}]});
        let tenant = TenantId::new("acme-retail").unwrap();
        assert!(validate(
            &mut plan,
            &tenant,
            &["commerce".into()],
            &json!({"sources":[]})
        )
        .is_ok());
        plan["steps"][0]["input"]["role"] = json!("owner");
        assert!(require_model_fields(&plan["steps"][0]["input"], schema).is_err());
        assert!(validate(
            &mut plan,
            &tenant,
            &["commerce".into()],
            &json!({"sources":[]})
        )
        .is_err());
        plan["steps"][0]["input"]
            .as_object_mut()
            .unwrap()
            .remove("role");
        plan["steps"][0]["input"]["price"] = json!("not an amount");
        assert!(
            validate(
                &mut plan,
                &tenant,
                &["commerce".into()],
                &json!({"sources":[]})
            )
            .is_err(),
            "The compact provider envelope does not weaken typed host validation"
        );
    }
    #[test]
    fn clarification_discards_unready_draft_actions() {
        let mut plan=normalize(json!({"summary":"Supplier details are missing","questions":["Which supplier should be used?"],"steps":[{"title":"Unready draft","tool":"purchasing.order.create","inputJson":"not an executable proposal","dependsOn":[]}]})).unwrap();
        assert_eq!(plan["steps"], json!([]));
        assert_eq!(plan["questions"].as_array().unwrap().len(), 1);
        validate(
            &mut plan,
            &TenantId::new("acme-retail").unwrap(),
            &[],
            &json!({}),
        )
        .unwrap();
    }
    #[test]
    fn scheduling_is_not_contact_search() {
        for text in [
            "Cari jadwal resource yang bentrok dan ajukan pilihan pemindahan reservasi.",
            "Find conflicting resource bookings and propose reservation changes.",
        ] {
            assert!(route(text).contains(&"scheduling".into()));
            assert!(!route(text).contains(&"crm".into()));
        }
    }
    #[test]
    fn bindings_cannot_supply_money_or_future_steps() {
        let mut v = json!({"price":{"$from":{"step":0,"path":["receipt","total"]}}});
        assert!(validate_bindings(&mut v, None, 1, &[json!(0)]).is_err());
        let mut v = json!({"productId":{"$from":{"step":2,"path":["receipt","id"]}}});
        assert!(validate_bindings(&mut v, None, 1, &[json!(0)]).is_err());
    }
    #[test]
    fn unregistered_fields_tools_and_invented_model_references_are_refused() {
        let tenant = TenantId::new("acme-retail").unwrap();
        for (tool, input) in [
            (
                "commerce.product.create",
                json!({"sku":"TEST","title":"Test","unit":"unit","trackStock":true,"price":1000,"role":"owner"}),
            ),
            ("shell.execute", json!({"command":"echo unsafe"})),
            (
                "purchasing.order.create",
                json!({"supplierId":"00000000-0000-4000-8000-000000000001","lines":[{"offerId":"00000000-0000-4000-8000-000000000002","quantity":1,"unitCost":-1}]}),
            ),
        ] {
            let mut plan = json!({"steps":[{"title":"Untrusted proposal","tool":tool,"input":input,"dependsOn":[]}]});
            assert!(validate(
                &mut plan,
                &tenant,
                &["commerce".into(), "purchasing".into()],
                &json!({})
            )
            .is_err());
        }
        let plan = json!({"steps":[{"tool":"purchasing.offer.create","input":{"productId":"00000000-0000-4000-8000-000000000001"}}]});
        assert!(validate_model_references(&plan, &json!({"sources":[]})).is_err());
    }
}
