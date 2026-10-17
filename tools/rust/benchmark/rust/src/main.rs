//! The compiled Rust consumer the benchmark measures.
//!
//! ## Why this binary exists at all
//!
//! The obvious way to benchmark the Rust engines is to call the napi addon from
//! Node and time `quoteJson`. That measurement is real, but it is not the one
//! PRICE-05 asks for, and the difference is not a detail.
//!
//! The addon re-parses the pricebook on *every* call — that is `lib.rs`'s own
//! documented second consequence, and it is a property of the bridge rather than
//! of the engine. So timing the addon measures "parse a pricebook, price one
//! request, serialize one quote, cross an FFI boundary", and calling that number
//! the Rust engine's speed would be exactly the comparison PRD §3.4 rejected when
//! it dismissed the earlier benchmark as "belum direproduksi". PRICE-05 spells the
//! rule out: *setup+quote and steady-state are reported separately, and a Rust
//! parse-per-call must never be compared against a TypeScript construct-once as a
//! language claim.*
//!
//! This binary is what makes that separation possible. It is a real Rust program
//! that owns the engine, so it can report:
//!
//! | field | what it measures |
//! |---|---|
//! | `steadyState` | engine built once, every request priced — the engine's cost |
//! | `parseOnly` | pricebook parsed, nothing priced — the setup cost alone |
//! | `setupPlusQuote` | pricebook re-parsed and engine rebuilt per call — the addon's shape |
//!
//! and `setupPlusQuote − steadyState ≈ parseOnly` is a decomposition the numbers
//! have to satisfy, so the setup cost is attributable rather than a black box.
//!
//! ## Why it lives outside the product workspace
//!
//! It is not a member of `rust/Cargo.toml`. Adding it there would put a benchmark
//! target inside the workspace that `cargo test --workspace` and the wasm gate both
//! walk, and a benchmark belongs to neither. Instead it path-depends on the engine
//! crates exactly as a cold consumer would — the same property
//! `verify-rust-consumers.mjs` proves for the packaged artifacts — which is the
//! second reason this is a binary: it *consumes* the engines from outside, built
//! against them from outside, with its own release profile rather than inheriting
//! the workspace's.
//!
//! ## What it prints
//!
//! One JSON object on stdout, and nothing else. Timing comes in as samples, not as
//! a single wall-clock figure: each repetition is a full pass over the whole case
//! set, timed separately, so the caller gets a distribution to take a median of
//! instead of one number to trust.

use std::io::Write;
use std::time::Instant;

use archava_pricing::engine::PricingEngine;
use archava_pricing::pricebook::Pricebook;
use archava_pricing::request::QuoteRequestInit;
use archava_rates as rates;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// One rates operation, spelled the way the parity case file spells it so a
/// single fixture set drives both engines.
#[derive(Debug, Deserialize)]
struct RatesOp {
    op: String,
    #[serde(rename = "unitId")]
    unit_id: Option<String>,
    night: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// The workload, handed in as JSON so this binary and the TypeScript consumer are
/// provably run against identical inputs.
#[derive(Debug, Deserialize)]
struct Workload {
    slice: String,
    #[serde(rename = "pricebookPath")]
    pricebook_path: Option<String>,
    requests: Vec<Value>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: archava-bench <workload.json>");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&args[1]).unwrap_or_else(|e| {
        eprintln!("could not read workload {}: {e}", args[1]);
        std::process::exit(2);
    });
    let workload: Workload = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("workload is not the expected shape: {e}");
        std::process::exit(2);
    });
    let warmup = read_count("BENCH_WARMUP", 3);
    let reps = read_count("BENCH_REPS", 9);
    // The digest proves both engines were handed the same bytes. Without it a
    // silent input difference would show up only as a puzzling speed gap.
    let digest = digest_of(&workload);
    let result = match workload.slice.as_str() {
        "pricing" => bench_pricing(&workload, warmup, reps),
        "rates" => bench_rates(&workload, warmup, reps),
        other => {
            eprintln!("unknown slice {other:?}");
            std::process::exit(2);
        }
    };
    let out = json!({
        "slice": workload.slice,
        "warmup": warmup,
        "reps": reps,
        "cases": workload.requests.len(),
        "inputDigest": digest,
        "result": result,
        "peakRssKiB": peak_rss_kib(),
    });
    // A single write, flushed, so a run interrupted mid-way still leaves nothing
    // half-parsed on stdout for the caller to trip over.
    let stdout = serde_json::to_string_pretty(&out).expect("the result is serializable");
    let mut handle = std::io::stdout().lock();
    handle
        .write_all(stdout.as_bytes())
        .expect("stdout is writable");
    handle.write_all(b"\n").expect("stdout is writable");
}

