//! Differential tests for `archava-assembly`.
//!
//! The baseline is `packages/config/test/assembly.test.ts` and `planAssembly` in
//! `packages/config/src/assembly.ts`. Every case below is either one of the
//! baseline's cases, in Rust, or a case the baseline's own table makes
//! checkable that the baseline never checks. The second group is where a port
//! usually drifts, so it is deliberately the larger one.
//!
//! Three of these cases exist because the first draft of this port got them
//! wrong, and each names the mistake in its own doc comment rather than leaving
//! a bare assertion that a future edit could re-introduce.

use archava_assembly::catalog::{
    CatalogContracts, CatalogHost, CatalogPackage, DatabaseLedger, NamedVersion, ReleaseCatalog,
};
use archava_assembly::contracts::MODULE_CONTRACTS;
use archava_assembly::error::AssemblyError;
use archava_assembly::plan::plan;
use archava_assembly::request::{AssemblyRequest, ModuleSelection};
use archava_assembly::ModuleId;

/// A catalog package with no migrations.
fn pkg(name: &str) -> CatalogPackage {
    CatalogPackage {
        name: name.to_string(),
        version: "1.3.0".to_string(),
        migrations: Vec::new(),
    }
}

/// The TS fixture's `trusted` catalogue.
///
/// profile `1.3.0`, six packages, one migration named `001_fixture`, one ledger
/// for `archava_crm`, one required adapter `@archava/crm-pg`, one operator
/// check, `contracts.assembly = "12.0.0"`.
fn trusted() -> ReleaseCatalog {
    ReleaseCatalog {
        schema_version: "1.0.0".to_string(),
        id: "archava-local-library".to_string(),
        version: "1.3.0".to_string(),
        host: CatalogHost {
            name: "@archava/crm-host".to_string(),
            version: "1.3.0".to_string(),
        },
        contracts: CatalogContracts {
            client_config: "1.0.0".to_string(),
            deployment_plan: "2.0.0".to_string(),
            host_settings: 1,
            assembly: Some("12.0.0".to_string()),
            presence: Some("1.0.0".to_string()),
            kit_plan: Some("1.0.0".to_string()),
            kit_catalogue: Some("1.0.0".to_string()),
        },
        packages: vec![
            pkg("@archava/crm"),
            pkg("@archava/crm-pg"),
            pkg("@archava/identity-pg"),
            pkg("@archava/crm-assistant"),
            pkg("@archava/crm-act"),
            pkg("@archava/act-host-pg"),
        ],
        externals: Some(vec![NamedVersion {
            name: "rustfs".to_string(),
            version: "1.2.3".to_string(),
        }]),
        database_ledgers: vec![DatabaseLedger {
            schema: "archava_crm".to_string(),
            package: "@archava/crm-pg".to_string(),
            migrations: vec!["001_fixture".to_string()],
        }],
        required_adapters: vec!["@archava/crm-pg".to_string()],
        operator_checks: vec!["verify host reachable".to_string()],
    }
}

/// A catalogue whose only declared difference from [`trusted`] is its assembly
/// contract, so each gate case changes exactly one input.
fn with_assembly(contract: Option<&str>) -> ReleaseCatalog {
    ReleaseCatalog {
        contracts: CatalogContracts {
            assembly: contract.map(str::to_string),
            ..trusted().contracts
        },
        ..trusted()
    }
}

/// The baseline's `FULL_MODULES` — the shared name is misleading; the value is
/// the four base modules with every optional module absent, not false.
fn base() -> ModuleSelection {
    ModuleSelection::full()
}

fn request(version: &str, modules: ModuleSelection) -> AssemblyRequest {
    AssemblyRequest {
        version: version.to_string(),
        profile: "1.3.0".to_string(),
        modules,
    }
}

// ---------------------------------------------------------------------------
// Plan output
// ---------------------------------------------------------------------------

/// The baseline's base case: the four base modules plan.
#[test]
fn plans_base_modules() {
    let resolved = plan(&request("1.0.0", base()), &trusted()).expect("base modules plan");
    assert_eq!(resolved.version, "1.0.0");
    assert_eq!(resolved.profile, "1.3.0");
    assert_eq!(resolved.modules, base());
    assert_eq!(
        resolved
            .entries
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        vec!["contacts", "pipeline", "assistant", "nativeActions"],
        "entries follow MODULE_CONTRACTS order, not the request's field order"
    );
    assert_eq!(resolved.installation.len(), 6);
    assert_eq!(resolved.authority, "configuration-only");
    assert_eq!(
        resolved.data_policy,
        "retain-data-audit-receipts; refuse disable with unresolved native effects"
    );
}

