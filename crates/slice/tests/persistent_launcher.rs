//! Persistent-launcher lifecycle proof.
//!
//! The fixture owns PostgreSQL for the test, but the server under test does
//! not: two separate `archava-server` processes connect to the same runtime
//! URL. The first process is killed, the second starts from the same database,
//! and the identity row created before either process is still usable.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use archava_pg_fixture::provision;

const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn unused_loopback_addr() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve a loopback port")
        .local_addr()
        .expect("read reserved loopback port")
}

fn scratch_site() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "archava-persistent-launcher-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("pkg")).expect("create fake site payload");
    std::fs::write(
        root.join("pkg/archava-slice.js"),
        "// lifecycle test payload",
    )
    .expect("write fake wasm javascript");
    std::fs::write(
        root.join("pkg/archava-slice_bg.wasm"),
        b"lifecycle test payload",
    )
    .expect("write fake wasm binary");
    root
}

struct OwnedServer(Child);
impl std::ops::Deref for OwnedServer {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedServer {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for OwnedServer {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn spawn_server(
    database_url: &str,
    bind_addr: SocketAddr,
    site_root: &Path,
    takeover: bool,
) -> OwnedServer {
    OwnedServer(
        Command::new(env!("CARGO_BIN_EXE_archava-server"))
            .env("DATABASE_URL", database_url)
            .env("ARCHAVA_IDENTITY_KEY_HEX", "07".repeat(32))
            .env("ARCHAVA_BIND_ADDR", bind_addr.to_string())
            .env("ARCHAVA_SITE_ROOT", site_root)
            .env(
                "ARCHAVA_WRITER_TAKEOVER",
                if takeover { "true" } else { "false" },
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn persistent Rust server"),
    )
}

fn http_request(addr: SocketAddr, request: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.write_all(request.as_bytes())?;

    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        });
        let chunked = headers.lines().any(|line| {
            let Some((name, value)) = line.split_once(':') else {
                return false;
            };
            name.eq_ignore_ascii_case("transfer-encoding")
                && value
                    .split(',')
                    .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        });
        if content_length.is_some_and(|length| bytes.len() >= header_end + length)
            || (chunked
                && bytes[header_end..]
                    .windows(5)
                    .any(|window| window == b"0\r\n\r\n"))
        {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn wait_for_health(addr: SocketAddr, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(response) = http_request(
            addr,
            "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        ) {
            if response.starts_with("HTTP/1.1 200") {
                return;
            }
        }
        if let Some(status) = child.try_wait().expect("poll server process") {
            panic!("persistent Rust server exited before /healthz: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "persistent Rust server did not become ready"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn login(addr: SocketAddr, username: &str) -> io::Result<String> {
    let body = format!(r#"{{"username":"{username}","password":"correct horse battery staple"}}"#);
    let request = format!(
        "POST /login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         x-archava-client: cli\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    http_request(addr, &request)
}

#[tokio::test]
async fn persistent_server_restarts_against_the_same_database() {
    let fixture = provision().await;
    let username = format!("restart-{}@archava.test", std::process::id());
    sqlx::query(
        "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
         VALUES ($1, $2, $3, true, 1)",
    )
    .bind(format!("restart-{}", std::process::id()))
    .bind(&username)
    .bind(PASSWORD_HASH)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed restart identity");

    let bind_addr = unused_loopback_addr();
    let site_root = scratch_site();
    fixture
        .writer_lease
        .drain(&fixture.runtime_pool)
        .await
        .expect("drain the fixture writer before native startup");
    let mut first = spawn_server(&fixture.runtime_url(), bind_addr, &site_root, false);
    wait_for_health(bind_addr, &mut first);
    let first_login = login(bind_addr, &username);
    first.kill().expect("kill first persistent process");
    first.wait().expect("reap first persistent process");
    assert!(
        first_login
            .expect("the first persistent process must answer login")
            .starts_with("HTTP/1.1 200"),
        "the first persistent process must authenticate the seeded identity"
    );

    let mut second = spawn_server(&fixture.runtime_url(), bind_addr, &site_root, true);
    wait_for_health(bind_addr, &mut second);
    let second_login = login(bind_addr, &username);
    second.kill().expect("kill second persistent process");
    second.wait().expect("reap second persistent process");
    assert!(
        second_login
            .expect("the restarted process must answer login")
            .starts_with("HTTP/1.1 200"),
        "the restarted process must authenticate the same database identity"
    );
    std::fs::remove_dir_all(site_root).expect("remove lifecycle test site");
}