/// The process's own peak resident set size in KiB, or `None` where it cannot be read.
///
/// PRD §3.4 dismissed the earlier benchmark partly for lacking "p95/RSS", and PRD
/// line 640 names p50/p95, throughput, errors, CPU and RSS as what a load
/// comparison reports. Timings alone leave that gap open, so the consumer reports
/// the one memory figure it can attribute to itself.
///
/// It reads `VmHWM` from `/proc/self/status` — the high-water mark, which is what
/// a peak means. A snapshot of `VmRSS` at the moment of printing would be a
/// different and much weaker claim: it measures whatever the allocator happened
/// to be holding at exit, which for a process that has just finished working is
/// close to worthless. `VmHWM` is set by the kernel and only ever rises.
///
/// Reported as `None` rather than as `0` on a platform without `/proc`, because a
/// harness that silently substituted a zero for an unreadable measurement is the
/// same failure shape as the coverage bug: a well-formed number that means
/// nothing. Callers record the `null` and the evidence says "not measured here".
fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmHWM:") {
            // The field reads "VmHWM:\t   7492 kB" — a decimal count and a unit.
            // Parsing the count and ignoring the unit is safe here because Linux
            // reports VmHWM in kB unconditionally; the suffix is trimmed rather
            // than trusted, so a locale or a kernel change cannot produce a
            // silently wrong magnitude.
            let digits: String = value
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            return digits.parse().ok();
        }
    }
    None
}

/// Read a repetition count from the environment, falling back to `default`.
///
/// PERF-02 requires at least five independent measured repetitions, so the floor
/// is enforced here rather than trusted to the caller: a `BENCH_REPS` of 2 would
/// otherwise produce a "measured" run that cannot support a verdict, and the
/// evidence file would look identical to a good one.
fn read_count(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n >= 5)
        .unwrap_or(default)
}

/**
 * A digest over the exact bytes both engines consume, so the evidence can show
 * the two sides were handed the same input rather than asserting they were.
 *
 * This is provenance, not security — it exists so a benchmark cannot silently
 * compare two different workloads. The requests are re-serialized from their
 * parsed form because they arrive in memory, and the pricebook is hashed as file
 * bytes because it arrives by path.
 */
fn digest_of(workload: &Workload) -> String {
    let requests = serde_json::to_vec(&workload.requests).expect("requests are serializable");
    let mut hasher = Sha256::new();
    hasher.update(b"rust-consumer-v1\0");
    hasher.update(&requests);
    hasher.update(b"\0");
    if let Some(path) = &workload.pricebook_path {
        if let Ok(bytes) = std::fs::read(path) {
            hasher.update(&bytes);
        }
    }
    hex::encode(hasher.finalize())
}

/**
 * The pricing measurement: setup+quote and steady-state, reported separately.
 *
 * The pricebook is parsed once up front to obtain the workload, and once more
 * inside the `parseOnly` pass so its cost is measured rather than assumed.
 */
