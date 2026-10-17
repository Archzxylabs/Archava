//! The pricing engine port — a line-for-line Rust translation of
//! `packages/pricing/src/engine.ts`.
//!
//! ## Why this exists
//!
//! The pilot asks one question: on three computational slices of Archava, is
//! Rust meaningfully faster than the shipped TypeScript, and at what cost in
//! faithfulness? Answering it needs a port that is *comparable* — not a fresh
//! implementation that happens to agree on the examples the tests use. So the
//! rule throughout is: reproduce what the baseline does, including where that
//! is surprising, and document every deliberate divergence in the module that
//! makes it.
//!
//! ## What is deliberately NOT reproduced
//!
//! - **Zod.** The engine consumes an already-validated `QuoteRequest`, so the
//!   port takes a `QuoteRequest` value directly. Validation is the
//!   configuration layer's job (`packages/config/src/pricing/schema.ts`) and
//!   porting it would mean re-implementing a library the baseline did not
//!   write either. Divergences that only a malformed pricebook can reach are
//!   recorded in `pricebook.rs`.
//! - **`includedStandardIntegrations`'s unguarded region lookup.** The
//!   baseline indexes `pricebook.regions[region]` there and throws a raw
//!   `TypeError` for an unknown region, while `requireRegion` throws a
//!   `PricingError` for the same input earlier in the same call. The Rust port
//!   returns `0` instead. The branch is unreachable through `quote()` because
//!   `requireRegion` runs first, so the difference is unobservable in the
//!   API the pilot measures. See `engine.rs`.
//! - **Money as integers.** `rounding.ts` is an f64 engine end to end. A
//!   decimal or integer-minor-unit port would produce different results on
//!   the baseline's own reference cases and stop being comparable, so `f64`
//!   stays. See `jsmath.rs`.
//!
//! ## Layout
//!
//! | module | role |
//! |---|---|
//! | [`jsmath`] | `Math.max` / `Math.round` semantics Rust's `f64` gets wrong |
//! | [`pricebook`] | the deserialized `config/pricing.v1.json` |
//! | [`classify`] | `classifyLine` — how a tier's price is expressed |
//! | [`request`] | `QuoteRequest` / `QuoteRequestInit` |
//! | [`engine`] | `PricingEngine` — Steps A–H and the quote it returns |
//! | [`error`] | `PricingError` |

pub mod classify;
pub mod engine;
pub mod error;
pub mod jsmath;
pub mod pricebook;
pub mod request;
