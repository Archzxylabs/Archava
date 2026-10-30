//! Receipt acknowledgement is a wire contract, never just an HTTP success code.
use serde_json::Value;
fn uuid_shape(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
pub fn crm(body: &Value, key: &str, kind: Option<&str>) -> bool {
    let r = &body["receipt"];
    (body["decision"] == "receipt" || body["status"] == "committed")
        && (body["requestId"]
            .as_str()
            .or_else(|| body["request_id"].as_str()))
            == Some(key)
        && body["requestId"].as_str().is_none_or(|id| id == key)
        && body["request_id"].as_str().is_none_or(|id| id == key)
        && r["id"].as_str().is_some_and(uuid_shape)
        && r["revision"].as_i64().is_some_and(|n| n > 0)
        && r["kind"].as_str().is_some_and(|k| {
            matches!(k, "contact" | "activity" | "followup" | "opportunity")
                && kind.is_none_or(|expected| k == expected)
        })
}
pub fn owner(body: &Value, key: &str) -> bool {
    let r = &body["receipt"];
    body["decision"] == "receipt"
        && r["requestId"] == key
        && r["revision"].as_i64().is_some_and(|n| n > 0)
        && r["outcome"]
            .as_str()
            .is_some_and(|outcome| matches!(outcome, "apply" | "restore"))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "8b1c2f3a-0000-4000-8000-000000000001";
    #[test]
    fn crm_requires_the_original_key_typed_resource_and_positive_revision() {
        let good = json!({"decision":"receipt","request_id":ID,"receipt":{"kind":"followup","id":ID,"revision":1}});
        assert!(crm(&good, ID, Some("followup")));
        assert!(!crm(&good, "other", None));
        assert!(!crm(&good, ID, Some("contact")));
        let mut contradictory = good.clone();
        contradictory["requestId"] = json!("foreign");
        assert!(!crm(&contradictory, ID, None));
        for field in ["id", "revision", "kind"] {
            let mut bad = good.clone();
            bad["receipt"].as_object_mut().unwrap().remove(field);
            assert!(!crm(&bad, ID, None));
        }
        let mut status = good;
        status.as_object_mut().unwrap().remove("decision");
        status["status"] = json!("committed");
        status["requestId"] = json!(ID);
        status.as_object_mut().unwrap().remove("request_id");
        assert!(crm(&status, ID, None));
        assert!(!crm(
            &json!({"status":"committed","requestId":ID}),
            ID,
            None
        ));
    }
    #[test]
    fn owner_requires_its_original_operation_and_committed_revision() {
        let good =
            json!({"decision":"receipt","receipt":{"requestId":ID,"revision":1,"outcome":"apply"}});
        assert!(owner(&good, ID));
        assert!(!owner(&good, "other"));
        assert!(!owner(&json!({"decision":"receipt"}), ID));
    }
}