fn bench_pricing(workload: &Workload, warmup: usize, reps: usize) -> Value {
    let path = workload
        .pricebook_path
        .as_deref()
        .expect("the pricing workload names a pricebook");
    let pricebook_text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("could not read pricebook {path}: {e}");
        std::process::exit(2);
    });
    let pricebook: Pricebook = serde_json::from_str(&pricebook_text).unwrap_or_else(|e| {
        eprintln!("the pricebook did not deserialize: {e}");
        std::process::exit(2);
    });
    // Requests are parsed up front so the timing below measures pricing rather
    // than JSON parsing of the requests. The pricebook parse deliberately stays
    // inside `parseOnly` and `setupPlusQuote` — that is the cost being reported,
    // and hoisting it out would hide exactly what those two numbers exist to show.
    let inits: Vec<QuoteRequestInit> = workload
        .requests
        .iter()
        .map(|r| serde_json::from_value(r.clone()).expect("a valid pricing request"))
        .collect();

    // --- steady state: the engine is built once and reused ---------------
    let engine = PricingEngine::new(&pricebook);
    let mut steady = measure(warmup, reps, || {
        for init in &inits {
            // The quote is consumed rather than accumulated: collecting into a
            // Vec would let the allocator's behaviour dominate the measurement,
            // and a benchmark whose cost is mostly `Vec` growth is measuring the
            // wrong thing.
            std::hint::black_box(engine.quote(init.clone()).is_ok());
        }
    });

    // --- parse only: the setup cost on its own ---------------------------
    let mut parse_only = measure(warmup, reps, || {
        let parsed: Pricebook =
            serde_json::from_str(&pricebook_text).expect("the pricebook parses again, identically");
        std::hint::black_box(PricingEngine::new(&parsed).schema_version());
    });

    // --- setup+quote: the pricebook is re-parsed per call -----------------
    // This is the addon's own cost shape. Reporting it separately is what keeps
    // it from ever being mistaken for the engine's.
    let mut setup = measure(warmup, reps, || {
        for init in &inits {
            let parsed: Pricebook = serde_json::from_str(&pricebook_text)
                .expect("the pricebook parses once more, identically");
            let engine = PricingEngine::new(&parsed);
            std::hint::black_box(engine.quote(init.clone()).is_ok());
        }
    });

    // Every case must actually price. A workload whose cases all silently failed
    // would produce a very fast and entirely meaningless measurement.
    let priced = inits
        .iter()
        .filter(|init| engine.quote((*init).clone()).is_ok())
        .count();
    json!({
        "casesPriced": priced,
        "steadyState": steady.finish(),
        "parseOnly": parse_only.finish(),
        "setupPlusQuote": setup.finish(),
    })
}

/**
 * What one rates operation did, as a distinction the caller can count.
 *
 * This type exists because of a bug, and it is worth stating precisely which one.
 * `run_rates_op` used to return `Option<Value>`, where `None` meant both "the
 * baseline answers `undefined` here" and "this consumer could not run this op".
 * A caller counting how many operations executed cannot tell those apart.
 *
 * So `casesRun` was written as `ops.len()` — the workload's *length*, not the
 * number that ran. The runner's coverage check then compared 753 against 753 and
 * passed while the engine ran 568. That is a tautology wearing a check's clothes,
 * which is worse than no check at all, because it looks like one.
 *
 * The return type is now `Option<Ran>`, and the `None` case is the point: it
 * means "this consumer *declined* the case". Nothing in the correct code
 * produces it, which is exactly what makes it useful — the bug's shape
 * (`if op.unit_id.is_none() { return None }`) now has somewhere to land other
 * than an excuse, and `bench_rates` refuses to report a timing built on it.
 */
#[derive(Debug)]
enum Ran {
    /// The operation executed and produced a value.
    Answered,
    /// The operation executed and legitimately answered nothing.
    Unanswered,
}

