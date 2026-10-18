//! R10's operational crate: the Rust replacement for the first-party
//! Node/Python operator commands.
//!
//! ## What belongs here, and what never will
//!
//! The platform's operator surface was a set of Node entry points
//! (`apps/crm/src/operator.ts` — `status`, `doctor`, `backup`/`backup-status`,
//! `restore`, plus the release-side `pack`/`manifest`/assembly verifiers).
//! R10's exit says that surface becomes Rust CLI commands, with **exact
//! identity proofs, drained/fenced writer handover, rollback rehearsal, local
//! operator runbook** (PRD §23 G10), and the MIG rules say what the rehearsals
//! must actually prove (MIG-03 fresh + prior, MIG-04 isolated restore with no
//! secret printing, MIG-05 single writer with a stale process refused, MIG-08
//! drained rollback within a compat window, MIG-09 pending-action inventory).
//!
//! Four modules split the job by authority:
//!
//! - [`release`] — the release-identity verifier. Reads the CONFIG source of
//!   truth (not the dist copy), recomputes every artifact digest, and scans
//!   the prior archives to prove the "never overwrite 1.17.0" rule (VER-04)
//!   by reading them, not by trusting a flag.
//! - [`lease`] — the fenced single-writer lease (MIG-05). A persistence-
//!   boundary primitive, not a process lock: the fence checks live in the
//!   same PostgreSQL transaction as the write, so a stale writer's write is
//!   refused where it executes, not where it started.
//! - [`backup`] — the capture/verify/restore-rehearsal machinery (OBS-04,
//!   MIG-04). A backup is not valid until its isolated restore was tested,
//!   and the restore refuses an active target.
//! - [`runbook`] — the registry of operator commands this crate replaces, in
//!   baseline order, so the gate can prove "commands replaced" by EXECUTING
//!   the table rather than quoting it.
//!
//! The CLI (`src/main.rs`) is a thin dispatch over these modules; every
//! subcommand prints ONE differential-safe JSON line and exits 0/2/1 (ok /
//! refusal / internal error), exactly the shape the TS operator printed.
//!
//! ## The boundary the wasm gate draws
//!
//! This crate is deliberately NOT on the portable list: it reads files, hash
//! digests and a live PostgreSQL. The wasm gate's claim is about the crates
//! the UI can reach — `archava-host` cannot compile for wasm32 and neither
//! can this; the portable list stays `core/domain/pricing/rates/assembly/
//! capacity/context` plus the planning leaves.

pub mod backup;
pub mod cli;
pub mod doctor;
pub mod lease;
pub mod release;
pub mod runbook;

/// The schema every rehearsal artifact line prints under, so the producers
/// and any operator grep see one stable shape.
pub const REPORT_SCHEMA: &str = "archava-ops/v1";