/// `installation` is the catalogue, not the selection's union.
///
/// `@archava/crm` is required by both `contacts` and `pipeline` and appears
/// once, because the plan lists what a deployment installs — the catalogue it
/// was given — and not the modules' package sets.
#[test]
fn installation_is_the_catalogue_not_the_selection() {
    let resolved = plan(&request("1.0.0", base()), &trusted()).expect("base modules plan");
    assert_eq!(
        resolved
            .installation
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "@archava/crm",
            "@archava/crm-pg",
            "@archava/identity-pg",
            "@archava/crm-assistant",
            "@archava/crm-act",
            "@archava/act-host-pg",
        ],
        "installation preserves catalogue order"
    );
    let crm_count = resolved
        .installation
        .iter()
        .filter(|p| p.name == "@archava/crm")
        .count();
    assert_eq!(crm_count, 1, "a shared package is listed once");
}

/// A contacts-only selection resolves to one entry.
#[test]
fn plans_a_minimal_selection() {
    let modules = ModuleSelection {
        contacts: true,
        ..ModuleSelection::default()
    };
    let resolved = plan(&request("1.0.0", modules), &trusted()).expect("contacts-only plans");
    assert_eq!(resolved.entries.len(), 1);
    assert_eq!(resolved.entries[0].id, "contacts");
    assert_eq!(resolved.entries[0].entry, "@archava/crm");
    assert_eq!(resolved.entries[0].version, "1.0.0");
}

// ---------------------------------------------------------------------------
// The five refusals, in the baseline's order
// ---------------------------------------------------------------------------

/// `contacts: false` is refused — the schema's `z.literal(true)`.
///
/// The baseline's own fixture spreads `FULL_MODULES` and overrides `contacts`;
/// this port refuses it in `ModuleSelection::validate` instead, and this
/// asserts the refusal matches.
#[test]
fn refuses_disabled_core() {
    let modules = ModuleSelection {
        contacts: false,
        pipeline: true,
        assistant: true,
        native_actions: true,
        ..ModuleSelection::default()
    };
    assert!(matches!(
        plan(&request("1.0.0", modules), &trusted()),
        Err(AssemblyError::MissingCore)
    ));
}

/// A profile the catalogue is not (`assembly.ts:621`).
#[test]
fn refuses_unknown_profile() {
    let request = AssemblyRequest {
        version: "1.0.0".to_string(),
        profile: "9.9.9".to_string(),
        modules: base(),
    };
    let error = plan(&request, &trusted()).expect_err("profile mismatch must refuse");
    assert!(
        matches!(
            &error,
            AssemblyError::UnsupportedProfile { requested, available }
                if requested == "9.9.9" && available == "1.3.0"
        ),
        "expected UnsupportedProfile, got {error:?}"
    );
    assert!(
        error
            .to_string()
            .starts_with("Unsupported assembly profile"),
        "message must start with the baseline's string, got: {error}"
    );
}

/// The schema's own rules fire before the profile check.
///
/// The baseline reports a Zod issue here; this port reports `ModuleDependency`.
/// Same refusal, different text — recorded rather than smoothed over.
///
/// This is the case that catches a port which checks the profile first and
/// reports the wrong refusal for a request that breaks both. The broken
/// prerequisite pair is in the *base* modules — `nativeActions` needs
/// `assistant`, and both are base modules admitted at `1.0.0` — so it passes the
/// per-module version gate and reaches the dependency check ahead of the profile
/// one. A request naming a module its own version predates would instead report
/// [`AssemblyError::ModuleContractUnavailable`] first, because that gate is the
/// other half of the same schema.
#[test]
fn schema_rules_fire_before_profile_check() {
    let request = AssemblyRequest {
        version: "1.0.0".to_string(),
        profile: "9.9.9".to_string(),
        modules: ModuleSelection {
            contacts: true,
            native_actions: true,
            assistant: false,
            ..ModuleSelection::default()
        },
    };
    assert!(
        matches!(
            plan(&request, &trusted()),
            Err(AssemblyError::ModuleDependency { .. })
        ),
        "a broken prerequisite pair reports as ModuleDependency, not UnsupportedProfile"
    );
}

/// A `version` the discriminated union does not accept never reaches the
/// baseline's `planAssembly` at all, so this port refuses it at the same point.
///
/// The baseline reports `invalid_union_discriminator`; this port reports
/// `UnsupportedContract`. The message therefore cannot match, and the test
/// asserts only the refusal and the versions it names.
#[test]
fn refuses_unknown_request_version() {
    let mismatched = AssemblyRequest {
        version: "4.2.0".to_string(),
        profile: "9.9.9".to_string(),
        modules: base(),
    };
    // The profile is *also* wrong, and the profile check comes first in the
    // baseline — so an unknown version reports a profile mismatch there. This
    // port checks version validity after the profile for exactly that reason.
    assert!(matches!(
        plan(&mismatched, &trusted()),
        Err(AssemblyError::UnsupportedProfile { .. })
    ));

    // With a correct profile, the unknown version itself is what reports. The
    // local is named `unknown` rather than shadowing the `request` helper, which
    // the next line would otherwise turn into a value.
    let unknown = request("4.2.0", base());
    let error = plan(&unknown, &trusted()).expect_err("4.2.0 is not a contract version");
    assert!(matches!(
        &error,
        AssemblyError::UnsupportedContract { requested, .. } if requested == "4.2.0"
    ));
}

