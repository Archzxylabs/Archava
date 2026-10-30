//! Real loopback HTTP, synthetic credentials; no Resend service is contacted.
use archava_providers::{
    http::{TransportError, MAX_REPLY_BYTES, REQUEST_TIMEOUT},
    resend::ResendClient,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

const KEY: &str = "re_synthetic_test_api_key_12345";

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
    async fn new(code: u16, body: String, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
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
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&bytes).to_string();
            recorded.lock().unwrap().push(text);
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let response = format!(
                "HTTP/1.1 {code} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
        Self {
            origin,
            requests,
            task,
        }
    }
}

#[tokio::test]
async fn send_email_succeeds_with_idempotency_and_bearer_auth() {
    let mock = Mock::new(200, r#"{"id":"email_987654321"}"#.into(), Duration::ZERO).await;
    let client = ResendClient::for_loopback(&mock.origin, KEY.into()).unwrap();
    let result = client
        .send_email(
            "idem-key-12345",
            "Acme <billing@acme.com>",
            "customer@example.com",
            "Invoice PAY-001",
            "Please pay your invoice.",
            Some("<p>Please pay your invoice.</p>"),
        )
        .await
        .unwrap();

    assert_eq!(result.id, "email_987654321");

    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert!(req.contains("POST /emails HTTP/1.1"));
    assert!(req.contains(&format!("authorization: Bearer {KEY}")));
    assert!(req.contains("idempotency-key: idem-key-12345"));
    assert!(req.contains("customer@example.com"));
    assert!(req.contains("Invoice PAY-001"));
}

#[tokio::test]
async fn malformed_inputs_refuse_before_network() {
    let client = ResendClient::new(KEY.into()).unwrap();

    // Invalid idempotency key (spaces, empty, too long)
    assert_eq!(
        client
            .send_email("", "from@a.com", "to@b.com", "Sub", "Text", None)
            .await
            .unwrap_err(),
        TransportError::Protocol
    );
    assert_eq!(
        client
            .send_email(
                "bad key with spaces",
                "from@a.com",
                "to@b.com",
                "Sub",
                "Text",
                None
            )
            .await
            .unwrap_err(),
        TransportError::Protocol
    );

    // Empty recipient or subject
    assert_eq!(
        client
            .send_email("idem-1", "from@a.com", "", "Sub", "Text", None)
            .await
            .unwrap_err(),
        TransportError::Protocol
    );
    assert_eq!(
        client
            .send_email("idem-1", "from@a.com", "to@b.com", "  ", "Text", None)
            .await
            .unwrap_err(),
        TransportError::Protocol
    );
}

#[tokio::test]
async fn status_errors_bubble_as_status_codes() {
    // 409 Conflict
    let mock409 = Mock::new(409, r#"{"error":"conflict"}"#.into(), Duration::ZERO).await;
    let client409 = ResendClient::for_loopback(&mock409.origin, KEY.into()).unwrap();
    assert_eq!(
        client409
            .send_email("idem-1", "a@a.com", "b@b.com", "Sub", "Body", None)
            .await
            .unwrap_err(),
        TransportError::Status(409)
    );

    // 422 Unprocessable
    let mock422 = Mock::new(422, r#"{"error":"validation"}"#.into(), Duration::ZERO).await;
    let client422 = ResendClient::for_loopback(&mock422.origin, KEY.into()).unwrap();
    assert_eq!(
        client422
            .send_email("idem-1", "a@a.com", "b@b.com", "Sub", "Body", None)
            .await
            .unwrap_err(),
        TransportError::Status(422)
    );
}

#[tokio::test]
async fn timeouts_and_oversized_responses_refuse() {
    // Timeout
    let mock_slow = Mock::new(
        200,
        r#"{"id":"late"}"#.into(),
        REQUEST_TIMEOUT + Duration::from_millis(500),
    )
    .await;
    let client_slow = ResendClient::for_loopback(&mock_slow.origin, KEY.into()).unwrap();
    assert_eq!(
        client_slow
            .send_email("idem-1", "a@a.com", "b@b.com", "Sub", "Body", None)
            .await
            .unwrap_err(),
        TransportError::Timeout
    );

    // Oversized response
    let big_body = format!(
        r#"{{"id":"{}","padding":"{}"}}"#,
        "big",
        "a".repeat(MAX_REPLY_BYTES + 10)
    );
    let mock_big = Mock::new(200, big_body, Duration::ZERO).await;
    let client_big = ResendClient::for_loopback(&mock_big.origin, KEY.into()).unwrap();
    assert_eq!(
        client_big
            .send_email("idem-1", "a@a.com", "b@b.com", "Sub", "Body", None)
            .await
            .unwrap_err(),
        TransportError::TooLarge
    );
}
