//! Real loopback HTTP, synthetic credentials; no Midtrans service is contacted.
use archava_domain::erp::payments::PaymentState;
use archava_providers::{
    http::{TransportError, MAX_REPLY_BYTES, REQUEST_TIMEOUT},
    midtrans::{verify_signature, MidtransClient},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
const KEY: &str = "synthetic-midtrans-merchant-key";
fn signature(order: &str, status: &str, gross: &str) -> String {
    format!(
        "{:x}",
        Sha512::digest(format!("{order}{status}{gross}{KEY}").as_bytes())
    )
}
fn status(state: &str) -> Value {
    json!({"order_id":"archava-order","transaction_id":"transaction-1","status_code":"200","gross_amount":"30000.00","currency":"IDR","fraud_status":"accept","transaction_status":state,"signature_key":signature("archava-order","200","30000.00")})
}
struct Mock {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Mock {
    async fn new(code: u16, body: String, headers: Vec<(String, String)>, delay: Duration) -> Self {
        Self::dynamic(code, move |_| body, headers, delay).await
    }
    async fn dynamic(
        code: u16,
        body: impl FnOnce(&str) -> String,
        headers: Vec<(String, String)>,
        delay: Duration,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let body = body(&origin);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 2048];
            loop {
                let n = stream.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..n]);
                assert!(bytes.len() < 32768);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            recorded
                .lock()
                .unwrap()
                .push(String::from_utf8(bytes).unwrap());
            tokio::time::sleep(delay).await;
            let extra = headers
                .into_iter()
                .map(|(k, v)| format!("{k}: {v}\r\n"))
                .collect::<String>();
            let response=format!("HTTP/1.1 {code} Fixture\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{extra}\r\n{body}",body.len());
            let _ = stream.write_all(response.as_bytes()).await;
        });
        Self {
            origin,
            requests,
            task,
        }
    }
    async fn json(body: Value) -> Self {
        Self::new(200, body.to_string(), vec![], Duration::ZERO).await
    }
    fn client(&self) -> MidtransClient {
        MidtransClient::for_loopback(&self.origin, KEY.into()).unwrap()
    }
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn authenticated_status_prefers_transaction_reference_and_normalizes_only_the_server_answer()
{
    let mock = Mock::json(status("pending")).await;
    let result = mock
        .client()
        .status("archava-order", Some("transaction-1"), 30000)
        .await
        .unwrap();
    assert_eq!(result.observation().state, PaymentState::Pending);
    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /v2/transaction-1/status HTTP/1.1"));
    let auth = requests[0]
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.trim())
        })
        .unwrap();
    assert_eq!(
        auth,
        format!("Basic {}", STANDARD.encode(format!("{KEY}:")))
    );
    assert!(
        !requests[0].contains(KEY),
        "credential must not enter URL/body/plaintext headers"
    );
    assert!(!format!("{:?}", mock.client()).contains(KEY));
}
#[tokio::test]
async fn status_refuses_foreign_order_transaction_amount_currency_and_signature() {
    let mut cases = Vec::new();
    for (field, value) in [
        ("order_id", json!("foreign-order")),
        ("transaction_id", json!("foreign-transaction")),
        ("gross_amount", json!("30001.00")),
        ("currency", json!("USD")),
        ("signature_key", json!("0".repeat(128))),
    ] {
        let mut body = status("settlement");
        body[field] = value;
        cases.push(body);
    }
    for body in cases {
        let mock = Mock::json(body).await;
        assert_eq!(
            mock.client()
                .status("archava-order", Some("transaction-1"), 30000)
                .await
                .unwrap_err(),
            TransportError::Protocol
        );
    }
    let mut body = status("capture");
    body["fraud_status"] = json!("challenge");
    let mock = Mock::json(body).await;
    assert_eq!(
        mock.client()
            .status("archava-order", None, 30000)
            .await
            .unwrap()
            .observation()
            .state,
        PaymentState::Review
    );
}
#[tokio::test]
async fn cumulative_refunds_are_exact_and_duplicate_provider_refunds_refuse() {
    let mut body = status("partial_refund");
    body["refund_amount"] = json!("12000.00");
    body["refunds"] = json!([{"refund_chargeback_id":1,"refund_amount":"5000.00"},{"refund_chargeback_id":2,"refund_amount":"7000.00"}]);
    let mock = Mock::json(body.clone()).await;
    let answer = mock
        .client()
        .status("archava-order", None, 30000)
        .await
        .unwrap();
    assert_eq!(answer.observation().state, PaymentState::Refunded);
    assert_eq!(answer.observation().refunded_minor, 12000);
    body["refunds"][1]["refund_chargeback_id"] = json!(1);
    let mock = Mock::json(body).await;
    assert_eq!(
        mock.client()
            .status("archava-order", None, 30000)
            .await
            .unwrap_err(),
        TransportError::Protocol
    );
}
#[tokio::test]
async fn snap_creation_uses_exact_server_reference_and_rejects_foreign_redirect() {
    let valid=Mock::dynamic(200,|origin|json!({"token":"synthetic-token","redirect_url":format!("{}snap/v3/redirection/synthetic-token",origin)}).to_string(),vec![],Duration::ZERO).await;
    let checkout = valid
        .client()
        .create_checkout("archava-order", 30000)
        .await
        .unwrap();
    assert!(checkout.redirect_url.starts_with(&valid.origin));
    assert!(!format!("{checkout:?}").contains("synthetic-token"));
    let redirect = "https://attacker.example/snap/redirection/token";
    let mock = Mock::json(json!({"token":"synthetic-token","redirect_url":redirect})).await;
    assert_eq!(
        mock.client()
            .create_checkout("archava-order", 30000)
            .await
            .unwrap_err(),
        TransportError::Protocol
    );
    let requests = mock.requests();
    assert!(requests[0].starts_with("POST /snap/v1/transactions HTTP/1.1"));
    let body: Value = serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        body,
        json!({"transaction_details":{"order_id":"archava-order","gross_amount":30000}})
    );
}
#[tokio::test]
async fn redirects_oversized_malformed_and_slow_answers_cannot_be_financial_success() {
    for (code, body, headers, expected) in [
        (
            302,
            String::new(),
            vec![("Location".into(), "https://attacker.example/".into())],
            TransportError::Status(302),
        ),
        (
            200,
            "x".repeat(MAX_REPLY_BYTES + 1),
            vec![],
            TransportError::TooLarge,
        ),
        (200, "{broken".into(), vec![], TransportError::Protocol),
        (
            429,
            "private provider diagnostic".into(),
            vec![],
            TransportError::Status(429),
        ),
    ] {
        let mock = Mock::new(code, body, headers, Duration::ZERO).await;
        assert_eq!(
            mock.client()
                .status("archava-order", None, 30000)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(mock.requests().len(), 1, "no redirect or automatic retry");
    }
    let mock = Mock::new(
        200,
        status("settlement").to_string(),
        vec![],
        REQUEST_TIMEOUT + Duration::from_secs(1),
    )
    .await;
    assert_eq!(
        mock.client()
            .status("archava-order", None, 30000)
            .await
            .unwrap_err(),
        TransportError::Timeout
    );
}
#[test]
fn malformed_signatures_and_nonloopback_test_endpoints_refuse() {
    let sig = signature("order", "200", "30000.00");
    assert!(verify_signature(
        "order",
        "200",
        "30000.00",
        KEY,
        &sig.to_ascii_uppercase()
    ));
    assert!(!verify_signature("other", "200", "30000.00", KEY, &sig));
    for invalid in ["x".repeat(128), "0".repeat(127), "0".repeat(129)] {
        assert!(!verify_signature("order", "200", "30000.00", KEY, &invalid));
    }
    for endpoint in [
        "http://192.0.2.1/",
        "https://127.0.0.1/",
        "http://localhost.example/",
        "http://user@127.0.0.1/",
        "http://127.0.0.1/path",
        "http://127.0.0.1/?key=value",
    ] {
        assert!(
            MidtransClient::for_loopback(endpoint, KEY.into()).is_err(),
            "{endpoint}"
        );
    }
}
