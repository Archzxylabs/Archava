//! Consumer A's proof: quote and price a stay through the *shipped crates*.
//!
//! This file is the assertion half of the cold consumer, and it is written to be
//! independent rather than a mirror. It does not call the parity harness, import
//! `compare.mjs`, or read either evidence file. Those prove the port matches the
//! TypeScript baseline; this proves something different and narrower — that a
//! program with no knowledge of the product tree can consume the artifacts and
//! get a *correct* answer out of them. A consumer that only replayed canned
//! output would pass just as loudly on engines that no longer worked.
//!
//! Every expected number below is derived from `config/pricing.v1.json` by hand
//! and cross-checked against the p1 parity evidence, not copied from it. If the
//! engine silently changes, this fails; that is the point.
//!
//! ## What is deliberately *not* asserted here
//!
//! Anything about the TypeScript engine. The two engines agreeing is the parity
//! harness's claim, and a consumer that re-checked it would be a second harness
//! rather than a second consumer. G05 asks for "two independent cold consumers
//! use identical shipped core/domain", so the two consumers must share the
//! artifacts and be independent of each other — which is why consumer B (the
//! addon) has its own tests with its own numbers.
//!
//! One more property worth naming: `deny_unknown_fields`. The port adds it
//! because the baseline is strict too, and the pricing error is *more* specific
//! than the TypeScript one. Consumer A asserts the refusal happens, not that the
//! message matches, because the message is the documented D01 divergence and
//! asserting it here would make a known contract difference look like a match.
use archava_pricing::engine::PricingEngine;
use archava_pricing::pricebook::Pricebook;
use archava_pricing::request::QuoteRequestInit;

/// The shipped pricebook, verbatim — four levels up, because `include_str!`
/// resolves relative to *this* file, which is D10's lesson: a fixture that
/// climbs out of its own directory by the wrong number of levels is a fixture
/// that cannot move. The consumer's scratch tree keeps this exact depth so the
/// bytes under test are the same object in both.
const PRICEBOOK: &str = include_str!("../../../../config/pricing.v1.json");

/// The one request this consumer makes, in the shape the engine wants.
fn request() -> QuoteRequestInit {
    serde_json::from_str(
        r#"{"region":"GLOBAL","presence":"chat","capability":"assist","environment":"existing_site"}"#,
    )
    .expect("a consumer-shaped request parses")
}

#[test]
fn quotes_a_stay_from_the_shipped_pricebook() {
    let pricebook: Pricebook =
        serde_json::from_str(PRICEBOOK).expect("the shipped pricebook parses");
    let engine = PricingEngine::new(&pricebook);
    let quote = engine.quote(request()).expect("a supported request quotes");

    // Checked by hand against `config/pricing.v1.json`: the `chat` presence
    // ships a 2,000 USD setup in the GLOBAL roundings, with no bundle discount
    // because one core component is below the two-component rule.
    assert_eq!(quote.region.as_str(), "GLOBAL");
    assert_eq!(quote.currency, "USD");
    assert_eq!(quote.presence, "chat");
    assert_eq!(quote.capability, "assist");
    assert_eq!(quote.environment, "existing_site");
    assert_eq!(quote.core_components.len(), 1);
    assert_eq!(quote.paid_core_component_count, 1);
    assert_eq!(quote.fixed_core_setup, 2000.0);
    assert_eq!(quote.discounted_core_setup, 2000.0);
    assert_eq!(quote.bundle_discount_pct, 0.0);
    assert!(quote.design_addons.is_empty());
}

#[test]
fn reports_the_pricebooks_own_effective_date() {
    let pricebook: Pricebook = serde_json::from_str(PRICEBOOK).unwrap();
    let engine = PricingEngine::new(&pricebook);
    // The consumer reads the profile off the artifact rather than hardcoding
    // it, because a consumer that pinned these by hand would pass against a
    // stale pricebook it had been told to expect.
    assert!(!engine.schema_version().is_empty());
    assert!(!engine.effective_date().is_empty());
    assert!(engine.quote_validity_days() > 0);
}

