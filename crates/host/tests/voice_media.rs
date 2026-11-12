//! Actual host WebSocket -> certificate-verified owned Gemini WSS -> audio/video.
//! No paid provider, shared browser, application database or external media is used.
mod common;
use archava_host::{presence::PresenceRegistry, AppState, Ledger};
use archava_pg_fixture::provision;
use archava_providers::voice::{live::LiveConfig, VoiceBridgeConfig, VoiceClient};
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{
    accept_async, connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;
const TABLES: &[&str] = &[
    "memberships",
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "commands",
    "movements",
    "returns",
    "audit",
    "approvals",
];
struct Pki(std::path::PathBuf);
impl Drop for Pki {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn pki() -> (Pki, Vec<u8>, rustls::ServerConfig) {
    let directory = std::env::temp_dir().join(format!("archava-owned-wss-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let guard = Pki(directory);
    let run = |args: &[&str]| {
        let result = std::process::Command::new("openssl")
            .args(args)
            .current_dir(&guard.0)
            .output()
            .expect("openssl is required for owned TLS evidence");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    run(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "ca.key",
        "-out",
        "ca.pem",
        "-days",
        "1",
        "-subj",
        "/CN=Archava owned test CA",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
    ]);
    run(&[
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "server.key",
        "-out",
        "server.csr",
        "-subj",
        "/CN=localhost",
    ]);
    std::fs::write(guard.0.join("extensions"),"subjectAltName=IP:127.0.0.1,DNS:localhost\nbasicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").unwrap();
    run(&[
        "x509",
        "-req",
        "-in",
        "server.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-days",
        "1",
        "-out",
        "server.pem",
        "-extfile",
        "extensions",
    ]);
    run(&["x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der"]);
    run(&[
        "x509",
        "-in",
        "server.pem",
        "-outform",
        "DER",
        "-out",
        "server.der",
    ]);
    run(&[
        "pkcs8",
        "-topk8",
        "-nocrypt",
        "-in",
        "server.key",
        "-outform",
        "DER",
        "-out",
        "server.der.key",
    ]);
    let root = std::fs::read(guard.0.join("ca.der")).unwrap();
    let cert =
        rustls::pki_types::CertificateDer::from(std::fs::read(guard.0.join("server.der")).unwrap());
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        std::fs::read(guard.0.join("server.der.key")).unwrap(),
    ));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    (guard, root, config)
}
async fn next(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let frame = timeout(Duration::from_secs(10), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_slice(&frame.into_data()).unwrap()
}
#[tokio::test]
async fn verified_owned_wss_carries_pcm_video_and_model_audio_then_closes_on_revocation() {
    let (_pki, root, tls) = pki();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("wss://{}/live", listener.local_addr().unwrap());
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    let frames = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&frames);
    let mock = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let socket = acceptor.accept(socket).await.unwrap();
        let mut ws = accept_async(socket).await.unwrap();
        let setup: Value =
            serde_json::from_slice(&ws.next().await.unwrap().unwrap().into_data()).unwrap();
        assert_eq!(setup["setup"]["model"], "models/owned-live");
        assert_eq!(
            setup["setup"]["generationConfig"]["responseModalities"],
            json!(["AUDIO"])
        );
        ws.send(Message::Text(
            json!({"setupComplete":{}}).to_string().into(),
        ))
        .await
        .unwrap();
        while let Some(Ok(frame)) = ws.next().await {
            if frame.is_close() {
                break;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&frame.into_data()) else {
                continue;
            };
            if let Some(audio) = value["realtimeInput"].get("audio") {
                assert_eq!(audio["mimeType"], "audio/pcm;rate=16000");
                assert_eq!(
                    STANDARD.decode(audio["data"].as_str().unwrap()).unwrap(),
                    vec![0u8; 3200]
                );
                observed.fetch_add(1, Ordering::SeqCst);
                ws.send(Message::Text(json!({"serverContent":{"inputTranscription":{"text":"Owned synthetic input"},"outputTranscription":{"text":"Owned synthetic response"},"modelTurn":{"parts":[{"inlineData":{"mimeType":"audio/pcm;rate=24000","data":STANDARD.encode(vec![0u8;4800])}}]},"turnComplete":true}}).to_string().into())).await.unwrap();
            }
            if let Some(video) = value["realtimeInput"].get("video") {
                assert_eq!(video["mimeType"], "image/jpeg");
                assert_eq!(
                    STANDARD.decode(video["data"].as_str().unwrap()).unwrap(),
                    vec![255, 216, 1, 2, 255, 217]
                );
                observed.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    let f = provision().await;
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES('alice','alice@archava.test',$1,true,1)").bind(common::HASH).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail','alice','human',ARRAY['catalog.read'],'all',true)").execute(&f.admin_pool).await.unwrap();
    let voice = VoiceClient::new(VoiceBridgeConfig {
        livekit_url: "ws://127.0.0.1:7880".into(),
        api_key: "owned-livekit-key".into(),
        api_secret: vec![7; 32],
        request_timeout: Duration::from_secs(5),
        loopback_mode: false,
    })
    .with_live(
        LiveConfig::new(&endpoint, "owned-gemini-key", "owned-live")
            .unwrap()
            .with_owned_root(root)
            .unwrap(),
    );
    let state = AppState::try_new(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "commerce",
            tables: TABLES,
        },
        &f.commerce_ledger,
        &f.identity_ledger,
        &[7; 32],
    )
    .await
    .unwrap()
    .with_presence_registry(PresenceRegistry::new(voice));
    let app = archava_host::router(state);
    let session = common::login(&app, "alice").await;
    let (status, presence) = common::call(
        &app,
        "POST",
        "/presence/sessions",
        json!({"tenant_id":"acme-retail","session_id":"owned-media","mode":"human"}),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{presence}");
    assert_eq!(presence["media_transport"], "gemini_live_pcm");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "ws://{}/presence/sessions/owned-media/stream",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let mut request = url.clone().into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", session["token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    request
        .headers_mut()
        .insert("x-archava-client", "cli".parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "https://foreign.test".parse().unwrap());
    assert!(
        connect_async(request).await.is_err(),
        "foreign-origin upgrades must fail"
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", session["token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    request
        .headers_mut()
        .insert("x-archava-client", "cli".parse().unwrap());
    let (mut socket, _) = connect_async(request).await.unwrap();
    socket
        .send(Message::Text(
            json!({"csrf":session["csrf"]}).to_string().into(),
        ))
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await["type"], "ready");
    socket
        .send(Message::Binary(vec![0u8; 3200].into()))
        .await
        .unwrap();
    let response = next(&mut socket).await;
    assert_eq!(
        response["content"]["outputTranscription"]["text"],
        "Owned synthetic response"
    );
    assert_eq!(
        STANDARD
            .decode(
                response["content"]["modelTurn"]["parts"][0]["inlineData"]["data"]
                    .as_str()
                    .unwrap()
            )
            .unwrap()
            .len(),
        4800
    );
    socket
        .send(Message::Text(
            json!({"type":"video","data":STANDARD.encode([255,216,1,2,255,217])})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    timeout(Duration::from_secs(5), async {
        while frames.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE archava_commerce.memberships SET active=false WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await["type"], "closed");
    timeout(Duration::from_secs(5), mock)
        .await
        .unwrap()
        .unwrap();
    server.abort();
    let _ = server.await;
    assert_eq!(frames.load(Ordering::SeqCst), 2);
}
