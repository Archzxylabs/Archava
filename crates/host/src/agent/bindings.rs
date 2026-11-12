//! Reviewed references to earlier verified business receipts. No templates, code or monetary bindings.
use archava_cockpit::{FlightStep, Mission, StepStatus};
use serde_json::Value;
pub(crate) fn resolve(
    mission: &Mission,
    current: &FlightStep,
    mut input: Value,
) -> Result<Value, &'static str> {
    let version = input
        .as_object_mut()
        .ok_or("Invalid business input")?
        .remove("__agent_bindings_version");
    if version != Some(serde_json::json!(1)) {
        return Err("Unsupported agent binding version");
    }
    fn walk(
        m: &Mission,
        s: &FlightStep,
        v: &mut Value,
        field: Option<&str>,
    ) -> Result<(), &'static str> {
        if let Some(binding) = v.get("$from") {
            let key = field.ok_or("A binding needs an input field")?;
            let revision = key == "revision" || key.ends_with("Revision");
            if !(revision || key == "id" || key.ends_with("Id"))
                || v.as_object().is_none_or(|o| o.len() != 1)
            {
                return Err("Only record IDs and revisions may bind to prior results");
            }
            let index = binding["step"].as_u64().ok_or("Invalid prior step")? as usize;
            if !s.depends_on_steps.contains(&index) {
                return Err("Binding source must be an explicit dependency");
            }
            let prior = m
                .steps
                .iter()
                .find(|p| p.step_index == index)
                .ok_or("Missing binding source")?;
            if prior.status != StepStatus::Passed
                || crate::business::tool(&prior.tool_target).is_none()
            {
                return Err("Binding source is not a completed business step");
            }
            let mut value = prior
                .payload_output
                .as_ref()
                .filter(|p| p["verified"] == true)
                .ok_or("Prior receipt is not verified")?;
            let path = binding["path"]
                .as_array()
                .filter(|p| !p.is_empty() && p.len() <= 6)
                .ok_or("Invalid receipt path")?;
            for part in path {
                let name = part
                    .as_str()
                    .filter(|s| {
                        s.len() <= 80 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    })
                    .ok_or("Invalid receipt key")?;
                value = value.get(name).ok_or("Receipt field is absent")?;
            }
            if revision {
                if value.as_i64().is_none_or(|n| n < 1) {
                    return Err("Receipt revision is invalid");
                }
            } else if value.as_str().is_none_or(|s| s.is_empty() || s.len() > 160) {
                return Err("Receipt record reference is invalid");
            }
            *v = value.clone();
            return Ok(());
        }
        match v {
            Value::Object(o) => {
                for (k, v) in o {
                    walk(m, s, v, Some(k))?
                }
            }
            Value::Array(a) => {
                for v in a {
                    walk(m, s, v, field)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(mission, current, &mut input, None)?;
    Ok(input)
}
