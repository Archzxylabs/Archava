//! Explicit business-data disclosure configuration. This never grants record access.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Closed categories; unknown fields are not implicitly approved for a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    ConversationText,
    RecordReferences,
    BusinessFacts,
    Names,
    ContactDetails,
    Notes,
    FinancialAmounts,
}
impl DataClass {
    pub const ALL: [Self; 7] = [
        Self::ConversationText,
        Self::RecordReferences,
        Self::BusinessFacts,
        Self::Names,
        Self::ContactDetails,
        Self::Notes,
        Self::FinancialAmounts,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ConversationText => "Current messages you send Ava",
            Self::RecordReferences => "Record references and versions",
            Self::BusinessFacts => "Statuses, quantities and dates",
            Self::Names => "Names, titles and business codes",
            Self::ContactDetails => "Contact details",
            Self::Notes => "Free-text record notes",
            Self::FinancialAmounts => "Prices, costs and financial amounts",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::ConversationText => "The current instruction you type can contain any details you include. This is separate from automatically retrieved record fields. Past conversation text and saved preferences are withheld.",
            Self::RecordReferences => "Identifiers and versions used to select and review existing records.",
            Self::BusinessFacts => "Operational state, quantities, units, currencies and dates from approved records.",
            Self::Names => "Customer and supplier names, item titles, company names and record codes.",
            Self::ContactDetails => "Contact fields, with the boundary's supported email and phone masking.",
            Self::Notes => "Notes, descriptions and memos can contain names, amounts or other information written into them. Review what your business stores in free text before including this category.",
            Self::FinancialAmounts => "Exact prices, costs and journal amounts. Company financial records require an explicitly selected company and current financial read access.",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConversationText => "conversation_text",
            Self::RecordReferences => "record_references",
            Self::BusinessFacts => "business_facts",
            Self::Names => "names",
            Self::ContactDetails => "contact_details",
            Self::Notes => "notes",
            Self::FinancialAmounts => "financial_amounts",
        }
    }
}

/// The owner approves this exact snapshot for one installed provider profile.
/// Its binding is opaque, not a destination URL, API key, role or entitlement.
/// Empty company IDs grant no company-owned data. Tenant-wide/unassigned records
/// require their separate explicit flag as well as module/category approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisclosurePolicy {
    pub provider_binding: String,
    pub modules: BTreeSet<String>,
    pub data_classes: BTreeSet<DataClass>,
    pub companies: BTreeSet<String>,
    #[serde(default)]
    pub include_workspace_records: bool,
}
impl DisclosurePolicy {
    /// Structural/catalogue validity only. The host must resolve each company,
    /// current management authority and installed provider binding independently.
    pub fn valid_for(&self, installed: &BTreeSet<String>) -> bool {
        self.provider_binding.len() == 64
            && self
                .provider_binding
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && self.modules.len() <= 32
            && self.modules.is_subset(installed)
            && self.companies.len() <= 100
            && self.companies.iter().all(|id| canonical_company_id(id))
            && (!self.modules.contains("accounting")
                || (self.data_classes.contains(&DataClass::FinancialAmounts)
                    && !self.companies.is_empty()))
    }
    pub fn shares(&self, module: &str, class: DataClass) -> bool {
        self.modules.contains(module) && self.data_classes.contains(&class)
    }
    pub fn company_allowed(&self, company: Option<&str>) -> bool {
        match company {
            Some(id) => self.companies.contains(id),
            None => self.include_workspace_records,
        }
    }
}
fn canonical_company_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(n, b)| {
            if matches!(n, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

/// Omission is not revocation: both native forms and restore carry an explicit
/// change, so a prior config without disclosure cannot accidentally merge a grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DisclosureChange {
    Replace { policy: DisclosurePolicy },
    Revoke {},
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn policy() -> DisclosurePolicy {
        serde_json::from_value(json!({"providerBinding":"a".repeat(64),"modules":["commerce"],"dataClasses":["record_references","business_facts"],"companies":["00000000-0000-4000-8000-000000000001"]})).unwrap()
    }
    #[test]
    fn categories_modules_and_company_scope_are_independent_and_never_default_to_all() {
        let p = policy();
        let installed = BTreeSet::from(["commerce".into(), "accounting".into()]);
        assert!(p.valid_for(&installed));
        assert!(p.shares("commerce", DataClass::BusinessFacts));
        assert!(!p.shares("accounting", DataClass::BusinessFacts));
        assert!(!p.shares("commerce", DataClass::ContactDetails));
        assert!(p.company_allowed(Some("00000000-0000-4000-8000-000000000001")));
        assert!(!p.company_allowed(Some("00000000-0000-4000-8000-000000000002")));
        assert!(!p.company_allowed(None));
        let mut empty = p.clone();
        empty.companies.clear();
        assert!(!empty.company_allowed(Some("00000000-0000-4000-8000-000000000001")));
        empty.include_workspace_records = true;
        assert!(empty.company_allowed(None));
        assert!(!empty.company_allowed(Some("00000000-0000-4000-8000-000000000001")));
    }
    #[test]
    fn unregistered_modules_bad_bindings_noncanonical_scope_and_authority_claims_refuse() {
        let p = policy();
        let installed = BTreeSet::from(["commerce".into(), "accounting".into()]);
        let mut bad = p.clone();
        bad.modules.insert("not-installed".into());
        assert!(!bad.valid_for(&installed));
        bad = p.clone();
        bad.provider_binding = "https://example.com/provider".into();
        assert!(!bad.valid_for(&installed));
        bad = p.clone();
        bad.companies.insert("not-a-company".into());
        assert!(!bad.valid_for(&installed));
        bad = p.clone();
        bad.modules.insert("accounting".into());
        assert!(!bad.valid_for(&installed));
        bad.data_classes.insert(DataClass::FinancialAmounts);
        assert!(bad.valid_for(&installed));
        let mut body = serde_json::to_value(&p).unwrap();
        body["approved"] = json!(true);
        assert!(serde_json::from_value::<DisclosurePolicy>(body).is_err());
        assert!(serde_json::from_value::<DisclosureChange>(
            json!({"kind":"revoke","approved":true})
        )
        .is_err());
        let mut body = serde_json::to_value(&p).unwrap();
        body["dataClasses"] = json!(["everything"]);
        assert!(serde_json::from_value::<DisclosurePolicy>(body).is_err());
    }
}
