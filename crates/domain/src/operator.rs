//! Public form contracts for the installed native business modules. These are
//! input descriptions, never grants; domain persistence re-authorizes every action.
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
pub fn catalogue() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../embedded/operator-actions.v1.json"))
            .expect("embedded operator form catalogue must be valid")
    })
}
pub fn module(id: &str) -> Option<&'static Value> {
    catalogue()["modules"]
        .as_array()?
        .iter()
        .find(|m| m["id"] == id)
}
/// Typed business references shared by native forms and authorized agent retrieval.
pub fn reference(name: &str, action: &str, domain: &str) -> Option<(&'static str, &'static str)> {
    if matches!(
        (domain, action, name),
        ("field", "create_job", "jobId") | ("education", "admit_applicant", "enrollmentId")
    ) {
        return None;
    }
    Some(match name {
        "bookId" if domain == "accounting" => ("accounting", "books"),
        "accountId" if domain == "accounting" => ("accounting", "accounts"),
        "bankAccountId" if domain == "accounting" => ("accounting", "bank_accounts"),
        "bankRowId" if domain == "accounting" => ("accounting", "bank_rows"),
        "journalId" if domain == "accounting" => ("accounting", "journals"),
        "originalPaymentId" if domain == "accounting" => ("accounting", "payable_settlements"),
        "bankPolicyId" if domain == "accounting" => ("accounting", "bank_policies"),
        "periodId" if domain == "accounting" => ("accounting", "periods"),
        "sourceId" if domain == "accounting" && action == "purchase_source.draft" => {
            ("accounting", "purchase_sources")
        }
        "sourceId" if domain == "accounting" && action == "invoice_terms.set" => {
            ("accounting", "invoice_term_sources")
        }
        "sourceId" if domain == "accounting" && action == "supplier_terms.set" => {
            ("accounting", "supplier_term_sources")
        }
        "sourceId" if domain == "accounting" && action == "clearing_settlement.draft" => {
            ("accounting", "clearing_sources")
        }
        "sourceId" if domain == "accounting" && action == "payable_payment.draft" => {
            ("accounting", "payables")
        }
        "sourceId" if domain == "accounting" && action == "stock_source.draft" => {
            ("accounting", "stock_sources")
        }
        "sourceId" if domain == "accounting" => ("accounting", "invoice_sources"),
        "companyId" if domain == "accounting" => ("organizations", "companies"),
        "contactId" => ("crm", "contacts"),
        "productId" => ("commerce", "products"),
        "assigneeId" => ("support", "assignees"),
        "supplierId" => ("purchasing", "suppliers"),
        "offerId" => ("purchasing", "offers"),
        "organizationId" => ("organizations", "organizations"),
        "proposalId" => ("proposals", "proposals"),
        "changeRequestId" => ("proposals", "change_requests"),
        "projectId" => ("projects", "projects"),
        "milestoneId" => ("projects", "milestones"),
        "invoiceId" => ("billing", "invoices"),
        "resourceId" => ("scheduling", "resources"),
        "reservationId" => ("scheduling", "reservations"),
        "propertyId" => ("accommodation", "properties"),
        "roomTypeId" => ("accommodation", "room_types"),
        "roomId" => ("accommodation", "rooms"),
        "stayId" => ("accommodation", "stays"),
        "assetId" => ("assets", "assets"),
        "tableId" => ("foodservice", "tables"),
        "menuId" => ("foodservice", "menu_items"),
        "ticketId" => ("foodservice", "tickets"),
        "subscriptionId" => ("subscriptions", "subscriptions"),
        "entitlementId" => ("subscriptions", "entitlements"),
        "rosterId" => ("attendance", "roster"),
        "enrollmentId" => ("education", "enrollments"),
        "jobId" => {
            if domain == "field" {
                ("field", "jobs")
            } else {
                ("fulfillment", "jobs")
            }
        }
        "sessionId" if domain == "attendance" => ("attendance", "sessions"),
        "orderId" if domain == "laundry" => ("laundry", "orders"),
        "orderId" if domain == "purchasing" => ("purchasing", "orders"),
        "id" => match domain {
            "accounting" => (
                "accounting",
                if action.starts_with("account.") {
                    "accounts"
                } else if action.starts_with("period.") {
                    "periods"
                } else if action == "bank_allocation.reverse" {
                    "bank_allocations"
                } else if action.starts_with("tax_code.") {
                    "tax_codes"
                } else {
                    "journals"
                },
            ),
            "commerce" => {
                if action.starts_with("product.") || action == "stock.adjust" {
                    ("commerce", "products")
                } else {
                    ("commerce", "orders")
                }
            }
            "purchasing" => {
                if action.starts_with("supplier.") {
                    ("purchasing", "suppliers")
                } else if action.starts_with("offer.") {
                    ("purchasing", "offers")
                } else if action.starts_with("document.") {
                    ("purchasing", "documents")
                } else {
                    ("purchasing", "orders")
                }
            }
            "support" => ("support", "items"),
            "projects" => {
                if action.starts_with("project.") {
                    ("projects", "projects")
                } else if action.starts_with("task.") {
                    ("projects", "tasks")
                } else {
                    ("projects", "milestones")
                }
            }
            "scheduling" => {
                if action.starts_with("resource.") {
                    ("scheduling", "resources")
                } else {
                    ("scheduling", "reservations")
                }
            }
            "accommodation" => {
                if action.starts_with("property.") {
                    ("accommodation", "properties")
                } else if action.starts_with("room-type.") {
                    ("accommodation", "room_types")
                } else if action.starts_with("room.") {
                    ("accommodation", "rooms")
                } else {
                    ("accommodation", "stays")
                }
            }
            "organizations" => {
                if action.starts_with("company.") {
                    ("organizations", "companies")
                } else if action.contains("affiliation") {
                    ("organizations", "affiliations")
                } else {
                    ("organizations", "organizations")
                }
            }
            "foodservice" => {
                if action.starts_with("table.") {
                    ("foodservice", "tables")
                } else if action.starts_with("menu.") {
                    ("foodservice", "menu_items")
                } else {
                    ("foodservice", "tickets")
                }
            }
            _ => return None,
        },
        _ => return None,
    })
}