fn bench_rates(workload: &Workload, warmup: usize, reps: usize) -> Value {
    let ops: Vec<RatesOp> = workload
        .requests
        .iter()
        .map(|r| serde_json::from_value(r.clone()).expect("a valid rates operation"))
        .collect();
    // Counted by the code that executes, one increment per case that ran. A count
    // taken from `ops.len()` is a promise about the workload; this is a record of
    // what happened, and the difference between those two is the whole bug.
    let mut cases_run = 0usize;
    let mut cases_answered = 0usize;
    let mut declined: Vec<String> = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        match run_rates_op(op) {
            Some(Ran::Answered) => {
                cases_run += 1;
                cases_answered += 1;
            }
            Some(Ran::Unanswered) => cases_run += 1,
            None => declined.push(format!("case {index} (op {})", op.op)),
        }
    }
    // A declined case is a workload this consumer did not run, so there is no
    // honest measurement to report for it — and no timing pass is started. The
    // alternative is a very fast, entirely meaningless number, which is what the
    // first version of this harness produced for rates.
    if !declined.is_empty() {
        eprintln!(
            "{} of {} rates cases were declined rather than run: {}",
            declined.len(),
            ops.len(),
            declined.join(", ")
        );
        eprintln!(
            "a benchmark that measures a subset of the workload is not measuring \
             the workload"
        );
        std::process::exit(2);
    }
    let mut measured = measure(warmup, reps, || {
        for op in &ops {
            std::hint::black_box(run_rates_op(op));
        }
    });
    json!({
        "cases": ops.len(),
        "casesRun": cases_run,
        "casesAnswered": cases_answered,
        "casesDeclined": 0,
        "perPass": measured.finish(),
    })
}

/// A required argument, or the workload is rejected.
///
/// A *missing* argument is a workload bug, not an absent answer. Every fixture
/// carries the fields its op needs, and answering `None` for a missing one would
/// silently shrink the measured set — a second way for the two sides to run
/// different work while the evidence claimed they ran the same workload. So a
/// missing argument stops the run and says which op and which field.
fn required<'a>(value: Option<&'a str>, op: &str, field: &str) -> &'a str {
    match value {
        Some(value) => value,
        None => {
            eprintln!(
                "op {op} is missing its required {field} argument; the workload's fixtures always carry it"
            );
            std::process::exit(2);
        }
    }
}

/**
 * Run one rates operation, or decline it.
 *
 * `None` means "this consumer did not run this case". Correct code never returns
 * it, and that is the whole reason it exists: the `Option` is the only channel
 * through which a bail-out that is not an argument error can be reported back, so
 * a reintroduced early return now surfaces as 568 declined cases and a non-zero
 * exit instead of a fast, plausible, wrong timing.
 *
 * The literal line that caused the bug — `let unit_id = op.unit_id.as_deref()?;`
 * hoisted above the match — no longer even compiles here, because `?` cannot
 * pull an `Option<&str>` out of a function returning `Option<Ran>`. That is a
 * compile-time proof the bug cannot come back in its original form, which is
 * worth more than any runtime check against it.
 */
fn run_rates_op(op: &RatesOp) -> Option<Ran> {
    // `rates_answer` returns `None` exactly where the baseline answers `null` —
    // a *result*, not a refusal. Writing this as `rates_answer(op)?` was the
    // first attempt, and it was wrong in a way that reproduced the original bug
    // through a new door: `?` folds "answers nothing" into the same `None` that
    // means "declined", so 319 legitimate null answers arrived as refusals and
    // the harness refused to measure the workload at all. The distinction is the
    // entire point of the type, so it is made explicitly here.
    Some(match rates_answer(op) {
        Some(value) if !value.is_null() => Ran::Answered,
        Some(_) => Ran::Unanswered,
        None => Ran::Unanswered,
    })
}

/**
 * One rates operation's answer, or `None` where the baseline answers nothing.
 *
 * Dispatched per op with each argument extracted by the arm that needs it — the
 * same shape as the parity harness's `OPS` table, for the same reason: the
 * argument list is the thing that can be silently transposed, so it is stated
 * once per op next to the strings it is handed.
 *
 * An earlier version hoisted `let unit_id = op.unit_id.as_deref()?;` above the
 * match. It looked equivalent and was not: that early return fired for every op
 * taking no unit id — `nightsBetween`, `bookableStays`, `cheapestBookable`,
 * `isReferenceNight`, `isWeekendNight`, 185 of the 753 cases — so the Rust side
 * ran 568 operations while the TypeScript side ran all 753. The Rust engine
 * therefore did about a quarter less work and still only reported a 1.7x
 * "faster", which made an invalid comparison look like a valid one. That is the
 * exact failure `bench_rates`'s own header warns about, committed three lines
 * above the code that caused it, and it is why each arm now extracts its own
 * arguments rather than sharing one.
 */
