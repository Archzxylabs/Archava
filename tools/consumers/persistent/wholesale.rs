mod shared;
use serde_json::{json, Value};
fn extension(module: &str, operation: &str, values: &Value) -> Result<(), String> {
    if module != "commerce" || operation != "order.create_from_contact" {
        return Err("wholesale_extension_not_offered".into());
    }
    let mut command = values.clone();
    command["operation"] = json!(operation);
    let command: archava_domain::erp::commerce::CommerceCommand =
        serde_json::from_value(command).map_err(|_| "shared_order_contract_refused")?;
    command
        .validate()
        .map_err(|_| "shared_order_contract_refused".to_string())?;
    Ok(())
}
fn main() {
    shared::run("wholesale", extension)
}
