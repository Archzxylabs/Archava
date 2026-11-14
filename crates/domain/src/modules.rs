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
    /// Label of the module's tab in the Operations workspace, for modules
    /// served by the generic operator workspace. Modules with a dedicated
    /// view have none.
    pub tab: Option<&'static str>,
    /// Whether the workspace read accepts a record id for a typed detail view.
    pub typed_detail: bool,
    /// Position in agent keyword routing; lower ranks are matched first.
    pub rank: u32,
    /// Lower-case phrases that route an utterance to this module.
    pub keywords: &'static [&'static str],
    /// Response collections the provider projection may read. Anything not
    /// listed stays withheld.
    pub collections: &'static [&'static str],
    /// Aggregate lookups the agent may request: a closed list per collection.
    /// Anything not named here is refused.
    pub aggregates: &'static [Aggregate],
    /// Other words an owner may call this module by when configuring the
    /// workspace in chat (`hotel`, `penginapan`). The id always counts.
    pub owner_aliases: &'static [&'static str],
    /// Module ids that must be enabled for this one to be enabled. Empty means
    /// the module needs only the mandatory base.
    pub owner_requires: &'static [&'static str],
}

/// One collection the agent may ask a count or a sum over, as declared under
/// `agent.aggregates`. The names are public vocabulary; the SQL behind each
/// one is fixed in the adapters (`archava_adapters::erp::aggregates`), which
/// refuses a name it does not implement.
#[derive(Debug)]
pub struct Aggregate {
    /// One of the module's provider-readable collections.
    pub collection: &'static str,
    /// Integer amount fields a sum may name. A count is always offered.
    pub sums: &'static [&'static str],
    /// What the result may be grouped by.
    pub dimensions: &'static [AggregateDimension],
    /// Filters a request may carry.
    pub filters: &'static [&'static str],
    /// Which date the `dateFrom`/`dateTo` filters and the `month` dimension
    /// read (`issued`, `created`).
    pub date: &'static str,
}

/// A group-by dimension and the data class of the owner's AI sharing policy
/// its labels belong to (`business_facts`, `names`).
#[derive(Debug)]
pub struct AggregateDimension {
    pub id: &'static str,
    pub class: &'static str,
}

fn leak_aggregates(value: &Value) -> &'static [Aggregate] {
    let Some(items) = value.as_array() else {
        assert!(
            value.is_null(),
            "embedded module registry: `agent.aggregates` must be an array"
        );
        return &[];
    };
    Box::leak(
        items
            .iter()
            .map(|item| Aggregate {
                collection: leak(&item["collection"], "agent.aggregates.collection"),
                sums: leak_list(&item["sums"], "agent.aggregates.sums"),
                dimensions: Box::leak(
                    item["dimensions"]
                        .as_array()
                        .expect("embedded module registry: `agent.aggregates.dimensions` must be an array")
                        .iter()
                        .map(|dimension| AggregateDimension {
                            id: leak(&dimension["id"], "agent.aggregates.dimensions.id"),
                            class: leak(&dimension["class"], "agent.aggregates.dimensions.class"),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                filters: leak_list(&item["filters"], "agent.aggregates.filters"),
                date: leak(&item["date"], "agent.aggregates.date"),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

fn leak(value: &Value, field: &str) -> &'static str {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("embedded module registry: `{field}` must be a string"));
    Box::leak(text.to_owned().into_boxed_str())
}

/// An optional list: absent means empty.
fn leak_optional_list(value: &Value, field: &str) -> &'static [&'static str] {
    if value.is_null() {
        &[]
    } else {
        leak_list(value, field)
    }
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
                tab: (!m["tab"].is_null()).then(|| leak(&m["tab"], "tab")),
                typed_detail: m["typedDetail"].as_bool().unwrap_or(false),
                rank: m["agent"]["rank"]
                    .as_u64()
                    .and_then(|rank| u32::try_from(rank).ok())
                    .expect("embedded module registry: `agent.rank` must be a small integer"),
                keywords: leak_list(&m["agent"]["keywords"], "agent.keywords"),
                collections: leak_list(&m["agent"]["collections"], "agent.collections"),
                aggregates: leak_aggregates(&m["agent"]["aggregates"]),
                owner_aliases: leak_optional_list(&m["owner"]["aliases"], "owner.aliases"),
                owner_requires: leak_optional_list(&m["owner"]["requires"], "owner.requires"),
            })
            .collect()
    })
}