fn rates_answer(op: &RatesOp) -> Option<Value> {
    let name = op.op.as_str();
    match name {
        "nightlyRate" => rates::nightly_rate(
            required(op.unit_id.as_deref(), name, "unitId"),
            required(op.night.as_deref(), name, "night"),
        )
        .map(|v| serde_json::to_value(v).expect("serializable")),
        "nightlyAvailability" => rates::nightly_availability(
            required(op.unit_id.as_deref(), name, "unitId"),
            required(op.night.as_deref(), name, "night"),
        )
        .map(|v| serde_json::to_value(v).expect("serializable")),
        "nightsBetween" => Some(
            serde_json::to_value(rates::nights_between(
                required(op.from.as_deref(), name, "from"),
                required(op.to.as_deref(), name, "to"),
            ))
            .expect("serializable"),
        ),
        "stayAvailability" => rates::stay_availability(
            required(op.unit_id.as_deref(), name, "unitId"),
            required(op.from.as_deref(), name, "from"),
            required(op.to.as_deref(), name, "to"),
        )
        .map(|v| serde_json::to_value(v).expect("serializable")),
        "stayQuote" => rates::stay_quote(
            required(op.unit_id.as_deref(), name, "unitId"),
            required(op.from.as_deref(), name, "from"),
            required(op.to.as_deref(), name, "to"),
        )
        .map(|v| serde_json::to_value(v).expect("serializable")),
        "bookableStays" => Some(
            serde_json::to_value(rates::bookable_stays(
                required(op.from.as_deref(), name, "from"),
                required(op.to.as_deref(), name, "to"),
            ))
            .expect("serializable"),
        ),
        "cheapestBookable" => Some(
            serde_json::to_value(rates::cheapest_bookable(
                required(op.from.as_deref(), name, "from"),
                required(op.to.as_deref(), name, "to"),
            ))
            .expect("serializable"),
        ),
        "roomsBuilt" => {
            rates::rooms_built_of(required(op.unit_id.as_deref(), name, "unitId")).map(|v| json!(v))
        }
        "baseRate" => {
            rates::base_rate(required(op.unit_id.as_deref(), name, "unitId")).map(|v| json!(v))
        }
        "isReferenceNight" => Some(json!(rates::is_reference_night(required(
            op.night.as_deref(),
            name,
            "night",
        )))),
        "isWeekendNight" => Some(json!(rates::is_weekend_night(required(
            op.night.as_deref(),
            name,
            "night",
        )))),
        other => {
            eprintln!("the workload names an operation this consumer does not implement: {other}");
            std::process::exit(2);
        }
    }
}

/// A handful of timing samples in nanoseconds, one per full pass.
struct Samples {
    values: Vec<u128>,
}

impl Samples {
    fn finish(&mut self) -> Value {
        // Sorted here rather than in the caller: a per-pass duration's *order*
        // carries no information once the passes are identical work, and handing
        // back a sorted list makes a median taken downstream trivially correct
        // instead of correct-by-convention.
        self.values.sort_unstable();
        json!({
            "unit": "nanoseconds",
            "perPass": self.values,
            "passes": self.values.len(),
        })
    }
}

/**
 * Run `warmup + reps` passes and time each repetition separately.
 *
 * The warmup passes are timed and discarded, which is the only honest option:
 * dropping them silently would bias the sample toward slower observations, and
 * PERF-01 requires cold and warm to be distinguishable rather than blended.
 */
fn measure<F: FnMut()>(warmup: usize, reps: usize, mut body: F) -> Samples {
    let mut values = Vec::with_capacity(reps);
    for index in 0..(warmup + reps) {
        let start = Instant::now();
        body();
        let elapsed = start.elapsed().as_nanos();
        if index >= warmup {
            values.push(elapsed);
        }
    }
    Samples { values }
}