/// A contract above the catalogue's (`assembly.ts:674`).
///
/// The fixture's `contracts.assembly` is `12.0.0`; `13.0.0` is refused, and the
/// refusal names both versions — a fact the baseline has to throw away, since
/// its message is a bare string.
#[test]
fn refuses_contract_above_catalogue() {
    let error = plan(&request("13.0.0", base()), &trusted()).expect_err("13.0.0 > 12.0.0 refuses");
    assert_eq!(
        error.to_string(),
        "Unsupported assembly contract: requested 13.0.0, catalogue provides 12.0.0"
    );
    assert!(error
        .to_string()
        .starts_with("Unsupported assembly contract"));
}

/// `1.0.0` has no clause in the baseline's gate.
///
/// This is not "gated at the floor": the gate is a chain of
/// `request.version === 'N.0.0' && ...` clauses and there is no
/// `'1.0.0'` one. So a `1.0.0` request passes unconditionally, including
/// against a catalogue that declares a contract version on no list, or none at
/// all. Modelling it as a floor would admit or refuse the wrong catalogues
/// depending on which side of the off-by-one it landed.
#[test]
fn one_zero_request_is_not_gated() {
    // Absent contract: `1.0.0` still plans.
    assert!(
        plan(&request("1.0.0", base()), &with_assembly(None)).is_ok(),
        "1.0.0 has no gate clause, so an absent contract cannot refuse it"
    );
    // A version on no list: `1.0.0` still plans.
    assert!(
        plan(&request("1.0.0", base()), &with_assembly(Some("4.2.0"))).is_ok(),
        "1.0.0 has no gate clause, so an unknown catalogue contract cannot refuse it"
    );
    // And the next version up is refused by both, which is what makes the
    // distinction observable rather than academic.
    assert!(matches!(
        plan(&request("2.0.0", base()), &with_assembly(None)),
        Err(AssemblyError::UnsupportedContract { .. })
    ));
    assert!(matches!(
        plan(&request("2.0.0", base()), &with_assembly(Some("4.2.0"))),
        Err(AssemblyError::UnsupportedContract { .. })
    ));
}

/// The gate's arithmetic, which is `>=` and not `>`.
///
/// A request at index `N` is admitted when the catalogue's contract is at index
/// `N` **or later** in the same list — the baseline's `slice(N).includes(...)`
/// includes the request's own version at the head of the slice. An earlier
/// draft of this port compared against a table that started at `1.0.0` and
/// refused a `2.0.0` request against a `2.0.0` catalogue, which the baseline
/// admits.
#[test]
fn catalogue_at_exact_version_is_admitted() {
    assert!(
        plan(&request("2.0.0", base()), &with_assembly(Some("2.0.0"))).is_ok(),
        "a catalogue at exactly the requested contract must admit it"
    );
    assert!(
        plan(&request("13.0.0", base()), &with_assembly(Some("13.0.0"))).is_ok(),
        "the mid-table case must hold too, not just the ends"
    );
    assert!(
        plan(&request("17.0.0", base()), &with_assembly(Some("17.0.0"))).is_ok(),
        "the newest contract admits itself"
    );
}

/// A catalogue newer than the request admits it — the gate is one-directional.
#[test]
fn catalogue_newer_than_request_is_admitted() {
    assert!(
        plan(&request("1.0.0", base()), &with_assembly(Some("17.0.0"))).is_ok(),
        "a catalogue at 17.0.0 serves a 1.0.0 request"
    );
    assert!(
        plan(&request("2.0.0", base()), &with_assembly(Some("17.0.0"))).is_ok(),
        "and a 2.0.0 request"
    );
}

/// A catalogue declaring a version on no list offers nothing, exactly as an
/// absent one does — `?? ''` puts the empty string in a list it is not on.
#[test]
fn unknown_catalogue_contract_offers_nothing() {
    for version in ["2.0.0", "12.0.0", "17.0.0"] {
        assert!(
            matches!(
                plan(&request(version, base()), &with_assembly(Some("4.2.0"))),
                Err(AssemblyError::UnsupportedContract { .. })
            ),
            "a catalogue declaring 4.2.0 must refuse a {version} request"
        );
    }
}

