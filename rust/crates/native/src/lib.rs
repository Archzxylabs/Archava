//! The napi bridge for the pilot's slices.
//!
//! Every function here has the same shape, and the shape is the point: **the
//! arguments are strings and the return is a string**. The pricebook comes in
//! as raw JSON text and the quote goes out as raw JSON text, with no Rust
//! struct ever crossing the boundary. The p2 rates bridge adds no pricebook
//! argument at all, because that slice's fixtures *are* its data.
//!
//! That is a deliberate choice with a cost, stated plainly. Struct-based napi
//! bindings would be faster — they avoid one parse and one serialize per call
//! — and the benchmark does *not* get to pretend otherwise: it measures this
//! string boundary exactly as built, not a hypothetical struct one. What the
//! string boundary buys is that the parity harness hands both engines the
//! *identical bytes*. A struct boundary would need a JS object on one side and
//! a Rust object on the other, and a port that was subtly wrong about a default
//! could recover itself during that conversion, making the two engines agree
//! on a wrong answer. Passing text means the only thing that can differ is the
//! computation.
//!
//! The second consequence: the addon holds no state. Every call re-parses the
//! pricebook, so the pricebook can never be stale relative to the file it came
//! from, and the function is trivially safe to call from any number of threads.
//! If measurement later shows re-parsing dominates, the fix is a cache keyed
//! on the pricebook's own hash — which is a benchmark finding, not something
//! to guess at now.

use napi::bindgen_prelude::*;
use napi_derive::napi;
// Imported explicitly rather than left to the prelude glob above: the glob does
// not carry the serde derives or the `#[serde(...)]` helper attribute, and
// `quote_json` gets away without them only because every type it deserializes
// is defined in `archava_pricing`, which derives for itself. The p2 bridge is
// the first function here to declare its own input struct.
use serde::{Deserialize, Serialize};

/// Quote one request against one pricebook, both as JSON text.
///
/// This is the entire API of the addon for the p1 slice. It exists so the
/// harness has a single entry point whose inputs and output are byte-comparable
/// between the two engines.
///
/// # Arguments
///
/// * `pricebook_json` — the pricebook, exactly the bytes of
///   `config/pricing.v1.json`. Passing text rather than a path means the
///   caller decides which file was read, and the harness already hashes that
///   file for the evidence manifest.
/// * `request_json` — a `QuoteRequestInit`: all four tier names required, every
///   other field optional. Unknown extra keys are ignored, matching how the
///   TypeScript engine receives its request.
///
/// # Returns
///
/// The quote, serialized with the 26 keys in the order `engine.ts` builds its
/// literal. The order is load-bearing: the harness compares the two engines as
/// *text*, so a key-order difference would show up as a divergence even with
/// identical values.
///
/// # Errors
///
/// Throws for three distinct reasons, and the harness distinguishes them by
/// message because a thrown napi error carries no structured code:
///
/// 1. The pricebook is not valid JSON, or does not match the pricebook schema.
///    This is `serde_json`'s own message, and it differs from what the
///    TypeScript loader would say about the same file — a known, documented
///    divergence, reported in `docs/RUST_PILOT_RESULTS.md` rather than hidden.
/// 2. The request names a tier the pricebook does not carry. The message is
///    `engine.ts`'s own string, character for character, because the parity
///    harness compares the two engines' error text.
/// 3. The request is not valid JSON, or is missing a required tier name. The
///    TypeScript types make those fields non-optional, so this state is not
///    reachable through the typed API — but the boundary is untyped, so the
///    port rejects it explicitly rather than defaulting a tier name to
///    something plausible.
#[napi]
pub fn quote_json(pricebook_json: String, request_json: String) -> Result<String> {
    let pricebook: archava_pricing::pricebook::Pricebook = serde_json::from_str(&pricebook_json)
        .map_err(|error| {
            Error::new(
                napi::Status::InvalidArg,
                format!("Rust could not deserialize the pricebook: {error}"),
            )
        })?;

    let init: archava_pricing::request::QuoteRequestInit = serde_json::from_str(&request_json)
        .map_err(|error| {
            Error::new(
                napi::Status::InvalidArg,
                format!("Rust could not deserialize the request: {error}"),
            )
        })?;

    let engine = archava_pricing::engine::PricingEngine::new(&pricebook);
    // `quote` takes the `init` directly and applies the defaults itself, so the
    // conversion lives in exactly one place — the same rule the baseline follows.
    let quote = engine
        .quote(init)
        .map_err(|error| Error::new(napi::Status::GenericFailure, error.to_string()))?;

    serde_json::to_string(&quote).map_err(|error| {
        Error::new(
            napi::Status::GenericFailure,
            format!("Rust could not serialize the quote: {error}"),
        )
    })
}

