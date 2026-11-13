//! Generates the list of installed business modules from the module registry.
//!
//! The registry snapshot that `archava-domain` compiles in is read here too, so
//! the command surface this crate mounts and the registry the running server
//! consults always describe the same modules. The output is one macro,
//! `for_each_erp_module!`, which hands `("<id>", <source>)` pairs to a callback
//! macro. `business.rs` and `routes/erp.rs` expand it into the agent dispatch
//! and the `/erp/<id>/commands` routes, so neither holds a per-module list.
//!
//! A module declared in the registry must therefore expose `command` and
//! `status` handlers in `src/erp/<source>.rs`; a missing one is a compile
//! error that names the path.
use std::{env, fs, path::PathBuf};

const REGISTRY: &str = "../domain/embedded/modules.v1.json";

fn is_route_segment(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_module_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn main() {
    println!("cargo:rerun-if-changed={REGISTRY}");
    println!("cargo:rerun-if-changed=build.rs");
    let text = fs::read_to_string(REGISTRY)
        .unwrap_or_else(|error| panic!("module registry {REGISTRY}: {error}"));
    let registry: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("module registry {REGISTRY}: {error}"));
    let modules = registry["modules"]
        .as_array()
        .unwrap_or_else(|| panic!("module registry {REGISTRY}: `modules` must be an array"));
    let mut pairs = String::new();
    for module in modules {
        let id = module["id"].as_str().unwrap_or_default();
        let source = module["source"].as_str().unwrap_or_default();
        // Both values are spliced into Rust source, so they are held to the
        // same shapes `tools/check-modules.mjs` requires of the registry.
        assert!(
            is_route_segment(id),
            "module registry {REGISTRY}: id {id:?} must be lower-case alphanumeric"
        );
        assert!(
            is_module_name(source),
            "module registry {REGISTRY}: source {source:?} of {id} must be a Rust module name"
        );
        pairs.push_str(&format!("            ({id:?}, {source}),\n"));
    }
    let generated = format!(
        "/// Calls `$callback!` with one `(\"<id>\", <source>)` pair per module in the\n\
         /// registry, in registry order.\n\
         macro_rules! for_each_erp_module {{\n    ($callback:ident) => {{\n        $callback! {{\n{pairs}        }}\n    }};\n}}\n"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    fs::write(out.join("erp_modules.rs"), generated).expect("write the generated module list");
}