/// The gate is not arithmetic, and this is the case that proves it.
///
/// At request contract `13.0.0` a `fieldServices` selection must be refused; only
/// `16.0.0` and later admit it. Deriving the allowed set from the version index
/// would accept the `13.0.0` and `14.0.0` requests, silently, on every machine.
///
/// Note *which* gate refuses: `ModuleContractUnavailable`, the per-module one,
/// and not `UnsupportedContract`, the catalogue one. The catalogue below
/// declares exactly the requested contract in each case, so the catalogue is
/// never the thing saying no — the request's own version is. An earlier draft of
/// this port had no per-module gate at all and reported the catalogue refusal,
/// which is how the missing check stayed invisible.
#[test]
fn field_services_gates_at_sixteen() {
    let modules = ModuleSelection {
        contacts: true,
        field_services: Some(true),
        ..ModuleSelection::default()
    };
    // A catalogue that carries fieldServices' own packages *and* an assembly
    // contract at least as new as every request below, so neither the
    // catalogue-side gate nor the package check can be the thing saying no.
    // `16.0.0` covers the `13.0.0` and `15.0.0` cases the catalogue must refuse
    // and the `16.0.0` case it must admit, which is what makes the per-module
    // gate the only gate under test.
    let mut catalog = with_assembly(Some("16.0.0"));
    catalog.packages.extend([
        pkg("@archava/field-services"),
        pkg("@archava/field-services-pg"),
    ]);

    // 13.0.0 does not carry fieldServices. The catalogue agrees — it declares
    // 13.0.0 — so only the per-module gate can refuse this.
    assert!(matches!(
        plan(&request("13.0.0", modules.clone()), &catalog),
        Err(AssemblyError::ModuleContractUnavailable { module, required })
            if module == ModuleId::FieldServices && required == "16.0.0"
    ));
    // 15.0.0 does not either — the module arrived at 16.0.0, and the baseline
    // refuses it separately at 15.0.0 with its own message.
    assert!(matches!(
        plan(&request("15.0.0", modules.clone()), &catalog),
        Err(AssemblyError::ModuleContractUnavailable { .. })
    ));
    // 16.0.0 does.
    let resolved =
        plan(&request("16.0.0", modules), &catalog).expect("16.0.0 carries fieldServices");
    assert_eq!(
        resolved.selected_ids(),
        vec![ModuleId::Contacts, ModuleId::FieldServices]
    );
}

/// The per-module gate is keyed on the *request's* version, so a catalogue that
/// is new enough to serve the module still cannot rescue it.
///
/// This is the property that distinguishes the two gates, and the case a
/// catalogue-only implementation gets wrong: the catalogue here declares
/// `17.0.0`, which carries every module, yet a `3.0.0` request naming `commerce`
/// is refused. The baseline refuses it in the schema, at `assembly.ts:395-413`,
/// before `planAssembly` ever looks at the catalogue.
///
/// A port that checked only `catalog.contracts.assembly` against the request
/// would accept this request and install a module whose migration the request's
/// own contract predates — which is the exact failure the refine exists to stop.
#[test]
fn a_new_catalogue_cannot_rescue_a_too_new_module() {
    let modules = ModuleSelection {
        contacts: true,
        commerce: Some(true),
        ..ModuleSelection::default()
    };
    assert!(matches!(
        plan(&request("3.0.0", modules), &with_assembly(Some("17.0.0"))),
        Err(AssemblyError::ModuleContractUnavailable { module, required })
            if module == ModuleId::Commerce && required == "5.0.0"
    ));
}

/// `accommodation` is the mirror of the `fieldServices` case: it is gated at
/// the *bottom* of the table, so a request for it at `1.0.0` must be refused
/// even when its prerequisite `scheduling` is enabled. The contract gates it;
/// the dependency does not rescue it.
///
/// Refused at every request version below `4.0.0`, and admitted at `4.0.0` —
/// including at `3.0.0`, where the module's *catalogue* is fine but the request
/// still predates the module.
#[test]
fn accommodation_gates_at_four() {
    let modules = ModuleSelection {
        contacts: true,
        scheduling: Some(true),
        accommodation: Some(true),
        ..ModuleSelection::default()
    };
    // At 1.0.0 and 2.0.0, both modules predate their contracts, and scheduling
    // is the earlier in contract order (index 3 vs 4), so scheduling reports.
    for version in ["1.0.0", "2.0.0"] {
        let error = plan(
            &request(version, modules.clone()),
            &with_assembly(Some(version)),
        );
        let refused = matches!(
            error.as_ref(),
            Err(AssemblyError::ModuleContractUnavailable { module, required })
                if *module == ModuleId::Scheduling && required == "3.0.0"
        );
        assert!(
            refused,
            "at {version} the earliest offending module in contract order is scheduling, not accommodation; got {error:?}"
        );
    }
    // At 3.0.0 scheduling is admitted — `3.0.0` is the first version that carries
    // it — so accommodation becomes the one that reports. The earliest-offender
    // rule is per request version, not global, and this is the case that shows it:
    // the same selection reports two different modules depending only on the
    // version it was written at.
    assert!(matches!(
        plan(&request("3.0.0", modules.clone()), &with_assembly(Some("3.0.0"))),
        Err(AssemblyError::ModuleContractUnavailable { module, required })
            if module == ModuleId::Accommodation && required == "4.0.0"
    ));
    // And at 4.0.0 the same selection plans. The fixture's catalogue does not
    // ship scheduling or accommodation, so they are added here: without them the
    // plan would be refused for a missing package and the assertion could not
    // tell the gate under test from the package check.
    let mut catalog = with_assembly(Some("4.0.0"));
    catalog.packages.extend([
        pkg("@archava/scheduling"),
        pkg("@archava/scheduling-pg"),
        pkg("@archava/accommodation"),
        pkg("@archava/accommodation-pg"),
    ]);
    let resolved = plan(&request("4.0.0", modules), &catalog).expect("4.0.0 carries accommodation");
    assert_eq!(
        resolved.selected_ids(),
        vec![
            ModuleId::Contacts,
            ModuleId::Scheduling,
            ModuleId::Accommodation
        ]
    );
}

