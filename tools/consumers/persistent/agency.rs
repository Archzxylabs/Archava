mod shared;
use serde_json::{json, Value};
fn extension(module: &str, operation: &str, values: &Value) -> Result<(), String> {
    if module != "projects" || operation != "project.create" {
        return Err("agency_extension_not_offered".into());
    }
    let mut command = values.clone();
    command["operation"] = json!(operation);
    archava_domain::erp::projects::parse_command(&command)
        .map_err(|_| "shared_project_contract_refused".to_string())?;
    Ok(())
}
fn main() {
    shared::run("agency", extension)
}
