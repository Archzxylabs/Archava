//! A5 regression, end to end: a real cluster must come up and bind its Unix
//! socket when `TMPDIR` is longer than `sockaddr_un.sun_path` allows.
//!
//! The unit tests in `src/lib.rs` prove the *decision* — `choose_socket_base`
//! picks a base whose socket path fits. This proves the *effect*: `initdb`,
//! `pg_ctl start`, and a socket file that exists at a path that fits. A decision
//! that is never exercised is exactly how the original failure shipped — `initdb`
//! succeeded, `pg_ctl` failed with `could not start server`, and every suite
//! reported `0 passed / N failed` without naming the socket.
//!
//! One test, one cluster, on purpose: the fixture serializes provisioning on
//! `PROVISION_SLOTS`, and the audit measured parallel clusters starving the box.

#![cfg(unix)]

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;

use archava_pg_fixture::{provision, socket_path, SUN_PATH_MAX};

/// The length of a path as the kernel counts it — bytes, not characters.
fn bytes(path: &Path) -> usize {
    path.as_os_str().as_bytes().len()
}

#[tokio::test]
async fn a_cluster_binds_its_socket_whatever_tmpdir_is() {
    let temp_dir = std::env::temp_dir();
    let fixture = provision().await;

    let socket = socket_path(fixture.dir(), fixture.port);

    // 1. The socket path fits in `sun_path`. This is the invariant the card is
    //    about, asserted on the path the server was actually started with.
    assert!(
        bytes(&socket) <= SUN_PATH_MAX,
        "socket path is {} bytes, over sun_path's {SUN_PATH_MAX}: {}",
        bytes(&socket),
        socket.display()
    );

    // 2. An exit code is not an effect: `pg_ctl start` succeeding means a socket
    //    was bound, so the socket must be there — and be a socket, not a file
    //    named like one.
    let file_type = std::fs::symlink_metadata(&socket)
        .map(|meta| meta.file_type())
        .unwrap_or_else(|error| {
            panic!(
                "pg_ctl reported success but nothing was bound at {} (socket dir {}, \
                 temp_dir() {}): {error}",
                socket.display(),
                fixture.dir().display(),
                temp_dir.display()
            )
        });
    assert!(
        file_type.is_socket(),
        "{} exists but is not a socket",
        socket.display()
    );

    // 3. Where the instance lives is the decision under test. When `temp_dir()`
    //    could not have held this socket it must be the `/tmp` fallback — the
    //    exact case this card was opened for. When it could, it must be
    //    `temp_dir()` itself, because the fallback is conditional and must not
    //    quietly become the new default.
    let name = fixture.dir().file_name().expect("fixture dir has a name");
    let would_be = socket_path(&temp_dir.join(name), fixture.port);
    if bytes(&would_be) > SUN_PATH_MAX {
        assert_eq!(
            fixture.dir().parent(),
            Some(Path::new("/tmp")),
            "temp_dir() ({}, {} bytes) cannot hold this socket, yet the instance is not under /tmp",
            temp_dir.display(),
            bytes(&temp_dir)
        );
    } else {
        assert_eq!(
            fixture.dir().parent(),
            Some(temp_dir.as_path()),
            "temp_dir() ({}) can hold this socket, so it should have been used",
            temp_dir.display()
        );
    }
}