/// Every prerequisite pair the contract table names is refused at parse time.
///
/// This is the test that explains why [`AssemblyError::DisabledModulePrerequisite`]
/// exists in the port but is never observed in it: the baseline's `superRefine`
/// (`assembly.ts:28-38`) refuses all four named pairs —
/// `accommodation→scheduling`, `nativeActions→assistant`,
/// `foodservice→commerce`, `purchasing→commerce` — and those four are exactly the
/// table's non-`contacts` `requires`. `contacts` is `z.literal(true)`, so it is
/// never falsy either.
///
/// So the plan-time `Disabled module prerequisite` throw (`assembly.ts:678`) is
/// unreachable in the baseline, and its port is unreachable in kind: `validate`
/// refuses the pair before the loop iterates, and the loop's own
/// `selected()` reads are the same truthiness the refine reads. I proved the
/// reachability rather than assuming it — a probe that pushed each pair through
/// the baseline's own `moduleSelectionSchema.safeParse` got four refusals, which
/// is what this test now asserts in Rust so the finding does not depend on a
/// throwaway script.
///
/// What the port keeps is the loop, because a caller that constructs an
/// `AssemblyRequest` without going through a schema still needs the contract
/// table enforced; the loop is that enforcement. But no *request* reaches it,
/// which is a statement about the port's reachable surface, not about its
/// correctness.
#[test]
fn schema_refuses_every_contract_prerequisite_pair() {
    let cases = [
        (
            "accommodation",
            ModuleSelection {
                contacts: true,
                accommodation: Some(true),
                ..ModuleSelection::default()
            },
            ModuleId::Accommodation,
            ModuleId::Scheduling,
        ),
        (
            "nativeActions",
            ModuleSelection {
                contacts: true,
                assistant: false,
                native_actions: true,
                ..ModuleSelection::default()
            },
            ModuleId::NativeActions,
            ModuleId::Assistant,
        ),
        (
            "foodservice",
            ModuleSelection {
                contacts: true,
                foodservice: Some(true),
                ..ModuleSelection::default()
            },
            ModuleId::Foodservice,
            ModuleId::Commerce,
        ),
        (
            "purchasing",
            ModuleSelection {
                contacts: true,
                purchasing: Some(true),
                ..ModuleSelection::default()
            },
            ModuleId::Purchasing,
            ModuleId::Commerce,
        ),
    ];
    for (name, modules, module, dependency) in cases {
        let error = plan(&request("1.0.0", modules), &with_assembly(Some("1.0.0")))
            .expect_err("{name} without its prerequisite must refuse");
        assert!(
            matches!(
                &error,
                AssemblyError::ModuleDependency { module: m, dependency: d }
                    if *m == module && *d == dependency
            ),
            "{name} must refuse at the schema layer, not at the plan-time loop; got {error:?}"
        );
    }
}

/// A module whose package the catalogue does not ship (`assembly.ts:680`).
///
/// `assistant` needs `@archava/crm-assistant`, which the fixture has. Removing
/// it refuses the module even though its only prerequisite is enabled — the
/// availability check is per-module and independent of the prerequisite check.
#[test]
fn refuses_module_missing_package() {
    let mut catalog = with_assembly(Some("12.0.0"));
    catalog
        .packages
        .retain(|p| p.name != "@archava/crm-assistant");

    let modules = ModuleSelection {
        contacts: true,
        assistant: true,
        ..ModuleSelection::default()
    };
    let error = plan(&request("1.0.0", modules), &catalog)
        .expect_err("a missing package must refuse the plan");
    assert!(
        matches!(
            &error,
            AssemblyError::ModuleImplementationUnavailable { module, package }
                if *module == ModuleId::Assistant && package == "@archava/crm-assistant"
        ),
        "expected ModuleImplementationUnavailable, got {error:?}"
    );
    assert!(
        error
            .to_string()
            .starts_with("Module implementation unavailable"),
        "got: {error}"
    );
}

/// The per-module loop runs one module at a time, not one check at a time.
///
/// The baseline's loop (`assembly.ts:675-681`) is a single `for (const module of
/// enabled)` doing prerequisite-then-package inside the body. This test pins the
/// half of that ordering that is *observable*: when two selected modules both
/// have a missing package, the reported one is the earlier in contract order —
/// not the first missing name alphabetically, not the last, and not a merged
/// report of both. A port that split the loop into two full passes over the
/// selection would report the second module instead.
///
/// The prerequisite half of the ordering is not observable, and deliberately so:
/// no request reaches that check (see
/// [`schema_refuses_every_contract_prerequisite_pair`]). Asserting it here would
/// be asserting an unreachable branch, which is how two earlier drafts of this
/// file ended up testing a refusal the baseline cannot produce.
#[test]
fn the_module_loop_reports_the_earliest_contract_not_the_earliest_name() {
    // `contacts` and `purchasing` both need packages the fixture does not have.
    // `contacts` is contract index 0 and `@archava/contacts` would sort first
    // anyway, so the second case is the one that actually discriminates: `support`
    // is at index 8 but its missing package `@archava/support` sorts after
    // `@archava/purchasing`, so an ordering keyed on package name rather than
    // contract order would report purchasing. It must report support.
    let mut catalog = with_assembly(Some("7.0.0"));
    catalog.packages.retain(|p| p.name != "@archava/support");

    let modules = ModuleSelection {
        contacts: true,
        support: Some(true),
        ..ModuleSelection::default()
    };
    let error = plan(&request("7.0.0", modules), &catalog)
        .expect_err("a missing package must refuse the plan");
    assert!(
        matches!(
            &error,
            AssemblyError::ModuleImplementationUnavailable { module, package }
                if *module == ModuleId::Support && package == "@archava/support"
        ),
        "the earliest module in contract order must be the one reported; got {error:?}"
    );
}

