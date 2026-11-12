//! The business-module registry: one descriptor per installed module.
//!
//! `config/modules.v1.json` is canonical; the byte-identical snapshot under
//! `embedded/` is what this crate compiles in (`tools/rust/domain-assets.mjs`
//! keeps the two equal). A descriptor names a module's identity, its ledger,
//! where its source lives and what the agent may route to and read from it.
//! It is data about wiring, never a grant: every domain transaction still
//! re-authorizes its own action.
//!
//! Tables that used to be hand-maintained per module read from here instead,
//! and `tools/check-modules.mjs` verifies the registration points that cannot
//! be derived (module declarations, routes, embedded migrations).
use serde_json::Value;
use std::sync::OnceLock;

/// One installed business module, as declared in `config/modules.v1.json`.
#[derive(Debug)]
pub struct Module {
    /// Stable public id (`laundry`, `field`): the `/erp/<id>/…` route segment.
    pub id: &'static str,
    pub label: &'static str,
    /// PostgreSQL schema that owns the module's tables.
    pub namespace: &'static str,
    /// Directory under `migrations/`.
    pub ledger: &'static str,
    /// File stem under `erp/` in the domain, adapters and host crates.
    pub source: &'static str,
    /// Position in agent keyword routing; lower ranks are matched first.
    pub rank: u32,
    /// Lower-case phrases that route an utterance to this module.
    pub keywords: &'static [&'static str],
    /// Response collections the provider projection may read. Anything not
    /// listed stays withheld.
    pub collections: &'static [&'static str],
}

fn leak(value: &Value, field: &str) -> &'static str {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("embedded module registry: `{field}` must be a string"));
    Box::leak(text.to_owned().into_boxed_str())
}

fn leak_list(value: &Value, field: &str) -> &'static [&'static str] {
    let items = value
        .as_array()
        .unwrap_or_else(|| panic!("embedded module registry: `{field}` must be an array"));
    Box::leak(
        items
            .iter()
            .map(|item| leak(item, field))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// Every installed module, in catalogue order.
pub fn all() -> &'static [Module] {
    static DATA: OnceLock<Vec<Module>> = OnceLock::new();
    DATA.get_or_init(|| {
        let root: Value = serde_json::from_str(include_str!("../embedded/modules.v1.json"))
            .expect("embedded module registry must be valid JSON");
        root["modules"]
            .as_array()
            .expect("embedded module registry must list modules")
            .iter()
            .map(|m| Module {
                id: leak(&m["id"], "id"),
                label: leak(&m["label"], "label"),
                namespace: leak(&m["namespace"], "namespace"),
                ledger: leak(&m["ledger"], "ledger"),
                source: leak(&m["source"], "source"),
                rank: m["agent"]["rank"]
                    .as_u64()
                    .and_then(|rank| u32::try_from(rank).ok())
                    .expect("embedded module registry: `agent.rank` must be a small integer"),
                keywords: leak_list(&m["agent"]["keywords"], "agent.keywords"),
                collections: leak_list(&m["agent"]["collections"], "agent.collections"),
            })
            .collect()
    })
}

/// The module with this public id, if it is installed.
pub fn get(id: &str) -> Option<&'static Module> {
    all().iter().find(|module| module.id == id)
}

/// Modules in agent routing order (ascending `rank`).
pub fn routing() -> &'static [&'static Module] {
    static ORDER: OnceLock<Vec<&'static Module>> = OnceLock::new();
    ORDER.get_or_init(|| {
        let mut order: Vec<&'static Module> = all().iter().collect();
        order.sort_by_key(|module| module.rank);
        order
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_embedded_registry_is_well_formed_and_unambiguous() {
        let modules = all();
        assert!(!modules.is_empty());
        let ids: BTreeSet<_> = modules.iter().map(|m| m.id).collect();
        let ranks: BTreeSet<_> = modules.iter().map(|m| m.rank).collect();
        let namespaces: BTreeSet<_> = modules.iter().map(|m| m.namespace).collect();
        assert_eq!(ids.len(), modules.len(), "module ids must be unique");
        assert_eq!(ranks.len(), modules.len(), "routing ranks must be unique");
        assert_eq!(
            namespaces.len(),
            modules.len(),
            "each module owns one schema"
        );
        for module in modules {
            assert!(module.namespace.starts_with("archava_"), "{}", module.id);
            assert!(!module.keywords.is_empty(), "{}", module.id);
            assert!(!module.collections.is_empty(), "{}", module.id);
            for word in module.keywords {
                assert_eq!(*word, word.to_lowercase(), "{} keyword case", module.id);
            }
            assert_eq!(get(module.id).map(|m| m.namespace), Some(module.namespace));
        }
        assert!(get("unknown").is_none());
    }

    #[test]
    fn routing_order_follows_rank_not_catalogue_position() {
        let order = routing();
        assert_eq!(order.len(), all().len());
        assert!(order.windows(2).all(|pair| pair[0].rank < pair[1].rank));
    }

    #[test]
    fn every_module_has_a_form_catalogue_entry_in_the_same_schema() {
        for module in all() {
            let forms = crate::operator::module(module.id)
                .unwrap_or_else(|| panic!("{} has no operator form catalogue", module.id));
            assert_eq!(forms["namespace"], module.namespace, "{}", module.id);
        }
    }
}
