# Archava Developer Onboarding & Contribution Guide

This guide describes how to configure your local workstation, run the persistent Rust server, execute tests, and implement new enterprise domain engines.

---

## 1. Prerequisites & Toolchain Setup

Archava requires a modern Rust toolchain with fast incremental linking:

### Required Tools
* **Rust:** 1.88+ (Stable toolchain with `clippy` and `rustfmt`)
* **Mold:** 3.0+ High-speed linker
* **Sccache:** Shared compiler cache
* **PostgreSQL:** 16+ with `pg_crypto`

### Installation

#### Arch Linux:
```bash
sudo pacman -S mold clang sccache postgresql
```

#### Ubuntu / Debian:
```bash
sudo apt update
sudo apt install -y mold clang postgresql
cargo install sccache
```

### Toolchain Configuration (`.cargo/config.toml`)
This configuration is already included in the workspace root:
```toml
[build]
rustc-wrapper = "sccache"

[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

---

## 2. Quickstart

```bash
# 1. Clone repository
cd /home/haikaru/Archverse/Lab/Archava

# 2. Build Rust workspace
cargo build
```

---

## 3. Running Development Servers

### A. Run Persistent Rust Host + Leptos Cockpit
```bash
# Sets up environment and launches the Axum HTTP host (:8347)
export DATABASE_URL="postgres://postgres:postgres@localhost:5432/archava"
export ARCHAVA_IDENTITY_KEY_HEX="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

# Launch binary directly
cargo run --bin archava-server
```
Navigate to `http://localhost:8347/app/erp` to access the interactive multi-pillar ERP cockpit.

---

## 4. Verification & Quality Gates (Zero Tolerance)

All pull requests and commits must pass every gate with **zero errors and zero warnings**:

```bash
# Gate 1: Rust Domain Unit Tests (156 tests in ~0.1s)
cargo test -p archava-domain --lib

# Gate 2: Rust Host Authority & Webhook Tests (67 tests)
cargo test -p archava-host --lib

# Gate 3: Ava Cockpit Durable Task Engine Tests (3 tests)
cargo test -p archava-cockpit --lib

# Gate 4: Ava Orbz Multi-Layer Memory Engine Tests (13 tests)
cargo test -p archava-orbz --lib

# Gate 5: Complete Workspace Test Suite (387 tests)
cargo test --workspace --lib

# Gate 6: Rust Workspace Clippy (Zero warnings enforced)
cargo clippy --workspace --all-targets -- -D warnings

# Gate 7: Rust Code Formatting Check
cargo fmt --all --check
```

---

## 5. How to Add a New Business Module

To add a new industry pillar (e.g. `dental_clinic` or `logistics`):

### Step 1: Define Domain Invariants & Models
Create [`crates/domain/src/erp/dental_clinic.rs`](crates/domain/src/erp/dental_clinic.rs):
```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DentalPatientRecord {
    pub patient_id: String,
    pub odontogram_chart: Vec<u8>,
    pub treatment_plan_cents: i64,
}

impl DentalPatientRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.patient_id.trim().is_empty() {
            return Err("Patient ID cannot be empty");
        }
        Ok(())
    }
}
```
Expose the module in [`crates/domain/src/erp/mod.rs`](crates/domain/src/erp/mod.rs).

### Step 2: Implement Database Persistence (RLS Enforced)
In `crates/adapters/src/erp/`:
* Implement transactional repositories taking `&mut TenantTransaction`.
* Ensure every table carries `tenant_id` and has Row-Level Security enabled.

### Step 3: Register Action in CommandAuthorizer
In `crates/domain/src/authority.rs`:
* Define capability tier (`L1` to `L5`).
* Register required permission scopes.

### Step 4: Write Regression Tests
Every module requires comprehensive test coverage:
* Positive happy paths.
* Negative unauthorized / cross-tenant refusal tests.
* Boundary constraint checks.

---

## 6. Git Workflow & Commit Rules

* **Branching:** Work on feature branches (`feat/module-name`).
* **Format Before Commit:**
  ```bash
  cargo fmt --all
  ```
* **No Broken Commits:** Never push a commit that breaks `cargo test` or `cargo clippy`.
