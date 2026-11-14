//! Last host boundary before the configured business planner sees any data.
//! Owner consent narrows current domain read grants; it never replaces them.
use super::*;
use archava_own::{DataClass, DisclosurePolicy};
use std::collections::BTreeSet;
mod fields;
#[cfg(test)]
mod tests;

pub(super) const INPUT_BYTE_LIMIT: usize = 131_072;
#[derive(Clone)]
pub(super) struct Permit {
    policy: DisclosurePolicy,
    identity: String,
    company_sources: std::collections::BTreeMap<String, String>,
}
impl Permit {
    /// Company identities must not escape through source organizations, parties
    /// or affiliations. This private index is never stored in a public goal.
    pub async fn refresh_company_sources(
        &mut self,
        state: &AppState,
        tenant: &TenantId,
        actor: &str,
        context: &Value,
    ) -> Result<(), Response> {
        self.company_sources.clear();
        if !self.policy.modules.contains("organizations") {
            return Ok(());
        }
        let mut ids = BTreeSet::new();
        for s in context["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["module"] == "organizations" && s["available"] == true)
        {
            for (collection, field) in [
                ("organizations", "id"),
                ("parties", "sourceOrganizationId"),
                ("affiliations", "organizationId"),
            ] {
                for row in s["data"][collection].as_array().into_iter().flatten() {
                    if let Some(id) = row[field].as_str().and_then(|id| Uuid::parse_str(id).ok()) {
                        ids.insert(id);
                    }
                }
            }
        }
        if ids.is_empty() {
            return Ok(());
        }
        let mut tx = begin(state, tenant, actor).await?;
        let rows = archava_adapters::erp::company_trade::company_sources(
            tx.checked_executor().await.map_err(unavailable)?,
            tenant.as_str(),
            &ids.into_iter().collect::<Vec<_>>(),
        )
        .await
        .map_err(unavailable)?;
        self.company_sources = rows
            .into_iter()
            .map(|(org, company)| (org.to_string(), company.to_string()))
            .collect();
        tx.commit().await.map_err(unavailable)?;
        Ok(())
    }
    pub fn can_plan(&self, company: Option<Uuid>) -> bool {
        !self.policy.modules.is_empty()
            && self
                .policy
                .data_classes
                .contains(&DataClass::ConversationText)
            && company
                .map(|id| self.policy.company_allowed(Some(&id.to_string())))
                .unwrap_or(self.policy.include_workspace_records)
    }
    pub fn modules(&self) -> Vec<String> {
        self.policy.modules.iter().cloned().collect()
    }
    pub fn stamp(&self) -> Value {
        json!({"policyHash":self.identity,"providerBinding":self.policy.provider_binding,
            "modules":self.policy.modules,"dataClasses":self.policy.data_classes,
            "companies":self.policy.companies,"includeWorkspaceRecords":self.policy.include_workspace_records,
            "history":"Historical messages, preferences, action inputs and raw outcomes are withheld. Only selected tool and verified completion metadata may be shared."})
    }
    /// Locally authorized snapshots remain untouched for guards and manual work.
    pub fn project(&self, context: &Value) -> Value {
        let company = context["companyId"].as_str();
        let mut sources = Vec::new();
        for source in context["sources"].as_array().into_iter().flatten() {
            let Some(module) = source["module"].as_str() else {
                continue;
            };
            if source["available"] != true || !self.policy.modules.contains(module) {
                sources.push(json!({"module":module,"available":false,"reason":"Unavailable under current read access or the approved sharing selection."}));
                continue;
            }
            let mut data = serde_json::Map::new();
            let mut coverage = serde_json::Map::new();
            for collection in fields::collections(module) {
                let Some(rows) = source["data"][collection].as_array() else {
                    continue;
                };
                let shared = rows
                    .iter()
                    .filter_map(|row| {
                        let source_id = match *collection {
                            "organizations" => row["id"].as_str(),
                            "parties" => row["sourceOrganizationId"].as_str(),
                            "affiliations" => row["organizationId"].as_str(),
                            _ => None,
                        };
                        let linked = source_id
                            .and_then(|id| self.company_sources.get(id))
                            .map(String::as_str);
                        let inherited = if module == "accounting" {
                            company
                        } else if module == "organizations" {
                            linked
                        } else {
                            None
                        };
                        fields::record(
                            row,
                            &self.policy,
                            inherited,
                            module == "organizations" && *collection == "companies",
                        )
                    })
                    .collect::<Vec<_>>();
                let omitted = shared.len() != rows.len();
                let mut c = source["coverage"][collection].clone();
                if !c.is_object() {
                    c = json!({"wholeCollection":false});
                }
                c["included"] = json!(shared.len());
                c["fieldsSelectedByPolicy"] = json!(true);
                if omitted {
                    // No counts of unapproved company rows or private records.
                    c = json!({"included":shared.len(),"wholeCollection":false,"withheldByPolicy":true,"fieldsSelectedByPolicy":true});
                }
                coverage.insert((*collection).into(), c);
                data.insert((*collection).into(), json!(shared));
            }
            // Accounting metadata is from a company-filtered, authorized read.
            if module == "accounting" {
                for field in [
                    "configuredBook",
                    "invoicePolicy",
                    "purchasePolicy",
                    "stockPolicy",
                ] {
                    if let Some(v) = source["data"].get(field).filter(|v| v.is_object()) {
                        if let Some(v) = fields::record(v, &self.policy, company, false) {
                            data.insert(field.into(), v);
                        }
                    }
                }
            }
            let projected =
                archava_context::mask::mask_sensitive_fields(&json!(data), &Default::default());
            sources.push(json!({"module":module,"available":true,"bounded":true,"coverage":coverage,"data":projected.value}));
        }
        let mut out = json!({"sources":sources,"bounded":true,"observedAt":context["observedAt"],
            "disclosure":{"fieldSelectionApplied":true,"historicalTextWithheld":true},
            "note":"Only approved fields and company scopes from current authorized bounded reads are included. This is not a complete workspace aggregate."});
        if self
            .policy
            .data_classes
            .contains(&DataClass::RecordReferences)
        {
            out["companyId"] = context["companyId"].clone();
            // Historical saved lookup IDs are private until re-read and projected.
            let mut ids = context["userReferenceIds"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for q in context["lookupRequests"].as_array().into_iter().flatten() {
                let Some(id) = q["id"].as_str() else { continue };
                let found = out["sources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["module"] == q["module"] && s["available"] == true)
                    .any(|s| {
                        s["data"].as_object().into_iter().flatten().any(|(_, v)| {
                            v.as_array()
                                .is_some_and(|rows| rows.iter().any(|r| r["id"] == id))
                        })
                    });
                if found && !ids.iter().any(|v| v == id) {
                    ids.push(json!(id));
                }
            }
            out["requestedReferenceIds"] = json!(ids);
        }
        if let Some(results) = context["lookupResults"].as_array() {
            let coverage=results.iter().filter(|r|r["module"].as_str().is_some_and(|m|self.policy.modules.contains(m))).map(|r| {
                let collection=if r["module"]=="crm" && r["collection"]=="contacts" {"items"} else {r["collection"].as_str().unwrap_or_default()};
                let included=out["sources"].as_array().into_iter().flatten().find(|s|s["module"]==r["module"]&&s["available"]==true).and_then(|s|s["data"][collection].as_array()).map_or(0,Vec::len);
                json!({"module":r["module"],"collection":r["collection"],"exactId":r["exactId"],"readStatus":r["httpStatus"],"includedRowsInCollection":included,"bounded":true,"note":"Counts describe the currently included approved collection rows, possibly merged from multiple reads. Query text and historical identifiers are withheld. Absence is not a global absence claim."})
            }).collect::<Vec<_>>();
            out["lookupResults"] = json!(coverage);
        }
        out
    }
    pub fn prior(&self, parent: Option<&Value>) -> Value {
        let Some(parent) = parent else {
            return Value::Null;
        };
        // Historical inputs can contain derived record text whose row-level
        // access changed without a membership change. Never resend those inputs.
        // Only trusted execution metadata is carried; current reads supply facts.
        let company = parent["context"]["companyId"].as_str();
        if !self.policy.company_allowed(company)
            || !self.policy.data_classes.contains(&DataClass::BusinessFacts)
        {
            return Value::Null;
        }
        let approved = parent["approved_hash"]
            .as_str()
            .zip(parent["plan_hash"].as_str())
            .is_some_and(|(approved, plan)| approved == plan)
            && parent["mission_id"]
                .as_str()
                .is_some_and(|id| parent["outcome"]["id"] == id);
        let steps = parent["plan"]["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .take(12)
            .enumerate()
            .filter_map(|(index, step)| {
                let tool = step["tool"].as_str()?;
                let (module, _) = crate::business::tool(tool)?;
                if !self.policy.modules.contains(module) {
                    return None;
                }
                let verified = approved
                    && parent["outcome"]["steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|done| {
                            done["step_index"].as_u64() == Some(index as u64)
                                && done["tool_target"] == tool
                                && done["status"] == "passed"
                                && done["payload_output"]["verified"] == true
                        });
                Some(json!({"stepIndex":index,"tool":tool,"verifiedCompleted":verified}))
            })
            .collect::<Vec<_>>();
        let completed = steps
            .iter()
            .filter(|s| s["verifiedCompleted"] == true)
            .count();
        json!({"state":parent["state"],"verifiedCompletedSteps":completed,"steps":steps,"historicalInputsWithheld":true,"notice":"Only selected tool and trusted per-step completion metadata are included. Historical inputs, text, identifiers and raw outcomes are withheld. Overall goal state alone is not proof of an effect. Do not repeat verified completed work. Use current authorized records for business values and targets; ask clarification if they are unavailable."})
    }
}
pub(super) async fn load(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
) -> Result<Option<Permit>, Response> {
    let mut tx = begin(state, tenant, actor).await?;
    let conn = tx.checked_executor().await.map_err(unavailable)?;
    let policy = crate::own::disclosure::current_policy(state, conn, tenant)
        .await
        .map_err(|e| e.response())?;
    let Some(mut policy) = policy else {
        tx.commit().await.map_err(unavailable)?;
        return Ok(None);
    };
    let raw = policy.clone();
    let mut visible = BTreeSet::new();
    for id in &policy.companies {
        let company = Uuid::parse_str(id).map_err(unavailable)?;
        match archava_adapters::erp::company_trade::resolve(
            conn,
            tenant.as_str(),
            &archava_domain::erp::purchasing::PurchasePrincipal::human(actor),
            company,
        )
        .await
        {
            Ok(_) => {
                visible.insert(id.clone());
            }
            Err(archava_adapters::erp::PurchaseError::Domain(
                archava_domain::erp::ErpError::Forbidden | archava_domain::erp::ErpError::NotFound,
            )) => {}
            Err(e) => return Err(unavailable(e)),
        }
    }
    policy.companies = visible;
    let identity = format!(
        "{:x}",
        Sha256::digest(
            json!({"approved":raw,"visibleCompanies":policy.companies})
                .to_string()
                .as_bytes()
        )
    );
    tx.commit().await.map_err(unavailable)?;
    Ok(Some(Permit {
        policy,
        identity,
        company_sources: Default::default(),
    }))
}
pub(super) async fn recheck(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    actor: &str,
    permit: &Permit,
    modules: &[String],
    context: &Value,
) -> Result<(), Response> {
    let session = authenticate(state, headers, true)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    if session.principal_id != actor
        || load(state, tenant, actor)
            .await?
            .is_none_or(|p| p.identity != permit.identity)
        || (context["accessHash"].is_string()
            && read::fingerprint(state, tenant, actor, modules).await? != context["accessHash"])
    {
        return Err(refused(StatusCode::CONFLICT, "agent_disclosure_changed"));
    }
    Ok(())
}
/// Enforce the existing transport cap across all sources and protocol metadata.
/// Required exact records are never silently shortened to fit.
pub(super) fn bound_payload(input: &mut Value) -> Result<(), Response> {
    let protected: BTreeSet<String> = input["authorizedContext"]["requestedReferenceIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    input["inputBudget"] = json!({"limit":INPUT_BYTE_LIMIT,"limited":false});
    input["authorizedContext"]["disclosure"]["limitedByTotalBytes"] = json!(false);
    while input.to_string().len() > INPUT_BYTE_LIMIT {
        let candidate = input["authorizedContext"]["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .flat_map(|(i, s)| {
                s["data"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(move |(k, v)| v.as_array().map(|rows| (i, k, rows)))
            })
            .filter_map(|(i, k, rows)| {
                rows.iter()
                    .enumerate()
                    .filter(|(_, r)| r["id"].as_str().is_none_or(|id| !protected.contains(id)))
                    .map(|(j, r)| (r.to_string().len(), i, k.clone(), j))
                    .max()
            })
            .max();
        let Some((_, i, key, j)) = candidate else {
            return Err(refused(StatusCode::BAD_REQUEST, "agent_context_too_large"));
        };
        let source = &mut input["authorizedContext"]["sources"][i];
        let rows = source["data"][&key]
            .as_array_mut()
            .expect("selected rows exist");
        rows.remove(j);
        let count = rows.len();
        source["coverage"][&key]["included"] = json!(count);
        source["coverage"][&key]["wholeCollection"] = json!(false);
        source["coverage"][&key]["limitedByTotalBytes"] = json!(true);
        input["inputBudget"]["limited"] = json!(true);
        input["authorizedContext"]["disclosure"]["limitedByTotalBytes"] = json!(true);
    }
    // Adding the marker must also fit; leave headroom without raising the cap.
    if input.to_string().len() > INPUT_BYTE_LIMIT {
        return Err(refused(StatusCode::BAD_REQUEST, "agent_context_too_large"));
    }
    Ok(())
}
