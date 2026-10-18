//! Sensitive-field masking — port of `packages/acl/src/mask.ts`.
//!
//! §16: "Sensitive fields must be masked before model/provider exposure
//! according to policy." The rule is applied at one place — the boundary where
//! an internal value crosses into anything a provider or a model can see — so
//! that masking is a property of the boundary, not of each call site's memory.
//!
//! Design rule: lossy by default, loud when it fires. Every field that was
//! withheld is reported in `notices`, so a consumer can tell an answer was
//! redacted rather than silently wrong.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mask strategy for a single field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaskStrategy {
    Preserve,
    Redact,
    Partial,
    Tokenise,
}

impl MaskStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            MaskStrategy::Preserve => "preserve",
            MaskStrategy::Redact => "redact",
            MaskStrategy::Partial => "partial",
            MaskStrategy::Tokenise => "tokenise",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskRule {
    pub field: String,
    pub strategy: MaskStrategy,
    /// For `partial`: how many leading characters survive. Encoded in the
    /// baseline as `prefix`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MaskOptions {
    /// Field names to mask whenever they appear, at any depth.
    pub rules: Vec<MaskRule>,
    /// Escapes the default denylist for a name the caller has verified is safe.
    /// Serialized as `preserve` (the baseline's option of the same name).
    #[serde(rename = "preserve", default)]
    pub preserve: Vec<String>,
}

/// Label substituted for a withheld value.
pub const REDACTED_LABEL: &str = "[redacted]";

/// Fields that must never be withheld outright when a partial view suffices.
const PARTIAL_FIELDS: &[&str] = &[
    "email",
    "customeremail",
    "phone",
    "phonenumber",
    "customerphone",
];

/// Substring-triggered denylist. A name containing one of these is sensitive.
const SENSITIVE_FRAGMENTS: &[&str] = &[
    "password",
    "passphrase",
    "secret",
    "token",
    "apikey",
    "authorization",
    "cookie",
    "cvv",
    "cvc",
    "cardnumber",
    "card_number",
    "taxid",
    "nationalid",
    "passport",
    "ssn",
    "otp",
    "bankaccount",
    "bank_account",
    "paymentinstrument",
    "pin",
];

/// Whole-name matches that get a partial mask (identifiable but usable).
const SENSITIVE_NAMES: &[&str] = &["email", "phone", "address", "dob", "dateofbirth", "rawform"];

fn is_plain_object(value: &Value) -> bool {
    matches!(value, Value::Object(_))
}

/// FNV-1a over the value's UTF-16 code units — exactly the baseline's hash,
/// which iterates `charCodeAt`. Not cryptographic: it only proves a value was
/// replaced, never to authenticate or identify anyone.
pub fn stable_hash(value: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in value.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{:08x}", hash)
}

fn looks_like_email(value: &str) -> bool {
    // `^[^\s@]+@[^\s@]+\.[^\s@]+$` — non-space, non-`@` on both sides of one
    // `@`, with at least one `.` in the domain part and a tail of two or more.
    let valid_side = |text: &str| -> bool {
        !text.is_empty() && text.chars().all(|c| !(c.is_whitespace() || c == '@'))
    };
    let Some(at) = value.find('@') else {
        return false;
    };
    let (local, domain) = value.split_at(at);
    let domain = &domain[1..];
    if !valid_side(local) || !valid_side(domain) || domain.contains('@') {
        return false;
    }
    let Some(dot) = domain.rfind('.') else {
        return false;
    };
    domain[dot + 1..].chars().count() >= 2
}

fn classify_field(field: &str, rules: &[MaskRule]) -> Option<MaskStrategy> {
    let explicit = rules.iter().find(|rule| rule.field == field);
    if let Some(rule) = explicit {
        return Some(rule.strategy);
    }

    let lower = field.to_lowercase();
    if PARTIAL_FIELDS.contains(&lower.as_str()) {
        return Some(MaskStrategy::Partial);
    }
    if SENSITIVE_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(fragment))
    {
        return Some(MaskStrategy::Redact);
    }
    if SENSITIVE_NAMES.contains(&lower.as_str()) {
        return Some(MaskStrategy::Partial);
    }
    None
}

fn prefix_for(strategy: MaskStrategy, rule: &MaskRule, value: &str) -> usize {
    if strategy != MaskStrategy::Partial {
        return 0;
    }
    if let Some(prefix) = rule.prefix {
        return prefix.max(1);
    }
    if looks_like_email(value) {
        2
    } else {
        3
    }
}