pub fn field_defaults(fields: &[Value]) -> Value {
    let mut result = Map::new();
    for f in fields {
        let Some(name) = f["name"].as_str() else {
            continue;
        };
        if f["automatic"] == "requestId" {
            continue;
        }
        if f["optional"] == true {
            continue;
        }
        let value = if let Some(v) = f.get("default") {
            v.clone()
        } else {
            match f["kind"].as_str() {
                Some("object") => field_defaults(
                    f["fields"]
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                ),
                Some("array") => {
                    let min = f["minItems"].as_u64().unwrap_or(0);
                    let item = if f["items"]["kind"] == "object" {
                        field_defaults(
                            f["items"]["fields"]
                                .as_array()
                                .map(Vec::as_slice)
                                .unwrap_or_default(),
                        )
                    } else {
                        Value::Null
                    };
                    json!((0..min).map(|_| item.clone()).collect::<Vec<_>>())
                }
                Some("boolean") => json!(false),
                _ => Value::Null,
            }
        };
        if f["optional"] != true || !value.is_null() {
            result.insert(name.into(), value);
        }
    }
    Value::Object(result)
}
fn clean(fields: &[Value], values: &Value, key: &str) -> Result<Value, String> {
    let mut result = Map::new();
    for f in fields {
        let name = f["name"].as_str().ok_or("Invalid form definition")?;
        if f["automatic"] == "requestId" {
            result.insert(name.into(), json!(key));
            continue;
        }
        if f["optional"] == true && values.get(name).is_none() {
            continue;
        }
        let value = values
            .get(name)
            .cloned()
            .or_else(|| f.get("default").cloned())
            .unwrap_or(Value::Null);
        if f["optional"] == true && value.is_null() && f["nullable"] != true {
            continue;
        }
        let value = clean_value(f, &value, key)?;
        result.insert(name.into(), value);
    }
    Ok(Value::Object(result))
}
fn clean_value(field: &Value, value: &Value, key: &str) -> Result<Value, String> {
    let label = field["label"].as_str().unwrap_or("This field");
    let invalid = || format!("{label}: enter a valid value.");
    if field["nullable"] == true && (value.is_null() || value.as_str() == Some("")) {
        return Ok(Value::Null);
    }
    let value = match field["kind"].as_str() {
        Some("object") => clean(
            field["fields"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default(),
            value,
            key,
        )?,
        Some("array") => {
            let rows = value.as_array().ok_or_else(invalid)?;
            if rows.len() < field["minItems"].as_u64().unwrap_or(0) as usize
                || rows.len() > field["maxItems"].as_u64().unwrap_or(100) as usize
            {
                return Err(format!("{label}: check the number of items."));
            }
            Value::Array(
                rows.iter()
                    .map(|row| clean_value(&field["items"], row, key))
                    .collect::<Result<_, _>>()?,
            )
        }
        Some("integer") => json!(value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(invalid)?),
        Some("number") => {
            let n = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                .filter(|v| v.is_finite())
                .ok_or_else(invalid)?;
            json!(n)
        }
        Some("boolean") => json!(value.as_bool().ok_or_else(invalid)?),
        _ => {
            let raw = value.as_str().ok_or_else(invalid)?;
            let s = if field["preserveWhitespace"] == true {
                if raw.trim() != raw {
                    return Err(invalid());
                }
                raw
            } else {
                raw.trim()
            };
            if s.is_empty() {
                return Err(invalid());
            }
            if s.chars().count() > field["maxLength"].as_u64().unwrap_or(2000) as usize {
                return Err(format!("{label}: the value is too long."));
            }
            if let Some(options) = field["options"].as_array() {
                if !options.iter().any(|v| v == s) {
                    return Err(format!("{label}: choose an available option."));
                }
            }
            json!(s)
        }
    };
    Ok(value)
}
pub fn envelope(
    module_id: &str,
    tenant: &str,
    key: &str,
    action: &str,
    values: &Value,
) -> Result<Value, String> {
    let m = module(module_id).ok_or("This module is unavailable")?;
    let a = m["actions"]
        .as_array()
        .and_then(|v| v.iter().find(|a| a["id"] == action))
        .ok_or("This action is unavailable")?;
    if a["providerOnly"] == true {
        return Err("Use the verified payment integration for this action.".into());
    }
    let mut command = clean(
        a["fields"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default(),
        values,
        key,
    )?;
    let tag = m["tag"].as_str().ok_or("Invalid form definition")?;
    command[tag] = json!(action);
    Ok(match m["envelope"].as_str() {
        Some("command") => json!({"tenant":tenant,"requestId":key,"command":command}),
        Some("action-command") => json!({"tenant":tenant,"command":command}),
        Some("nested-command") => {
            json!({"tenant":tenant,"mutation":{"requestId":key,"command":command}})
        }
        Some("organization" | "data-mutation") => {
            let mut data = command;
            data.as_object_mut().unwrap().remove(tag);
            data.as_object_mut().unwrap().remove("requestId");
            json!({"tenant":tenant,"mutation":{"requestId":key,"type":action,"data":data}})
        }
        _ => json!({"tenant":tenant,"mutation":command}),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_catalogue_has_unique_complete_input_contracts() {
        let modules = catalogue()["modules"].as_array().unwrap();
        assert_eq!(modules.len(), 19);
        let mut count = 0;
        for m in modules {
            let mut ids = std::collections::BTreeSet::new();
            for a in m["actions"].as_array().unwrap() {
                assert!(ids.insert(a["id"].as_str().unwrap()));
                assert!(a["permission"].as_str().unwrap().contains('.'));
                assert!(!a["fields"].as_array().unwrap().is_empty());
                count += 1;
            }
        }
        assert_eq!(count, 199);
    }
    #[test]
    fn supplier_payment_forms_roundtrip_exact_source_bank_and_original_payment_references() {
        let account_ref = |suffix: &str| json!({"accountId":format!("a0000000-0000-4000-8000-{suffix}"),"expectedAccountRevision":2});
        for (action, key) in [
            ("payable_payment.draft", "sourceId"),
            ("payable_return.draft", "originalPaymentId"),
        ] {
            let mut values = json!({key:"a0000000-0000-4000-8000-000000000001","bankRowId":"a0000000-0000-4000-8000-000000000002","code":"REVIEWED-PAYMENT","payableAccount":account_ref("000000000003"),"cashAccount":account_ref("000000000004"),"amountMinor":"8999999999999999","reason":"Exact native supplier bank review"});
            values[if action == "payable_payment.draft" {
                "expectedDocumentRevision"
            } else {
                "expectedJournalRevision"
            }] = json!(2);
            let wire = envelope(
                "accounting",
                "acme",
                "a0000000-0000-4000-8000-000000000005",
                action,
                &values,
            )
            .unwrap();
            assert_eq!(wire["command"][key], values[key]);
            assert_eq!(wire["command"]["bankRowId"], values["bankRowId"]);
            assert_eq!(wire["command"]["amountMinor"], "8999999999999999");
            let parsed = crate::erp::accounting::parse(
                &json!({"requestId":wire["requestId"],"command":wire["command"]}),
            )
            .unwrap();
            assert_eq!(parsed.command.operation(), action);
            assert_eq!(
                reference(key, action, "accounting"),
                Some((
                    "accounting",
                    if action == "payable_payment.draft" {
                        "payables"
                    } else {
                        "payable_settlements"
                    }
                ))
            );
        }
    }
    #[test]
    fn native_bank_envelope_preserves_original_refs_and_refuses_silent_space_changes() {
        let values = json!({"bankAccountId":"00000000-0000-4000-8000-000000000001","account":{"accountId":"00000000-0000-4000-8000-000000000002","expectedAccountRevision":1},"statement":{"reference":"November / original","fromDate":"2026-11-01","toDate":"2026-11-30","currency":"USD","openingMinor":"0","closingMinor":"100","rows":[{"externalRef":"Original / bank: A","date":"2026-11-09","amountMinor":"100"}]},"reason":"Review the original synthetic bank references"});
        let request = "00000000-0000-4000-8000-000000000003";
        let wire = envelope(
            "accounting",
            "acme",
            request,
            "bank_statement.import",
            &values,
        )
        .unwrap();
        assert_eq!(
            wire["command"]["statement"]["reference"],
            "November / original"
        );
        assert_eq!(
            wire["command"]["statement"]["rows"][0]["externalRef"],
            "Original / bank: A"
        );
        let parsed = crate::erp::accounting::parse(
            &json!({"requestId":wire["requestId"],"command":wire["command"]}),
        )
        .unwrap();
        assert_eq!(parsed.request_id, request);
        let mut changed = values.clone();
        changed["statement"]["reference"] = json!(" November / original ");
        assert!(envelope(
            "accounting",
            "acme",
            request,
            "bank_statement.import",
            &changed
        )
        .is_err());
        let mut changed = values;
        changed["statement"]["rows"][0]["externalRef"] = json!("Original / bank: A ");
        assert!(envelope(
            "accounting",
            "acme",
            request,
            "bank_statement.import",
            &changed
        )
        .is_err());
    }
    #[test]
    fn review_keeps_integer_amounts_exact_and_builds_the_real_wire_envelope() {
        let body=envelope("proposals","acme","key","create_change_request",&json!({"proposalId":"id","title":"Change","reason":"Extra work","deltaSummary":"Scope","timeDeltaDays":1,"costDeltaValue":"9007199254740993","newScopeItems":[]})).unwrap();
        assert_eq!(body["mutation"]["commandId"], "key");
        assert_eq!(
            body["mutation"]["costDeltaValue"].as_i64(),
            Some(9_007_199_254_740_993)
        );
        assert!(envelope(
            "billing",
            "acme",
            "key",
            "record_provider_event",
            &json!({})
        )
        .is_err());
    }
    #[test]
    fn nested_operator_records_keep_the_domain_wire_shape() {
        let b=envelope("education","acme","key","admit_applicant",&json!({"enrollment":{"enrollmentId":"E1","studentRef":"S1","classRef":"C1","termRef":"T1"}})).unwrap();
        assert_eq!(b["command"]["commandId"], "key");
        assert_eq!(b["command"]["action"], "admit_applicant");
        assert_eq!(b["command"]["enrollment"]["studentRef"], "S1");
    }
    #[test]
    fn every_asset_mutation_binds_the_original_request_key() {
        let assets = module("assets").unwrap();
        for action in assets["actions"].as_array().unwrap() {
            assert!(action["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["name"] == "requestId" && f["automatic"] == "requestId"));
        }
        let body = envelope("assets", "acme", "original-key", "create_asset", &json!({"code":"A1","name":"Machine","category":"equipment","ownership":"business_owned","condition":"good"})).unwrap();
        assert_eq!(body["mutation"]["requestId"], "original-key");
        assert_eq!(body["mutation"]["data"]["code"], "A1");
        assert!(body["mutation"]["data"].get("requestId").is_none());
        let valid_key = "00000000-0000-4000-8000-000000000001";
        let body = envelope("assets", "acme", valid_key, "create_asset", &json!({"code":"A1","name":"Machine","category":"equipment","ownership":"business_owned","condition":"good"})).unwrap();
        assert!(crate::erp::assets::parse_mutation(&body["mutation"]).is_ok());
    }
}