/// Answer one rates question, as JSON text in and JSON text out.
///
/// The p2 bridge. Where `quote_json` needs a pricebook because the p1 engine
/// reads one from disk, the rates slice's fixtures *are* its data — the tables
/// live in the source — so there is nothing to pass and nothing to load. The
/// consequence for parity is structural rather than incidental: there is no
/// file path, no pricebook hash, and no separate loader, so the two engines
/// cannot disagree about which fixture set they were handed. The only input is
/// the question.
///
/// # Arguments
///
/// * `request_json` — `{"op": …, …}`. `op` names which accessor to run and the
///   remaining keys are its arguments:
///
///   | `op` | other keys |
///   | --- | --- |
///   | `nightlyRate` | `unitId`, `night` |
///   | `nightlyAvailability` | `unitId`, `night` |
///   | `nightsBetween` | `from`, `to` |
///   | `stayAvailability` | `unitId`, `from`, `to` |
///   | `stayQuote` | `unitId`, `from`, `to` |
///   | `bookableStays` | `from`, `to` |
///   | `cheapestBookable` | `from`, `to` |
///   | `roomsBuilt` | `unitId` |
///   | `baseRate` | `unitId` |
///   | `isReferenceNight` | `night` |
///   | `isWeekendNight` | `night` |
///
/// # Returns
///
/// The answer, serialized with the same keys in the same order the TypeScript
/// functions build their literals, because the harness compares as *text*.
///
/// The absent case is the JSON literal `null`. The TypeScript accessors return
/// `undefined`, which `JSON.stringify` refuses to turn into a string at all, so
/// the harness normalizes `undefined` to `null` before comparing. That is safe
/// here specifically and only because **no accessor in this slice returns
/// `null`** — every one returns either a value or `undefined` — so `null` can
/// mean exactly one thing. If a future slice adds a real `null`, this
/// normalization has to be re-examined rather than inherited.
///
/// # Errors
///
/// Only for a request the harness itself could not have meant: invalid JSON, or
/// an `op` that is not in the table above. Neither is a state the TypeScript
/// functions can express — they are called directly, not dispatched by name —
/// so neither can be a parity case, and the port says so rather than defaulting
/// to a plausible op.
#[napi]
pub fn rates_json(request_json: String) -> Result<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RatesRequest {
        op: String,
        #[serde(default)]
        unit_id: Option<String>,
        #[serde(default)]
        night: Option<String>,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
    }

    let request: RatesRequest = serde_json::from_str(&request_json).map_err(|error| {
        Error::new(
            napi::Status::InvalidArg,
            format!("Rust could not deserialize the rates request: {error}"),
        )
    })?;

    /// A required argument, named in the error so a harness mistake points at
    /// itself rather than at the port.
    fn required(field: &str, value: &Option<String>) -> Result<String> {
        value.clone().ok_or_else(|| {
            Error::new(
                napi::Status::InvalidArg,
                format!("Rust rates request is missing its \"{field}\" argument"),
            )
        })
    }

    /// Serialize an answer to JSON text, mapping the failure the way
    /// `quote_json` maps its own: a named napi error, never a bare `?`.
    ///
    /// Serializing a plain struct of strings, integers and booleans cannot
    /// realistically fail, but "cannot realistically fail" is not a reason to
    /// leave a path where it would surface as a panic across the boundary.
    fn json_text<T: Serialize>(value: &T) -> Result<String> {
        serde_json::to_string(value).map_err(|error| {
            Error::new(
                napi::Status::GenericFailure,
                format!("Rust could not serialize the rates answer: {error}"),
            )
        })
    }

    let unit_id = |field: &'static str| required(field, &request.unit_id);
    let night = |field: &'static str| required(field, &request.night);
    let from = |field: &'static str| required(field, &request.from);
    let to = |field: &'static str| required(field, &request.to);

    let answer = match request.op.as_str() {
        "nightlyRate" => {
            let rate = archava_rates::nightly_rate(&unit_id("unitId")?, &night("night")?);
            json_text(&rate)?
        }
        "nightlyAvailability" => {
            let availability =
                archava_rates::nightly_availability(&unit_id("unitId")?, &night("night")?);
            json_text(&availability)?
        }
        "nightsBetween" => {
            let nights = archava_rates::nights_between(&from("from")?, &to("to")?);
            json_text(&nights)?
        }
        "stayAvailability" => {
            let availability =
                archava_rates::stay_availability(&unit_id("unitId")?, &from("from")?, &to("to")?);
            json_text(&availability)?
        }
        "stayQuote" => {
            let quote = archava_rates::stay_quote(&unit_id("unitId")?, &from("from")?, &to("to")?);
            json_text(&quote)?
        }
        "bookableStays" => {
            let bookable = archava_rates::bookable_stays(&from("from")?, &to("to")?);
            json_text(&bookable)?
        }
        "cheapestBookable" => {
            let cheapest = archava_rates::cheapest_bookable(&from("from")?, &to("to")?);
            json_text(&cheapest)?
        }
        "roomsBuilt" => {
            let rooms = archava_rates::rooms_built_of(&unit_id("unitId")?);
            json_text(&rooms)?
        }
        "baseRate" => {
            let base = archava_rates::base_rate(&unit_id("unitId")?);
            json_text(&base)?
        }
        "isReferenceNight" => {
            let is = archava_rates::is_reference_night(&night("night")?);
            json_text(&is)?
        }
        "isWeekendNight" => {
            let is = archava_rates::is_weekend_night(&night("night")?);
            json_text(&is)?
        }
        other => {
            return Err(Error::new(
                napi::Status::InvalidArg,
                format!("Rust rates request names no such op: {other}"),
            ))
        }
    };

    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of the real pricebook: only the tiers the tests below
    /// name, so a failure points at the bridge rather than at the pricebook.
    const PRICEBOOK: &str = r#"{
    "schema_version": "1.0.0",
    "effective_date": "2026-09-22",
    "currency_rounding": { "GLOBAL": 50, "ID": 100000 },
    "regions": {
      "GLOBAL": {
        "currency": "USD",
        "presence": {
          "chat": { "setup": 2000, "monthly": 249,
                    "included_usage": { "unit": "active_conversation", "quantity": 1000 },
                    "overage": { "unit": "active_conversation", "price": 0.1 } }
        },
        "capability": { "assist": { "setup": 0, "monthly": 0, "included_standard_integrations": 0 } },
        "environment": { "existing_site": { "setup": 0, "monthly": 0, "note": "x" } },
        "integration": { "standard": { "setup": 750 } },
        "design_addons": { "custom_visual_direction": { "setup": 1500 } },
        "support_addons": { "standard": { "monthly": 0 } }
      }
    },
    "discount_rules": {
      "two_core_components_setup_discount_pct": 5,
      "three_core_components_setup_discount_pct": 10,
      "core_components": ["presence"],
      "annual_recurring_prepay_discount_pct": 10
    },
    "commercial_rules": { "quote_validity_days": 14 }
  }"#;

    const REQUEST: &str = r#"{
    "region": "GLOBAL", "presence": "chat", "capability": "assist",
    "environment": "existing_site"
  }"#;

    #[test]
    fn a_valid_request_produces_a_quote_string() {
        let quote = quote_json(PRICEBOOK.to_string(), REQUEST.to_string())
            .expect("a well-formed request quotes");
        // The output must be JSON text, not an object the caller re-parses.
        let parsed: serde_json::Value =
            serde_json::from_str(&quote).expect("the bridge returns JSON text");
        assert_eq!(parsed["region"], "GLOBAL");
        assert_eq!(parsed["oneTimeTotal"], 2000.0);
    }

    #[test]
    fn an_unknown_tier_surfaces_the_engines_own_message() {
        // Not `serde_json`'s error and not a Rust `Debug` string — the exact
        // sentence `engine.ts` throws, which is what parity compares.
        //
        // `Error::to_string()` prints napi's `Display`, which prepends the status
        // variant; the message the JS side actually receives is `reason` alone.
        // This test asserts on `reason` because that is what the harness reads.
        let bad = REQUEST.replace("\"chat\"", "\"telepathy\"");
        let error = quote_json(PRICEBOOK.to_string(), bad).expect_err("no such presence");
        assert_eq!(error.reason, "Unknown presence \"telepathy\".");
    }

    #[test]
    fn a_malformed_pricebook_is_rejected_before_the_request_is_read() {
        let error = quote_json("{ not json".to_string(), REQUEST.to_string())
            .expect_err("a broken pricebook");
        assert!(error.to_string().contains("pricebook"));
    }

    #[test]
    fn a_malformed_request_is_rejected_explicitly() {
        let error = quote_json(PRICEBOOK.to_string(), "{ nope".to_string())
            .expect_err("a broken request cannot quote");
        assert!(error.to_string().contains("request"));
    }

    /// An absent tier name must arrive as a **catchable** error, never a panic.
    ///
    /// This is the regression test for the pilot's most severe finding. The first
    /// version of `request.rs` made the four tier names required fields and
    /// called `.expect()` on each, so a request that omitted one was unreachable
    /// as a catchable error — the same request against the built addon printed
    /// `fatal runtime error: failed to initiate panic`, exited 134 with a core
    /// dump, and the calling script's own `try/catch` never ran. A panic crossing
    /// the napi boundary is not a JavaScript exception.
    ///
    /// What that means for the test's shape: the test *fails* rather than
    /// aborting, and it is worth being precise about why, because the words
    /// "abort" and "fail" are not interchangeable here. `cargo test` does not
    /// cross a real napi boundary, so a reintroduced panic unwinds inside the
    /// harness and the test fails normally — verified by putting the `.expect()`
    /// back on `region` and watching this test fail with exit 101, not by
    /// watching a core drop. The exit-134 abort is what happens when the same
    /// request reaches the *addon* from Node. So this test pins the contract but
    /// does not reproduce the severity, and the severity is only provable by
    /// calling the built `.node` from a script — which is why the original
    /// finding came out of a probe rather than out of the test suite.
    ///
    /// It asserts all four tier names, because the original defect was four
    /// separate call sites and fixing one is not fixing the finding.
    ///
    /// Note what is *not* asserted here. p1's 346 fixtures never omit a tier
    /// name, so the parity run passed straight over this path; the case set is
    /// extended separately in `node/gen-cases.mjs`. This test pins the boundary's
    /// contract, and the parity run pins the two engines' agreement — neither
    /// substitutes for the other.
    #[test]
    fn an_absent_tier_name_is_a_catchable_error_not_a_process_abort() {
        for (missing, expected) in [
            ("region", "Unknown region \"undefined\". No quote produced."),
            ("presence", "Unknown presence \"undefined\"."),
            ("capability", "Unknown capability \"undefined\"."),
            ("environment", "Unknown environment \"undefined\"."),
        ] {
            // Each variant is the complete request minus the one key under test, so
            // the other three names stay valid and the failure can only be the
            // absence. Every key here is in the schema, so `deny_unknown_fields`
            // cannot be what rejects the request.
            let body = [
                r#""region": "GLOBAL""#,
                r#""presence": "chat""#,
                r#""capability": "assist""#,
                r#""environment": "existing_site""#,
            ]
            .into_iter()
            .filter(|entry| !entry.contains(&format!("\"{missing}\"")))
            .collect::<Vec<_>>()
            .join(", ");

            let error = quote_json(PRICEBOOK.to_string(), format!("{{ {body} }}"))
                .expect_err("an absent tier name cannot produce a quote");
            // `reason`, not `to_string()`: napi's `Display` prepends the status
            // variant, and `reason` is the message the JS side actually receives —
            // the one the baseline's own `PricingError` message must equal.
            assert_eq!(
                error.reason, expected,
                "the message for a missing {missing} must match engine.ts character for character"
            );
        }
    }
}