fn mask_value(value: &str, rule: &MaskRule, path: &str, notices: &mut Vec<String>) -> String {
    let strategy = rule.strategy;
    if strategy == MaskStrategy::Preserve {
        return value.to_string();
    }
    if strategy == MaskStrategy::Tokenise {
        notices.push(format!("{path} tokenised to a non-reversible reference"));
        return format!("tok_{}", stable_hash(value));
    }
    if strategy == MaskStrategy::Partial {
        let prefix = prefix_for(strategy, rule, value);
        // The baseline slices by UTF-16 code units; the port slices whole
        // characters, so a prefix that would cut a multi-byte character
        // halfway through keeps that character — the safe superset of a
        // half-slice, and the withheld count follows from the visible text
        // that actually survives.
        let units = value.encode_utf16().count();
        let mut visible = String::new();
        let mut spent = 0usize;
        for character in value.chars() {
            let character_units = character.len_utf16();
            if spent + character_units <= prefix {
                spent += character_units;
                visible.push(character);
            } else {
                break;
            }
        }
        let withheld = units - visible.encode_utf16().count();
        notices.push(format!(
            "{path} partially masked ({withheld} character{plural} withheld)",
            plural = if withheld == 1 { "" } else { "s" }
        ));
        if withheld == 0 {
            return format!("{visible}\u{2026}");
        }
        return format!("{visible}{}", "\u{2022}".repeat(withheld.min(12)));
    }
    notices.push(format!("{path} redacted before provider exposure"));
    REDACTED_LABEL.to_string()
}

/// Recursively mask `input` (untrusted shape), returning a new value. The input
/// is never mutated — masking a payload must not corrupt the record the caller
/// still holds.
///
/// Values arrive as JSON; the baseline walked plain objects, arrays and
/// primitives with `typeof`, and the mapping here is:
///
/// | TypeScript | Rust |
/// | --- | --- |
/// | object (not array) | `Value::Object` |
/// | array | `Value::Array` |
/// | string / number / boolean / null | same |
/// | `undefined` | (cannot appear in `serde_json`; constructed fields omit) |
pub fn mask_sensitive_fields(input: &Value, options: &MaskOptions) -> MaskedResult {
    let preserved: Vec<&str> = options.preserve.iter().map(|name| name.as_str()).collect();
    let mut notices = Vec::new();
    let masked = walk(
        input,
        "$",
        options.rules.as_slice(),
        &preserved,
        &mut notices,
    );
    let any_masked = !notices.is_empty();
    MaskedResult {
        value: masked,
        notices,
        masked: any_masked,
    }
}

fn walk(
    value: &Value,
    path: &str,
    rules: &[MaskRule],
    preserved: &[&str],
    notices: &mut Vec<String>,
) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    walk(item, &format!("{path}[{index}]"), rules, preserved, notices)
                })
                .collect(),
        ),
        Value::Object(object) => {
            let mut output = serde_json::Map::new();
            for (key, child) in object {
                let child_path = format!("{path}.{key}");
                if preserved.contains(&key.as_str()) {
                    output.insert(
                        key.clone(),
                        walk(child, &child_path, rules, preserved, notices),
                    );
                    continue;
                }
                let Some(strategy) = classify_field(key, rules) else {
                    output.insert(
                        key.clone(),
                        walk(child, &child_path, rules, preserved, notices),
                    );
                    continue;
                };
                if is_plain_object(child) || matches!(child, Value::Array(_)) {
                    notices.push(format!(
                        "{child_path} withheld before provider exposure (structured value)"
                    ));
                    output.insert(key.clone(), Value::String(REDACTED_LABEL.to_string()));
                    continue;
                }
                if let Value::String(text) = child {
                    let rule = MaskRule {
                        field: key.clone(),
                        strategy,
                        prefix: rules
                            .iter()
                            .find(|explicit| explicit.field == *key)
                            .and_then(|explicit| explicit.prefix),
                    };
                    output.insert(
                        key.clone(),
                        Value::String(mask_value(text, &rule, &child_path, notices)),
                    );
                    continue;
                }
                if matches!(child, Value::Null) {
                    output.insert(key.clone(), Value::Null);
                    continue;
                }
                // A boolean is a flag, not a credential: no secret in this
                // domain is a boolean, and withholding one destroys signal the
                // model needs — `integrationTokensEnabled: [redacted]` reads as
                // a withheld secret. Numbers still count, because a numeric OTP
                // or PIN is possible.
                if matches!(child, Value::Bool(_)) {
                    output.insert(key.clone(), child.clone());
                    continue;
                }
                notices.push(format!(
                    "{child_path} withheld before provider exposure (non-text value)"
                ));
                output.insert(key.clone(), Value::String(REDACTED_LABEL.to_string()));
            }
            Value::Object(output)
        }
        _ => value.clone(),
    }
}

/// The result of masking: the new value, what was withheld, and whether
/// anything was.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskedResult {
    pub value: Value,
    /// Human-readable notes for every field that was withheld.
    pub notices: Vec<String>,
    /// True when at least one value was withheld.
    pub masked: bool,
}

