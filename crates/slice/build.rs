//! Generates the migrations the production server embeds from the manifest.
//!
//! `migrations/manifest.v1.json` is the list the Ops installer applies. The
//! server must refuse to serve a database whose history differs from the build,
//! so it embeds the same files and derives their checksums at startup. Reading
//! the manifest here means a new migration is embedded by adding it to the
//! manifest alone; there is no second list in the server to keep in step.
//!
//! The output is `EMBEDDED_LEDGERS`: one `(namespace, [(id, bytes)])` entry per
//! ledger the server verifies, in manifest order.
use std::{env, fs, path::PathBuf};

const MANIFEST: &str = "../../migrations/manifest.v1.json";

/// Ledgers the installer applies but the server's readiness profile does not
/// verify. They are left out so the binary embeds exactly what it checks.
const NOT_VERIFIED_AT_STARTUP: &[&str] = &["archava_provider", "archava_control"];

fn is_name(text: &str, extra: &[char]) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra.contains(&c))
}

fn main() {
    println!("cargo:rerun-if-changed={MANIFEST}");
    println!("cargo:rerun-if-changed=build.rs");
    let text = fs::read_to_string(MANIFEST)
        .unwrap_or_else(|error| panic!("migration manifest {MANIFEST}: {error}"));
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("migration manifest {MANIFEST}: {error}"));
    let ledgers = manifest["ledgers"]
        .as_array()
        .unwrap_or_else(|| panic!("migration manifest {MANIFEST}: `ledgers` must be an array"));
    let mut generated = String::from(
        "/// One ledger: its schema and its `(id, bytes)` migrations in apply order.\n\
         type EmbeddedLedger = (&'static str, &'static [(&'static str, &'static [u8])]);\n\n\
         /// The migrations of every ledger this server verifies before serving,\n\
         /// generated from `migrations/manifest.v1.json`.\n\
         const EMBEDDED_LEDGERS: &[EmbeddedLedger] = &[\n",
    );
    for ledger in ledgers {
        let namespace = ledger["namespace"].as_str().unwrap_or_default();
        assert!(
            is_name(namespace, &['_']),
            "migration manifest {MANIFEST}: namespace {namespace:?} is not a schema name"
        );
        if NOT_VERIFIED_AT_STARTUP.contains(&namespace) {
            continue;
        }
        let migrations = ledger["migrations"].as_array().unwrap_or_else(|| {
            panic!("migration manifest {MANIFEST}: {namespace} has no migrations")
        });
        generated.push_str(&format!("    ({namespace:?}, &[\n"));
        for migration in migrations {
            let id = migration["id"].as_str().unwrap_or_default();
            let path = migration["path"].as_str().unwrap_or_default();
            // Both values are spliced into Rust source and a file path, so the
            // path is held to `migrations/<directory>/<id>.sql`.
            let directory = path
                .strip_prefix("migrations/")
                .and_then(|rest| rest.strip_suffix(&format!("/{id}.sql")))
                .unwrap_or_default();
            assert!(
                is_name(id, &['_']) && is_name(directory, &['-', '_']),
                "migration manifest {MANIFEST}: {namespace} entry {id:?} at {path:?} \
                 must be migrations/<directory>/<id>.sql"
            );
            println!("cargo:rerun-if-changed=../../{path}");
            generated.push_str(&format!(
                "        ({id:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../{path}\"))),\n"
            ));
        }
        generated.push_str("    ]),\n");
    }
    generated.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    fs::write(out.join("embedded_ledgers.rs"), generated)
        .expect("write the generated migration list");
}