// ---------------------------------------------------------------------------
// The contract table
// ---------------------------------------------------------------------------

/// The table's shape, asserted rather than assumed.
///
/// One contract per module, in `ModuleId::ALL` order, every `requires` naming a
/// real module and not itself, every `package` and `entry` inside the product
/// scope. A transposed row or a dropped entry is a silent plan defect in the
/// baseline — its table is never walked in a test — and a failure here.
#[test]
fn contract_table_is_well_formed() {
    assert_eq!(MODULE_CONTRACTS.len(), ModuleId::ALL.len());
    let mut seen: Vec<ModuleId> = Vec::new();
    for contract in &MODULE_CONTRACTS {
        assert!(
            !seen.contains(&contract.id),
            "{} appears twice in MODULE_CONTRACTS",
            contract.id
        );
        seen.push(contract.id);
        for dependency in contract.requires {
            assert!(
                ModuleId::ALL.contains(dependency),
                "{} requires {}, which is not a module",
                contract.id,
                dependency
            );
            assert_ne!(*dependency, contract.id, "{} requires itself", contract.id);
        }
        for package in contract.packages {
            assert!(
                package.starts_with("@archava/"),
                "{} ships {}, which is outside the product scope",
                contract.id,
                package
            );
        }
        assert!(
            contract.entry.starts_with("@archava/"),
            "{}'s entry point {} is not a product package",
            contract.id,
            contract.entry
        );
        assert_eq!(contract.version, "1.0.0");
    }
    assert_eq!(
        MODULE_CONTRACTS.iter().map(|c| c.id).collect::<Vec<_>>(),
        ModuleId::ALL.to_vec(),
        "table order must match ModuleId::ALL, which is what makes entries stable"
    );
}

/// The four two-prerequisite entries, called out individually.
///
/// A generator that modelled the majority shape (`requires: ["contacts"]`)
/// would silently drop these — and the baseline's own table is hand-written,
/// which is why the exceptions survive there.
#[test]
fn multi_prerequisite_contracts() {
    let requires = |id: ModuleId| {
        archava_assembly::contracts::ModuleContract::find(id)
            .expect("contract exists")
            .requires
            .to_vec()
    };
    assert_eq!(requires(ModuleId::Contacts), vec![]);
    assert_eq!(requires(ModuleId::Assistant), vec![ModuleId::Contacts]);
    assert_eq!(
        requires(ModuleId::Accommodation),
        vec![ModuleId::Contacts, ModuleId::Scheduling],
        "accommodation has two prerequisites"
    );
    assert_eq!(
        requires(ModuleId::NativeActions),
        vec![ModuleId::Contacts, ModuleId::Assistant],
        "nativeActions has two prerequisites"
    );
    assert_eq!(
        requires(ModuleId::Foodservice),
        vec![ModuleId::Contacts, ModuleId::Commerce],
        "foodservice has two prerequisites"
    );
    assert_eq!(
        requires(ModuleId::Purchasing),
        vec![ModuleId::Contacts, ModuleId::Commerce],
        "purchasing has two prerequisites"
    );
    // `accommodation` and `purchasing` are also the two that share a `-pg`
    // package with another module, which is the other exception in the table.
    let shares = |id: ModuleId, package: &str| {
        archava_assembly::contracts::ModuleContract::find(id)
            .expect("contract exists")
            .packages
            .contains(&package)
    };
    assert!(shares(ModuleId::Accommodation, "@archava/scheduling-pg"));
    assert!(shares(ModuleId::Purchasing, "@archava/commerce-pg"));
}