/// Mask a single string value, for call sites that hold one field.
pub fn mask_string(value: &str, field: &str, options: &MaskOptions) -> MaskedResult {
    let preserved: Vec<&str> = options.preserve.iter().map(|name| name.as_str()).collect();
    if preserved.contains(&field) {
        return MaskedResult {
            value: Value::String(value.to_string()),
            notices: Vec::new(),
            masked: false,
        };
    }
    let strategy = classify_field(field, options.rules.as_slice()).unwrap_or(MaskStrategy::Redact);
    let mut notices = Vec::new();
    let rule = MaskRule {
        field: field.to_string(),
        strategy,
        prefix: None,
    };
    let masked = mask_value(value, &rule, &format!("$.{field}"), &mut notices);
    let changed = masked != value;
    MaskedResult {
        value: Value::String(masked),
        notices,
        masked: changed,
    }
}

/// Merge an action's declared sensitive fields with the platform defaults into
/// mask rules. Action declarations (§18) are the primary source; the platform
/// contributes fields that must never be exposed regardless of what an action
/// remembered to declare.
pub fn resolve_mask_rules(
    action_sensitive_fields: &[String],
    client_sensitive_fields: &[String],
) -> Vec<MaskRule> {
    let mut fields: Vec<String> = Vec::new();
    let mut push = |field: &str| {
        if !fields.contains(&field.to_string()) {
            fields.push(field.to_string());
        }
    };
    for name in SENSITIVE_NAMES {
        push(name);
    }
    for name in PARTIAL_FIELDS {
        push(name);
    }
    for name in action_sensitive_fields {
        push(name);
    }
    for name in client_sensitive_fields {
        push(name);
    }
    fields
        .iter()
        .map(|field| MaskRule {
            field: field.clone(),
            strategy: if PARTIAL_FIELDS.contains(&field.to_lowercase().as_str()) {
                MaskStrategy::Partial
            } else {
                MaskStrategy::Redact
            },
            prefix: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_password_is_redacted_and_renamed_shards_are_found() {
        let result = mask_sensitive_fields(&json!({"password": "s3cr3t"}), &MaskOptions::default());
        assert_eq!(result.value, json!({"password": "[redacted]"}));
        assert!(result.masked);
        assert_eq!(result.notices.len(), 1);
    }

    #[test]
    fn an_email_is_partially_visible_and_an_api_key_is_redacted() {
        let result = mask_sensitive_fields(
            &json!({"email": "ada@example.com", "apiKey": "sk-123456"}),
            &MaskOptions::default(),
        );
        // email: the PARTIAL default survives the first 2 characters of an
        // email-shaped value...
        assert_eq!(result.value["email"], "ad••••••••••••");
        // apiKey: fragment rule.
        assert_eq!(result.value["apiKey"], "[redacted]");
    }

    #[test]
    fn tokenise_hashes_deterministically() {
        let mut options = MaskOptions::default();
        options.rules.push(MaskRule {
            field: "referee".to_string(),
            strategy: MaskStrategy::Tokenise,
            prefix: None,
        });
        let result = mask_sensitive_fields(&json!({"referee": "acme"}), &options);
        assert_eq!(
            result.value["referee"],
            format!("tok_{}", stable_hash("acme"))
        );
        assert_eq!(stable_hash("acme"), stable_hash("acme"));
        assert_ne!(stable_hash("acme"), stable_hash("acme2"));
    }

    #[test]
    fn booleans_and_nulls_pass_through_numbers_do_not() {
        let result = mask_sensitive_fields(
            &json!({"otpEnabled": true, "otp": 123456, "memo": null}),
            &MaskOptions::default(),
        );
        assert_eq!(result.value["otpEnabled"], true);
        assert_eq!(result.value["otp"], "[redacted]");
        assert_eq!(result.value["memo"], Value::Null);
        // The boolean made no notice; the number did.
        assert_eq!(result.notices.len(), 1);
    }

    #[test]
    fn a_preserved_name_is_walked_not_refused() {
        let mut options = MaskOptions::default();
        options.preserve.push("tokenSecret".to_string());
        let result = mask_sensitive_fields(&json!({"tokenSecret": []}), &options);
        assert_eq!(result.value["tokenSecret"], json!([]));
        assert!(!result.masked);
    }

    #[test]
    fn resolve_defaults_list_the_platform_fields() {
        let rules = resolve_mask_rules(&["contactEmail".to_string()], &[]);
        assert!(rules
            .iter()
            .any(|rule| rule.field == "email" && rule.strategy == MaskStrategy::Partial));
        // The action-declared field rides along as a redact (it is not in the
        // partial set).
        assert!(rules
            .iter()
            .any(|rule| rule.field == "contactEmail" && rule.strategy == MaskStrategy::Redact));
        // The partial set's members stay partial wherever they came from, and
        // the rule's field is the field string as it was pushed.
        assert!(rules
            .iter()
            .any(|rule| rule.field == "customerphone" && rule.strategy == MaskStrategy::Partial));
    }
}
