//! The operator CLI binary (`archava-ops`). One subcommand, one JSON line,
//! exit 0/2/1 — see `archava_ops::cli` for the dispatch and every command's
//! proving note in `runbook::table()`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(archava_ops::cli::run_cli(args));
}