/// The p2 bridge.
///
/// Unlike the p1 tests above, these assert on the *text*, because text is what
/// the parity harness compares. Each one names the key order it depends on, so
/// a reordering of a Rust struct — which `serde` would happily do — fails here
/// as a key-order regression rather than showing up across the boundary as an
/// unexplained divergence.
#[cfg(test)]
mod rates_tests {
    use super::*;

    /// Ask the bridge and parse the answer.
    fn ask(request: &str) -> serde_json::Value {
        serde_json::from_str(&ask_raw(request)).expect("the bridge answers with JSON text")
    }

    /// Ask the bridge and keep the answer as text.
    ///
    /// Separate from `ask` because `serde_json::Value` stores its object keys in a
    /// `BTreeMap` and therefore sorts them: anything parsed into a `Value` has lost
    /// the order the bridge emitted, so a key-order assertion has to run on this
    /// raw string. The first version of the order test below used `ask` and
    /// "failed" against a correct answer, which is exactly the harness bug worth
    /// writing down.
    fn ask_raw(request: &str) -> String {
        rates_json(request.to_string())
            .unwrap_or_else(|error| panic!("the bridge rejected {request}: {}", error.reason))
    }

    #[test]
    fn the_rate_for_one_night_arrives_in_the_baselines_key_order() {
        let answer = ask_raw(r#"{"op":"nightlyRate","unitId":"garden-twin","night":"2026-10-05"}"#);
        // `unitId`, `night`, `amountMinor`, `currency` — the order rates.ts builds
        // its literal in. The harness compares the two engines as *text*, so this
        // order is load-bearing, and it comes from the struct's declaration order
        // rather than from anything sorted.
        assert_eq!(
            answer,
            r#"{"unitId":"garden-twin","night":"2026-10-05","amountMinor":850000,"currency":"IDR"}"#
        );
    }

    #[test]
    fn the_weekend_rate_is_added_then_stepped_across_the_boundary() {
        // The whole reason the rates slice was chosen for p2: 850,000 x 1.15 is
        // 977499.9999999999 in f64, which rounds *down*. Both engines must agree on
        // 978,000 here, so this case is the one that would expose a float path.
        let saturday = ask(r#"{"op":"nightlyRate","unitId":"garden-twin","night":"2026-10-10"}"#);
        assert_eq!(saturday["amountMinor"], 978_000);
        let friday = ask(r#"{"op":"nightlyRate","unitId":"garden-twin","night":"2026-10-09"}"#);
        assert_eq!(friday["amountMinor"], 978_000);
        let deluxe = ask(r#"{"op":"nightlyRate","unitId":"deluxe-valley","night":"2026-10-10"}"#);
        assert_eq!(deluxe["amountMinor"], 1_438_000);
    }

    #[test]
    fn an_absent_answer_is_null_not_zero_and_not_an_empty_object() {
        // The TypeScript accessors return `undefined`; `JSON.stringify` turns that
        // into `null` at the top level. Zero would be a real rate, and `{}` would be
        // a real shape, so both are ruled out explicitly.
        assert_eq!(
            ask(r#"{"op":"nightlyRate","unitId":"penthouse","night":"2026-10-05"}"#),
            serde_json::Value::Null
        );
        assert_eq!(
            ask(r#"{"op":"nightlyRate","unitId":"garden-twin","night":"2026-10-11"}"#),
            serde_json::Value::Null,
            "the checkout is a legal bound but not a night"
        );
        assert_eq!(
            ask(r#"{"op":"roomsBuilt","unitId":"penthouse"}"#),
            serde_json::Value::Null
        );
    }

    #[test]
    fn a_closed_night_reports_no_rooms_left() {
        let closed =
            ask(r#"{"op":"nightlyAvailability","unitId":"garden-twin","night":"2026-10-08"}"#);
        assert_eq!(closed["available"], false);
        assert_eq!(closed["unitsLeft"], 0);
        let closed_again =
            ask(r#"{"op":"nightlyAvailability","unitId":"garden-twin","night":"2026-10-09"}"#);
        assert_eq!(closed_again["unitsLeft"], 0);
        let partial =
            ask(r#"{"op":"nightlyAvailability","unitId":"deluxe-valley","night":"2026-10-09"}"#);
        assert_eq!(partial["available"], true);
        assert_eq!(partial["unitsLeft"], 2);
    }

    #[test]
    fn the_window_end_is_exclusive_and_an_unstated_end_prices_nothing() {
        let nights = ask(r#"{"op":"nightsBetween","from":"2026-10-05","to":"2026-10-11"}"#);
        assert_eq!(
            nights,
            serde_json::json!([
                "2026-10-05",
                "2026-10-06",
                "2026-10-07",
                "2026-10-08",
                "2026-10-09",
                "2026-10-10"
            ]),
            "the checkout is not itself a night"
        );
        assert_eq!(
            ask(r#"{"op":"nightsBetween","from":"2026-10-05","to":"2026-10-12"}"#),
            serde_json::json!([])
        );
        assert_eq!(
            ask(
                r#"{"op":"stayQuote","unitId":"garden-twin","from":"2026-10-05","to":"2026-10-12"}"#
            ),
            serde_json::Value::Null,
            "a stay the fixtures do not state prices nothing, rather than extrapolating"
        );
    }

    #[test]
    fn a_full_stay_prices_every_night_and_taxes_the_subtotal() {
        let quote = ask(
            r#"{"op":"stayQuote","unitId":"garden-twin","from":"2026-10-05","to":"2026-10-11"}"#,
        );
        // 4 x 850,000 + 2 x 978,000 = 5,356,000; PPN 11% = 589,160.
        assert_eq!(quote["subtotalMinor"], 5_356_000);
        assert_eq!(quote["taxMinor"], 589_160);
        assert_eq!(quote["totalMinor"], 5_945_160);
        assert_eq!(
            quote["nights"]
                .as_array()
                .expect("the nights are listed")
                .len(),
            6
        );
    }

    #[test]
    fn only_a_sellable_every_night_unit_is_bookable_and_cheapest_comes_first() {
        let bookable = ask(r#"{"op":"bookableStays","from":"2026-10-05","to":"2026-10-11"}"#);
        let ids: Vec<&str> = bookable
            .as_array()
            .expect("a list")
            .iter()
            // Flat, not `stay["quote"]["unitId"]`: the baseline spreads the quote into
            // the entry, and the first port's nested shape diverged on all 42 sellable
            // windows of the parity harness.
            .map(|stay| stay["unitId"].as_str().expect("a unit id"))
            .collect();
        assert_eq!(ids, vec!["deluxe-valley"]);

        let cheapest = ask(r#"{"op":"cheapestBookable","from":"2026-10-05","to":"2026-10-11"}"#);
        let totals: Vec<i64> = cheapest
            .as_array()
            .expect("a list")
            .iter()
            .map(|stay| stay["totalMinor"].as_i64().expect("a total"))
            .collect();
        assert_eq!(totals.len(), 1);
        assert!(
            totals.windows(2).all(|pair| pair[0] <= pair[1]),
            "sorted ascending"
        );

        // The stated window must never empty the listing page, which is the
        // regression the baseline records.
        assert!(!bookable.as_array().expect("a list").is_empty());
    }

    #[test]
    fn a_predicate_op_answers_a_boolean() {
        assert_eq!(
            ask(r#"{"op":"isWeekendNight","night":"2026-10-10"}"#),
            serde_json::json!(true)
        );
        assert_eq!(
            ask(r#"{"op":"isWeekendNight","night":"2026-10-05"}"#),
            serde_json::json!(false)
        );
        assert_eq!(
            ask(r#"{"op":"isWeekendNight","night":"2026-10-11"}"#),
            serde_json::json!(false)
        );
        assert_eq!(
            ask(r#"{"op":"isReferenceNight","night":"2026-10-07"}"#),
            serde_json::json!(true)
        );
        assert_eq!(
            ask(r#"{"op":"isReferenceNight","night":"2026-10-11"}"#),
            serde_json::json!(false)
        );
    }

    #[test]
    fn an_op_the_table_does_not_name_is_refused_rather_than_guessed() {
        // Defaulting to some plausible op would make a harness typo look like a
        // parity result — the worst possible failure, because it would be counted.
        let error = rates_json(r#"{"op":"nightlyRates"}"#.to_string())
            .expect_err("a near-miss op name is not an op");
        assert!(error.reason.contains("nightlyRates"));
    }

    #[test]
    fn a_request_missing_its_argument_names_the_argument() {
        let error = rates_json(r#"{"op":"nightlyRate","unitId":"garden-twin"}"#.to_string())
            .expect_err("a night is required");
        assert!(
            error.reason.contains("night"),
            "the error names the gap: {}",
            error.reason
        );
    }

    #[test]
    fn a_request_that_is_not_json_is_rejected_explicitly() {
        let error = rates_json("{ nope".to_string()).expect_err("a broken request answers nothing");
        assert!(error.to_string().contains("rates request"));
    }
}
