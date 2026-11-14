//! Client-owned transport port: shared engines come only from packaged public crates.
use archava_core::tenant::TenantId;
use archava_domain::erp::crm::{
    text_len, transition_allowed, valid_email, ContactFields, PipelineStage,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub tenant: String,
    pub model: String,
    pub config_revision: u64,
    pub pipeline_labels: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub request_id: String,
    pub module: String,
    pub operation: String,
    pub values: Value,
    pub observed_stage: Option<String>,
}
pub fn prepare(
    config: &Config,
    input: Input,
    model: &str,
    extension: fn(&str, &str, &Value) -> Result<(), String>,
) -> Result<Value, String> {
    TenantId::new(&config.tenant).map_err(|_| "invalid_config_tenant")?;
    if config.model != model || config.config_revision == 0 {
        return Err("unreviewed_consumer_config".into());
    }
    for stage in ["new", "qualified", "proposal", "won", "lost"] {
        if !config
            .pipeline_labels
            .get(stage)
            .is_some_and(|s| text_len(s, 1, 80))
        {
            return Err("incomplete_stage_configuration".into());
        }
    }
    if input.module == "crm" {
        match input.operation.as_str() {
            "contact.create" => {
                let fields: ContactFields = serde_json::from_value(input.values["data"].clone())
                    .map_err(|_| "invalid_contact")?;
                if !text_len(&fields.full_name, 1, 160)
                    || fields.email.as_ref().is_some_and(|s| !valid_email(s))
                {
                    return Err("invalid_contact".into());
                }
            }
            "opportunity.create" => {}
            "opportunity.move" => {
                let from = input
                    .observed_stage
                    .as_deref()
                    .and_then(PipelineStage::parse)
                    .ok_or("missing_observed_stage")?;
                let to = input.values["stage"]
                    .as_str()
                    .and_then(PipelineStage::parse)
                    .ok_or("invalid_canonical_stage")?;
                if !transition_allowed(from, to) {
                    return Err("shared_crm_transition_refused".into());
                }
            }
            _ => return Err("consumer_operation_not_offered".into()),
        }
    } else {
        extension(&input.module, &input.operation, &input.values)?
    }
    let request = archava_domain::operator::envelope(
        &input.module,
        &config.tenant,
        &input.request_id,
        &input.operation,
        &input.values,
    )?;
    let stage = input.values["stage"].as_str().unwrap_or("new");
    Ok(
        json!({"module":input.module,"request":request,"view":{"businessModel":model,"canonicalStage":stage,"stageLabel":config.pipeline_labels.get(stage),"configurationRevision":config.config_revision},"authorization":"The native serving application must resolve current identity, tenant, grants, scope and revisions"}),
    )
}
pub fn run(model: &str, extension: fn(&str, &str, &Value) -> Result<(), String>) {
    let result = (|| -> Result<Value, String> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 2 || args[0] != "--config" {
            return Err("usage: --config CONFIG_FILE".into());
        }
        let config: Config =
            serde_json::from_slice(&std::fs::read(&args[1]).map_err(|_| "config_unavailable")?)
                .map_err(|_| "invalid_config")?;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| "input_unavailable")?;
        if bytes.len() > 65536 {
            return Err("input_too_large".into());
        }
        let input: Input = serde_json::from_slice(&bytes).map_err(|_| "invalid_input")?;
        prepare(&config, input, model, extension)
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            println!("{}", json!({"refused":error}));
            std::process::exit(1)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cfg() -> Config {
        serde_json::from_value(json!({"tenant":"client","model":"test","configRevision":1,"pipelineLabels":{"new":"Inquiry","qualified":"Qualified","proposal":"Quoted","won":"Accepted","lost":"Declined"}})).unwrap()
    }
    fn no_extension(_: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("no_extension".into())
    }
    #[test]
    fn shipped_crm_engine_refuses_stage_skips_even_under_custom_labels() {
        let c = cfg();
        let input=serde_json::from_value(json!({"requestId":"11111111-1111-4111-8111-111111111111","module":"crm","operation":"opportunity.move","values":{"id":"22222222-2222-4222-8222-222222222222","expectedRevision":1,"stage":"won"},"observedStage":"new"})).unwrap();
        assert_eq!(
            prepare(&c, input, "test", no_extension).unwrap_err(),
            "shared_crm_transition_refused"
        );
    }
    #[test]
    fn principal_claims_and_unoffered_extensions_do_not_become_authority() {
        assert!(serde_json::from_value::<Input>(json!({"requestId":"x","module":"crm","operation":"contact.create","values":{},"actorId":"owner"})).is_err());
        let input = serde_json::from_value(
            json!({"requestId":"x","module":"accounting","operation":"journal.post","values":{}}),
        )
        .unwrap();
        assert_eq!(
            prepare(&cfg(), input, "test", no_extension).unwrap_err(),
            "no_extension"
        );
    }
}
