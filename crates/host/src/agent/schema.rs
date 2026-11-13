//! Registered action schemas for model proposals. Domain handlers remain authoritative.
use serde_json::{json, Map, Value};
fn binding() -> Value {
    json!({"type":"object","properties":{"$from":{"type":"object","properties":{"step":{"type":"integer","minimum":0,"maximum":11},"path":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":6}},"required":["step","path"],"additionalProperties":false}},"required":["$from"],"additionalProperties":false})
}
fn object(fields: &Value) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in fields.as_array().into_iter().flatten() {
        if field["automatic"] == "requestId" {
            continue;
        }
        let Some(name) = field["name"].as_str() else {
            continue;
        };
        properties.insert(name.into(), field_schema(field));
        if field["optional"] != true {
            required.push(json!(name));
        }
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn field_schema(field: &Value) -> Value {
    let mut value = match field["kind"].as_str() {
        Some("object") => object(&field["fields"]),
        Some("array") => {
            json!({"type":"array","items":field_schema(&field["items"]),"minItems":field["minItems"].as_u64().unwrap_or(0),"maxItems":field["maxItems"].as_u64().unwrap_or(100)})
        }
        Some("integer") => json!({"type":"integer"}),
        Some("number") => json!({"type":"number"}),
        Some("boolean") => json!({"type":"boolean"}),
        _ => json!({"type":"string","maxLength":field["maxLength"].as_u64().unwrap_or(2000)}),
    };
    if let Some(options) = field.get("options") {
        value["enum"] = options.clone();
    }
    let name = field["name"].as_str().unwrap_or_default();
    if name == "id" || name.ends_with("Id") || name == "revision" || name.ends_with("Revision") {
        let mut description = format!("An observed {name}, or an earlier verified result for the SAME entity type. A supplier ID cannot be used as a product ID. Receipt paths traverse the complete verified step output wrapper, not just an HTTP response.");
        if name == "supplierId" {
            description.push_str(" For a supplier created at step 0, use exactly {\"$from\":{\"step\":0,\"path\":[\"receipt\",\"receipt\",\"id\"]}}.");
        }
        value = json!({"anyOf":[value,binding()],"description":description});
    }
    if field["nullable"] == true {
        value = json!({"anyOf":[value,{"type":"null"}]});
    }
    if let Some(meaning) = field["description"].as_str() {
        let existing = value["description"].as_str().unwrap_or_default();
        value["description"] = json!(format!("{existing} {meaning}").trim());
    }
    value
}
pub(super) fn contracts(modules: &[String]) -> Vec<Value> {
    modules.iter().filter_map(|id|archava_domain::operator::module(id)).flat_map(|m|{
        m["actions"].as_array().into_iter().flatten().filter(|a|a["providerOnly"]!=true).map(move|a| {
            let mut contract = json!({"tool":format!("{}.{}",m["id"].as_str().unwrap_or_default(),a["id"].as_str().unwrap_or_default()),"label":a["label"],"inputSchema":object(&a["fields"])});
            if let Some(description) = a["description"].as_str() {
                contract["description"] = json!(description);
            }
            contract
        })
    }).collect()
}
pub(super) fn plan(contracts: &[Value]) -> Value {
    // Avoid repeating a complete decoding branch for every registered action.
    // Exact per-tool input schemas remain in the approved prompt contracts and
    // are validated by the host before any goal/approval can be committed.
    let tools = contracts
        .iter()
        .map(|a| a["tool"].clone())
        .collect::<Vec<_>>();
    let steps = if tools.is_empty() {
        json!({"type":"array","maxItems":0,"items":{"type":"string"}})
    } else {
        json!({"type":"array","maxItems":12,"items":{"type":"object","properties":{"title":{"type":"string","maxLength":200},"tool":{"type":"string","enum":tools},"input":shared_input(contracts),"dependsOn":{"type":"array","items":{"type":"integer","minimum":0,"maximum":11},"description":"Earlier ZERO-BASED array indices. The second step depends on the first with [0]."}},"required":["title","tool","input","dependsOn"],"additionalProperties":false}})
    };
    let mut schema = json!({"type":"object","properties":{"summary":{"type":"string","maxLength":4000},"questions":{"type":"array","items":{"type":"string","maxLength":1000},"maxItems":8,"description":"Missing BUSINESS DATA only. Ready action plans use []. The host UI asks for approval separately. When questions are nonempty, steps must be []."},"steps":steps},"required":["summary","questions","steps"],"additionalProperties":false});
    schema["properties"]["lookups"] = json!({"type":"array","maxItems":8,"description":"Optional authorized read expansion only: when nonempty, steps and questions must both be empty. Use only the current host read budget; request a lookup only when readExpansion.canRequestAnotherRead is true. Choose only available source module/collection pairs.","items":{"type":"object","properties":{"module":{"type":"string"},"collection":{"type":"string"},"query":{"type":"string","maxLength":160},"id":{"type":"string","maxLength":128}},"required":["module","collection"],"additionalProperties":false}});
    compact(&mut schema);
    schema
}
/// The same proposal schema with the read-expansion field removed, for a call
/// after which the host could not serve another read. The model is then
/// constrained to answer with a plan or questions instead of being told in
/// prose not to ask.
pub(super) fn without_reads(schema: &Value) -> Value {
    let mut closed = schema.clone();
    if let Some(properties) = closed["properties"].as_object_mut() {
        properties.remove("lookups");
    }
    closed
}
/// A repair only needs the already selected registered contracts. Small exact
/// branches avoid the shared-field ambiguity without the full catalogue grammar.
pub(super) fn correction_plan(contracts: &[Value]) -> Value {
    let shared = plan(contracts);
    if contracts.is_empty() || contracts.len() > 4 {
        return shared;
    }
    let template = shared["properties"]["steps"]["items"].clone();
    let alternatives = contracts
        .iter()
        .map(|contract| {
            let mut step = template.clone();
            step["properties"]["tool"] = json!({"type":"string","enum":[contract["tool"]]});
            step["properties"]["input"] = contract["inputSchema"].clone();
            step
        })
        .collect::<Vec<_>>();
    let mut exact = shared.clone();
    exact["properties"]["steps"]["items"] = json!({"anyOf":alternatives});
    compact(&mut exact);
    // Internal repair-size budget, not a published provider schema limit.
    if exact.to_string().len() <= 16_384 {
        exact
    } else {
        shared
    }
}
/// Describe every registered field once instead of repeating a complete step
/// branch per action. Explicit nested types matter: an unconstrained object can
/// cause providers to encode a result binding as a string rather than an object.
/// The selected tool's exact required/allowed fields remain host-validated.
fn shared_input(contracts: &[Value]) -> Value {
    let mut choices: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    for contract in contracts {
        for (name, schema) in contract["inputSchema"]["properties"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let mut schema = schema.clone();
            compact(&mut schema);
            let variants = choices.entry(name.clone()).or_default();
            if !variants.contains(&schema) {
                variants.push(schema);
            }
        }
    }
    let properties: Map<String, Value> = choices
        .into_iter()
        .map(|(name, mut variants)| {
            let schema = if variants.len() == 1 {
                variants.remove(0)
            } else {
                json!({"anyOf":variants})
            };
            (name, schema)
        })
        .collect();
    json!({"type":"object","properties":properties,"additionalProperties":false,"description":"Use only the fields of the selected actionContracts inputSchema, including all required fields. Every binding must be a nested object, not a string. Host validation enforces the exact selected contract."})
}
// Counting bounds multiply Gemini's decoding grammar, including each complete
// action schema for every allowed step. Keep structural types here; host/domain
// validation still enforces all length, count, money and dependency limits.
fn compact(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    for key in [
        "minimum",
        "maximum",
        "minItems",
        "maxItems",
        "minLength",
        "maxLength",
    ] {
        object.remove(key);
    }
    if let Some(fields) = object.get_mut("properties").and_then(Value::as_object_mut) {
        for value in fields.values_mut() {
            compact(value);
        }
    }
    if let Some(items) = object.get_mut("items") {
        compact(items);
    }
    if let Some(choices) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        for value in choices {
            compact(value);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crm_organization_label_contract_keeps_text_semantics_and_nullable_clear_distinct_from_entity_links(
    ) {
        let catalogue = contracts(&["crm".into(), "organizations".into()]);
        let update = catalogue
            .iter()
            .find(|c| c["tool"] == "crm.contact.update")
            .unwrap();
        assert!(update["description"]
            .as_str()
            .unwrap()
            .contains("not a managed Organization link"));
        let field = &update["inputSchema"]["properties"]["data"]["properties"]["organization"];
        assert_eq!(field["anyOf"][0]["type"], "string");
        assert_eq!(field["anyOf"][1]["type"], "null");
        assert!(field["description"]
            .as_str()
            .unwrap()
            .contains("Free-text organization label"));
        assert!(field["description"].as_str().unwrap().contains("new label"));
        let entity = catalogue
            .iter()
            .find_map(|c| c["inputSchema"]["properties"].get("organizationId"))
            .unwrap();
        assert!(entity["description"]
            .as_str()
            .unwrap()
            .contains("observed organizationId"));
        assert!(entity["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["properties"]["$from"].is_object()));
    }
    #[test]
    fn correction_schema_uses_the_selected_tools_exact_fields_and_keeps_full_catalogue_unchanged() {
        let catalogue = contracts(&["purchasing".into()]);
        let chosen = catalogue
            .iter()
            .find(|c| c["tool"] == "purchasing.offer.set-cost")
            .unwrap()
            .clone();
        let schema = correction_plan(std::slice::from_ref(&chosen));
        let step = &schema["properties"]["steps"]["items"]["anyOf"][0];
        assert_eq!(
            step["properties"]["tool"]["enum"],
            json!(["purchasing.offer.set-cost"])
        );
        assert_eq!(step["properties"]["input"]["additionalProperties"], false);
        assert!(step["properties"]["input"]["properties"]
            .get("currency")
            .is_none());
        assert!(step["properties"]["input"]["properties"]
            .get("productId")
            .is_none());
        assert_eq!(
            step["properties"]["input"]["required"],
            chosen["inputSchema"]["required"]
        );
        assert!(step["properties"]["input"]["properties"]["unitCost"]
            .get("maximum")
            .is_none());
        assert_eq!(catalogue, contracts(&["purchasing".into()]));
    }
    #[test]
    fn planner_contracts_omit_server_keys_and_restrict_each_tool_input() {
        let actions = contracts(&["purchasing".into(), "billing".into()]);
        assert!(!actions
            .iter()
            .any(|a| a["tool"] == "billing.record_provider_event"));
        let supplier = actions
            .iter()
            .find(|a| a["tool"] == "purchasing.supplier.create")
            .unwrap();
        assert_eq!(supplier["inputSchema"]["additionalProperties"], false);
        let invoice = actions
            .iter()
            .find(|a| a["tool"] == "billing.create_invoice")
            .unwrap();
        assert!(invoice["inputSchema"]["properties"]
            .get("commandId")
            .is_none());
        let offer = actions
            .iter()
            .find(|a| a["tool"] == "purchasing.offer.create")
            .unwrap();
        assert_eq!(
            offer["inputSchema"]["properties"]["unitCost"]["type"],
            "integer"
        );
        assert!(offer["inputSchema"]["properties"]["supplierId"]["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["properties"]["$from"].is_object()));
        let schema = plan(&actions);
        let step = &schema["properties"]["steps"]["items"];
        assert_eq!(
            step["properties"]["tool"]["enum"],
            json!(actions
                .iter()
                .map(|a| a["tool"].clone())
                .collect::<Vec<_>>())
        );
        assert_eq!(step["properties"]["input"]["type"], "object");
        assert_eq!(step["properties"]["input"]["additionalProperties"], false);
        assert_eq!(
            step["properties"]["input"]["properties"]["supplierId"]["anyOf"][1]["properties"]
                ["$from"]["type"],
            "object"
        );
        assert_eq!(step["additionalProperties"], false);
        let modules = archava_domain::operator::catalogue()["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let all = contracts(&modules);
        assert!(
            plan(&all).to_string().len() < 131072,
            "Installed action catalogue must keep a bounded decoding envelope"
        );
        assert!(schema["properties"]["steps"].get("maxItems").is_none());
        // Compact provider grammar must not alter the source contracts.
        assert_eq!(
            offer["inputSchema"]["properties"]["supplierId"]["anyOf"][1]["properties"]["$from"]
                ["properties"]["step"]["maximum"],
            11
        );
    }
    #[test]
    fn a_final_round_schema_offers_no_read_expansion_and_is_otherwise_unchanged() {
        let contracts = contracts(&["crm".to_string()]);
        for open in [plan(&contracts), correction_plan(&contracts[..1])] {
            assert!(open["properties"]["lookups"].is_object());
            let closed = without_reads(&open);
            assert!(closed["properties"].get("lookups").is_none());
            let mut expected = open.clone();
            expected["properties"]
                .as_object_mut()
                .unwrap()
                .remove("lookups");
            assert_eq!(closed, expected);
            assert_eq!(closed["required"], open["required"]);
        }
    }
    #[test]
    fn compact_preserves_field_names_while_removing_grammar_counting_bounds() {
        let mut schema = json!({"type":"object","properties":{"maxItems":{"type":"array","maxItems":100,"items":{"anyOf":[{"type":"integer","minimum":0},{"type":"string","maxLength":2000}]}}},"required":["maxItems"]});
        compact(&mut schema);
        assert!(schema["properties"]["maxItems"].is_object());
        assert!(schema["properties"]["maxItems"].get("maxItems").is_none());
        assert_eq!(
            schema["properties"]["maxItems"]["items"]["anyOf"],
            json!([{"type":"integer"},{"type":"string"}])
        );
    }
}
