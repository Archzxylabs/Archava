//! A5's second half: a *forced* long socket directory must fail with the
//! diagnosis, not with `pg_ctl: could not start server`.
//!
//! `ARCHAVA_PG_SOCKET_DIR` is honoured even when it does not fit — the override
//! exists so a diagnosis can be reproduced exactly, so it is never silently
//! repaired. What must not happen is the A5 symptom: `initdb` succeeds, the
//! server fails to start, and seventeen tests report `0 passed / N failed`
//! without a single line naming the socket.
//!
//! This lives in its own test binary because the variable is process-global: a
//! file is a process, so setting it here cannot race the provisioning test in
//! `socket_dir.rs`.

#![cfg(unix)]

use std::path::PathBuf;

use archava_pg_fixture::provision;

#[tokio::test]
#[should_panic(expected = "sockaddr_un.sun_path")]
async fn a_forced_long_socket_dir_names_the_sun_path_limit() {
    // 105 bytes plus the instance name plus `/.s.PGSQL.<port>`: far over 107.
    let forced = PathBuf::from(format!("/tmp/{}", "f".repeat(100)));
    std::env::set_var("ARCHAVA_PG_SOCKET_DIR", &forced);

    // Panics inside `instance_dir`, before `initdb` — so this assertion also
    // pins the fail-fast: no cluster, no data directory, no misleading start
    // failure. The panic message is the fixture's own `SocketPathTooLong`.
    provision().await;
}