/// The aggregate declared for one collection of a module, if any.
pub fn aggregate(module: &str, collection: &str) -> Option<&'static Aggregate> {
    get(module)?
        .aggregates
        .iter()
        .find(|aggregate| aggregate.collection == collection)
}

/// The module with this public id, if it is installed.
pub fn get(id: &str) -> Option<&'static Module> {
    all().iter().find(|module| module.id == id)
}

/// The module an owner means by this lower-case word: its id or one of its
/// declared aliases.
pub fn owner_named(word: &str) -> Option<&'static Module> {
    all()
        .iter()
        .find(|module| module.id == word || module.owner_aliases.contains(&word))
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
    fn aggregates_are_declared_over_readable_collections_with_known_label_classes() {
        let mut declared = Vec::new();
        for module in all() {
            for aggregate in module.aggregates {
                assert!(
                    module.collections.contains(&aggregate.collection),
                    "{}.{} is not provider-readable",
                    module.id,
                    aggregate.collection
                );
                assert!(!aggregate.dimensions.is_empty() && !aggregate.filters.is_empty());
                for dimension in aggregate.dimensions {
                    assert!(
                        matches!(dimension.class, "business_facts" | "names"),
                        "{}.{} dimension {}",
                        module.id,
                        aggregate.collection,
                        dimension.id
                    );
                }
                let names: BTreeSet<_> = aggregate.dimensions.iter().map(|d| d.id).collect();
                assert_eq!(names.len(), aggregate.dimensions.len());
                assert!(!aggregate.sums.contains(&"count"), "count is implicit");
                declared.push((module.id, aggregate.collection));
            }
        }
        assert_eq!(
            declared,
            [
                ("commerce", "orders"),
                ("purchasing", "orders"),
                ("billing", "invoices")
            ],
            "a new aggregate needs its SQL in the adapters and a disclosure review"
        );
        let invoices = aggregate("billing", "invoices").unwrap();
        assert_eq!(invoices.sums, ["gross", "outstanding", "tax"]);
        assert_eq!(invoices.date, "issued");
        assert_eq!(
            invoices
                .dimensions
                .iter()
                .map(|d| (d.id, d.class))
                .collect::<Vec<_>>(),
            [
                ("status", "business_facts"),
                ("customer", "names"),
                ("month", "business_facts")
            ]
        );
        assert!(aggregate("billing", "payments").is_none());
        assert!(aggregate("crm", "contacts").is_none());
    }

    #[test]
    fn owner_words_name_one_module_and_requirements_are_installed() {
        let mut words = BTreeSet::new();
        for module in all() {
            assert!(words.insert(module.id), "{} repeats a word", module.id);
            for alias in module.owner_aliases {
                assert_eq!(*alias, alias.to_lowercase(), "{} alias case", module.id);
                assert!(!alias.contains(char::is_whitespace), "{alias} is one word");
                assert!(words.insert(alias), "{alias} names two modules");
                assert_eq!(owner_named(alias).map(|m| m.id), Some(module.id));
            }
            assert_eq!(owner_named(module.id).map(|m| m.id), Some(module.id));
            for required in module.owner_requires {
                assert_ne!(*required, module.id, "{} requires itself", module.id);
                assert!(get(required).is_some(), "{} requires {required}", module.id);
            }
        }
        assert_eq!(owner_named("hotel").map(|m| m.id), Some("accommodation"));
        assert_eq!(
            get("accommodation").map(|m| m.owner_requires),
            Some(&["scheduling"][..])
        );
        assert!(owner_named("unknown").is_none());
    }

    #[test]
    fn workspace_tabs_are_optional_labels_in_catalogue_order() {
        let tabs: Vec<_> = all().iter().filter_map(|m| m.tab).collect();
        assert!(!tabs.is_empty());
        assert!(tabs.iter().all(|label| !label.is_empty()));
        assert!(all().iter().any(|m| m.tab.is_none()));
        assert!(all().iter().any(|m| m.typed_detail));
        assert!(all().iter().any(|m| !m.typed_detail));
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