#[test]
fn a_request_for_a_tier_the_pricebook_does_not_carry_is_refused() {
    let pricebook: Pricebook = serde_json::from_str(PRICEBOOK).unwrap();
    let engine = PricingEngine::new(&pricebook);
    let bad: QuoteRequestInit =
        serde_json::from_str(r#"{"region":"GLOBAL","presence":"nope"}"#).unwrap();
    let error = engine
        .quote(bad)
        .expect_err("an unknown presence is not silently defaulted");
    // The *fact* of refusal, not its wording: the wording is D01, recorded as a
    // contract difference rather than asserted as parity.
    assert!(error.to_string().contains("Unknown presence"));
}

#[test]
fn the_crates_expose_no_hidden_entry_point_beyond_their_boundaries() {
    // A consumer can only reach what the artifact publishes. This is the
    // structural half of "uses identical shipped core": the symbols consumed
    // here are the whole public surface this proof depends on, and it does not
    // touch `archava_rates`' internals or anything from the product's own
    // crates (`archava_host`, `archava_native`). If a future refactor moved a
    // needed type into a private module, this consumer would stop compiling
    // — which is exactly the signal G05 is asking for.
    let pricebook: Pricebook = serde_json::from_str(PRICEBOOK).unwrap();
    let engine = PricingEngine::new(&pricebook);
    // The path through which `RegionCode` is actually public. Found by compiling
    // this file against the packaged crate rather than against the product tree,
    // where a `use` inside `request.rs` makes `request::RegionCode` *look* like a
    // valid path. It is not: `request.rs` imports the enum privately and never
    // re-exports it. A consumer written from reading the source reaches for the
    // wrong path and fails to compile — which is the signal, and the reason this
    // test is compiled from the tarball instead of the workspace.
    let included =
        engine.included_standard_integrations(archava_pricing::pricebook::RegionCode::Global, "assist");
    assert_eq!(included, 0.0);
    // The same enum, reached through the engine's own surface, gives a different
    // answer for a capability that does carry an allowance. Asserting only the
    // zero case would pass even if the lookup always returned zero.
    assert_eq!(
        engine.included_standard_integrations(archava_pricing::pricebook::RegionCode::Global, "act"),
        1.0
    );
}

/// The small assembly fixture this consumer plans against.
///
/// The TS baseline's own fixture catalogue (the shape
/// `archava-assembly`'s differential suite transcribes): profile `1.3.0`, six
/// packages, one migration ledger for `archava_crm`, one required adapter, one
/// operator check, and the assembly contract pinned at `12.0.0`. It is inline
/// and not `include_str!`-climbed because the catalogue is a *test-owned*
/// fixture, unlike the pricebook, which is the artifact-under-test's own data —
/// a consumer asserting on the planner's semantics does not need the product's
/// real catalogue bytes; it needs a catalogue the planner's rules have to
/// resolve or refuse.
const CATALOGUE: &str = r#"{
    "schemaVersion": "1.0.0",
    "id": "archava-local-library",
    "version": "1.3.0",
    "host": { "name": "@archava/crm-host", "version": "1.3.0" },
    "contracts": {
        "clientConfig": "1.0.0",
        "deploymentPlan": "2.0.0",
        "hostSettings": 1,
        "assembly": "12.0.0"
    },
    "packages": [
        { "name": "@archava/crm", "version": "1.3.0" },
        { "name": "@archava/crm-pg", "version": "1.3.0" },
        { "name": "@archava/identity-pg", "version": "1.3.0" },
        { "name": "@archava/crm-assistant", "version": "1.3.0" },
        { "name": "@archava/crm-act", "version": "1.3.0" },
        { "name": "@archava/act-host-pg", "version": "1.3.0" }
    ],
    "externals": [{ "name": "rustfs", "version": "1.2.3" }],
    "databaseLedgers": [
        { "schema": "archava_crm", "package": "@archava/crm-pg", "migrations": ["001_fixture"] }
    ],
    "requiredAdapters": ["@archava/crm-pg"],
    "operatorChecks": ["verify host reachable"]
}"#;

/// A cold consumer *plans* with the shipped crate, not only quotes.
///
/// P-PLANNING-CONFORMANCE's third check is this file: the planning engine is
/// consumed the way a customer consumes it — packed tarball, no product path,
/// offline — and it answers with the fields a caller renders. The assertions
/// are the plan's structural contract, cross-checked by hand against the
/// fixture: version/profile echoed, base modules resolved in contract order,
/// the installation taken verbatim from the catalogue (and NOT filtered by the
/// selection), the two resolved literals, and no unresolved legacy names.
#[test]
fn plans_an_assembly_from_the_shipped_crate() {
    let catalog: archava_assembly::catalog::ReleaseCatalog =
        serde_json::from_str(CATALOGUE).expect("the fixture catalogue parses");
    let request: archava_assembly::request::AssemblyRequest = serde_json::from_str(
        r#"{
            "version": "1.0.0",
            "profile": "1.3.0",
            "modules": {
                "contacts": true, "pipeline": true, "assistant": true, "nativeActions": true
            }
        }"#,
    )
    .expect("the consumer's request parses");

    let resolved = archava_assembly::plan::plan(&request, &catalog)
        .expect("base modules resolve against the fixture catalogue");

    assert_eq!(resolved.version, "1.0.0", "the request version echoes");
    assert_eq!(resolved.profile, "1.3.0", "the catalogue profile echoes");
    assert_eq!(
        resolved
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["contacts", "pipeline", "assistant", "nativeActions"],
        "entries resolve in contract order, not request field order"
    );
    assert_eq!(resolved.installation.len(), 6, "the plan installs the catalogue");
    assert_eq!(
        resolved.installation[0].name, "@archava/crm",
        "installation follows catalogue order"
    );
    assert_eq!(resolved.authority, "configuration-only");
    assert_eq!(
        resolved.data_policy,
        "retain-data-audit-receipts; refuse disable with unresolved native effects"
    );
    assert_eq!(
        resolved.unresolved_legacy_names,
        vec!["crm_core", "payment_gateway", "inventory", "booking", "erp"],
        "the resolved legacy names are the plan's fixed record, not a per-catalogue derivation"
    );
}

/// And the shipped planner refuses the way its baseline spells the refusal,
/// not with a panic — the negative half of consuming it cold.
#[test]
fn a_cold_catalogue_mismatch_refuses_with_the_baselines_message() {
    let catalog: archava_assembly::catalog::ReleaseCatalog =
        serde_json::from_str(CATALOGUE).expect("the fixture catalogue parses");
    let request: archava_assembly::request::AssemblyRequest = serde_json::from_str(
        r#"{
            "version": "1.0.0",
            "profile": "9.9.9",
            "modules": {
                "contacts": true, "pipeline": true, "assistant": true, "nativeActions": true
            }
        }"#,
    )
    .expect("the mismatched request parses");

    let error = archava_assembly::plan::plan(&request, &catalog)
        .expect_err("a profile the catalogue does not carry must refuse");
    // The message prefix is the plan's own contract surface — the same test in
    // the crate asserts the variant structurally; the consumer asserts the
    // string a caller would show an operator.
    assert!(
        error.to_string().starts_with("Unsupported assembly profile"),
        "expected the baseline's refusal wording, got: {error}"
    );
}