/// Every one of the 22 modules plans against a catalogue shipping all of them at
/// the newest contract.
///
/// This is the case that catches a contract row whose `packages` entry was
/// mistyped — such a row would otherwise only surface in a real deployment.
#[test]
fn all_modules_plan_against_full_catalogue() {
    let mut catalog = with_assembly(Some("17.0.0"));
    catalog.packages = MODULE_CONTRACTS
        .iter()
        .flat_map(|contract| contract.packages.iter().map(|name| pkg(name)))
        .collect();
    // Every package the table names, duplicated where two modules share one —
    // `@archava/identity-pg` is shared by 18 of them. `installation` is the
    // catalogue verbatim, so its length is this record count and *not* the number
    // of distinct names. `planAssembly` maps `catalog.packages`, so any
    // deduplication here would be this port inventing a catalogue the operator
    // did not write.
    let records = catalog.packages.len();
    // The distinct-name count, asserted separately so the difference between the
    // two numbers is visible rather than assumed.
    let mut names: Vec<String> = catalog.packages.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names.dedup();

    let resolved = plan(&request("17.0.0", ModuleSelection::composable()), &catalog)
        .expect("every module plans against its own packages");
    assert_eq!(
        resolved.entries.len(),
        22,
        "a composable selection must resolve all 22 modules"
    );
    assert_eq!(
        resolved.entries.first().map(|e| e.id.as_str()),
        Some("contacts"),
        "contacts is always first: entries follow MODULE_CONTRACTS order"
    );
    assert_eq!(
        resolved.entries.last().map(|e| e.id.as_str()),
        Some("formalEducation")
    );
    assert_eq!(
        resolved.installation.len(),
        records,
        "installation is the catalogue, duplicates included"
    );
    assert!(
        names.len() < records,
        "the fixture must actually contain shared packages, or this test proves nothing"
    );
}

/// A catalogue missing exactly one module's package refuses the whole plan.
///
/// Not a partial plan with that module dropped: `planAssembly` throws, and a
/// port that returned a best-effort plan would be silently more permissive
/// than the baseline.
#[test]
fn one_missing_package_refuses_the_whole_plan() {
    let mut catalog = with_assembly(Some("17.0.0"));
    catalog
        .packages
        .retain(|p| p.name != "@archava/formal-education-pg");
    assert!(
        plan(&request("17.0.0", ModuleSelection::composable()), &catalog).is_err(),
        "a single missing package refuses the plan; it does not drop the module"
    );
}

// ---------------------------------------------------------------------------
// Absent vs false, and the shapes the schema pins
// ---------------------------------------------------------------------------

/// The absent/false distinction is what makes the gate work at all.
///
/// The baseline's gate reads `modules.foodservice === undefined`, so
/// `foodservice: false` at a contract that gates foodservice is refused while
/// an absent one plans. A `HashMap<ModuleId, bool>` would collapse those two and
/// accept a request the baseline rejects.
#[test]
fn absent_and_false_are_different() {
    // Absent at a contract that gates foodservice (5.0.0): plans.
    assert!(
        plan(&request("4.0.0", base()), &with_assembly(Some("4.0.0"))).is_ok(),
        "an absent optional module is not gated out"
    );
    // Explicit false at the same contract: refused, because
    // `foodservice === undefined` is false.
    let explicit_false = ModuleSelection {
        contacts: true,
        foodservice: Some(false),
        ..ModuleSelection::default()
    };
    assert!(
        matches!(
            plan(
                &request("4.0.0", explicit_false),
                &with_assembly(Some("4.0.0"))
            ),
            Err(AssemblyError::ModuleContractUnavailable { .. })
        ),
        "`foodservice: false` at contract 4.0.0 must refuse, exactly as `=== undefined` reads"
    );
}

/// The distinction survives a JSON round trip.
#[test]
fn absent_and_false_survive_serialization() {
    let selection = ModuleSelection {
        contacts: true,
        foodservice: Some(false),
        ..ModuleSelection::default()
    };
    let json = serde_json::to_string(&selection).expect("serializes");
    assert!(
        json.contains("\"foodservice\":false"),
        "an explicit false must survive, got {json}"
    );
    assert!(
        !json.contains("\"support\""),
        "an absent module must not be serialized as false, got {json}"
    );
    let back: ModuleSelection = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back.foodservice, Some(false));
    assert_eq!(back.support, None, "absent must come back absent");
}

/// `hostSettings` is a **number**.
///
/// The schema pins it to `z.literal(1)` — the one field in `contracts` whose
/// type differs from its neighbours. A port that typed it `String` would parse
/// the real catalogue and then fail to match it.
#[test]
fn host_settings_is_a_number() {
    let json = r#"{"hostSettings":1}"#;
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(rename = "hostSettings")]
        host_settings: u32,
    }
    let probe: Probe = serde_json::from_str(json).expect("hostSettings deserializes as a number");
    assert_eq!(probe.host_settings, 1);

    let trusted_json = serde_json::to_string(&trusted()).expect("serializes");
    assert!(
        trusted_json.contains("\"hostSettings\":1"),
        "the catalogue round trip must keep the numeric type, got {trusted_json}"
    );
    let back: ReleaseCatalog = serde_json::from_str(&trusted_json).expect("deserializes");
    assert_eq!(back.contracts.host_settings, 1);
}

/// The catalogue's serde renames, asserted as a group.
///
/// A missing rename parses on a hand-built Rust value and then fails on the
/// real file — or, worse, silently reads `None` for an optional field the real
/// catalogue sets, which would gate every request the baseline admits.
#[test]
fn catalogue_field_names() {
    let json = serde_json::to_string(&trusted()).expect("serializes");
    for key in [
        "\"schemaVersion\"",
        "\"databaseLedgers\"",
        "\"requiredAdapters\"",
        "\"operatorChecks\"",
        "\"clientConfig\"",
        "\"deploymentPlan\"",
        "\"hostSettings\"",
        "\"kitPlan\"",
        "\"kitCatalogue\"",
    ] {
        assert!(
            json.contains(key),
            "catalogue JSON is missing {key}: {json}"
        );
    }
    let back: ReleaseCatalog = serde_json::from_str(&json).expect("round trips");
    assert_eq!(back, trusted());
}

/// The plan's serde renames, asserted the same way.
///
/// `unresolvedLegacyNames` and `dataPolicy` are camelCase on the wire; a plan
/// serialized snake_case is a plan no consumer recognizes.
#[test]
fn plan_serializes_with_baseline_field_names() {
    let resolved = plan(&request("1.0.0", base()), &trusted()).expect("base modules plan");
    let json = serde_json::to_string(&resolved).expect("serializes");
    for key in [
        "\"unresolvedLegacyNames\"",
        "\"dataPolicy\"",
        "\"installation\"",
        "\"authority\"",
        "\"entries\"",
        "\"modules\"",
    ] {
        assert!(json.contains(key), "plan JSON is missing {key}: {json}");
    }
    let back: archava_assembly::AssemblyPlan = serde_json::from_str(&json).expect("round trips");
    assert_eq!(back, resolved);
}

/// The module selection's camelCase renames — `nativeActions`,
/// `fieldServices`, `formalEducation`.
#[test]
fn selection_field_names() {
    let selection = ModuleSelection {
        contacts: true,
        native_actions: true,
        assistant: true,
        field_services: Some(true),
        formal_education: Some(true),
        ..ModuleSelection::default()
    };
    let json = serde_json::to_string(&selection).expect("serializes");
    for key in [
        "\"nativeActions\"",
        "\"fieldServices\"",
        "\"formalEducation\"",
    ] {
        assert!(
            json.contains(key),
            "selection JSON is missing {key}: {json}"
        );
    }
    let back: ModuleSelection = serde_json::from_str(&json).expect("round trips");
    assert_eq!(back, selection);
}

/// The legacy names and the two literals, verbatim.
///
/// Operators read a plan looking for these; a reworded list is a lie about what
/// the baseline still resolves.
#[test]
fn legacy_names_and_literals_are_verbatim() {
    let resolved = plan(&request("1.0.0", base()), &trusted()).expect("base modules plan");
    assert_eq!(
        resolved.unresolved_legacy_names,
        vec!["crm_core", "payment_gateway", "inventory", "booking", "erp"]
    );
    assert_eq!(resolved.authority, "configuration-only");
    assert_eq!(
        resolved.data_policy,
        "retain-data-audit-receipts; refuse disable with unresolved native effects"
    );
}

// ---------------------------------------------------------------------------
// Message parity
// ---------------------------------------------------------------------------

/// Each of the baseline's four plan-time throw strings, and each variant that
/// renders one. The messages are the compatibility surface: a caller that has
/// always switched on `starts_with("Unsupported assembly profile")` must keep
/// matching after the port.
#[test]
fn baseline_messages_hold() {
    let cases: Vec<(AssemblyError, &str)> = vec![
        (
            AssemblyError::UnsupportedProfile {
                requested: "1.3.0".to_string(),
                available: "2.0.0".to_string(),
            },
            "Unsupported assembly profile",
        ),
        (
            AssemblyError::UnsupportedContract {
                requested: "13.0.0".to_string(),
                available: Some("12.0.0".to_string()),
            },
            "Unsupported assembly contract",
        ),
        (
            AssemblyError::UnsupportedContract {
                requested: "13.0.0".to_string(),
                available: None,
            },
            "Unsupported assembly contract",
        ),
        (
            AssemblyError::DisabledModulePrerequisite {
                module: ModuleId::Accommodation,
                dependency: ModuleId::Scheduling,
            },
            "Disabled module prerequisite",
        ),
        (
            AssemblyError::ModuleImplementationUnavailable {
                module: ModuleId::Assistant,
                package: "@archava/crm-assistant".to_string(),
            },
            "Module implementation unavailable",
        ),
    ];
    for (error, prefix) in cases {
        assert!(
            error.to_string().starts_with(prefix),
            "{error} does not start with the baseline's `{prefix}`"
        );
    }
}

/// `ModuleId` renders as the schema spells it, which is what the messages use
/// and what `PlanEntry::id` carries.
#[test]
fn module_ids_render_as_the_schema_spells_them() {
    for id in ModuleId::ALL {
        let rendered = id.to_string();
        assert_eq!(rendered, id.as_str());
        assert_eq!(ModuleId::parse(&rendered), Some(id));
    }
    // The two camelCase ones are the ones a snake_case habit would get wrong.
    assert_eq!(ModuleId::FieldServices.as_str(), "fieldServices");
    assert_eq!(ModuleId::FormalEducation.as_str(), "formalEducation");
    assert_eq!(ModuleId::NativeActions.as_str(), "nativeActions");
}
