//! Postmark, behind the email gateway — the port of
//! `packages/act-postmark/src/gateway.ts` (+ `transport.ts`, `label.ts`,
//! and `status.ts`'s reconciliation port), the HTTP seam the outbox's
//! at-most-once rule leans on.
//!
//! The template, the sender and the recipient are not this file's to choose:
//! they arrive inside the `EmailSendRequest`, and the adapter sends exactly
//! those and nothing else. What this port pins is the answer to one
//! question, honestly — Postmark says three distinguishable things about a
//! send, and only one of them is an acceptance:
//!
//! 1. **`accepted`, only with the id Postmark issued.** A 200 whose payload
//!    carries no usable `MessageID` is not an acceptance (`accepted, only
//!    with the id Postmark issued`).
//! 2. **`rejected` is Postmark answering no** — keyed by the body's
//!    `ErrorCode` (the discriminator; a 200 does not mean accepted and a
//!    batch returns 200-level even when messages fail), with the corner
//!    cases that arrive unsigned falling back to the documented HTTP-status
//!    refusals.
//! 3. **Everything else is `unknown`.** 404/500/503, non-JSON bodies,
//!    uninterpretable codes, timeouts and unreachable transports are silence,
//!    and silence stays silence.
//!
//! The send is never retried: one call per attempt, whatever the answer —
//! the boundary owns at-most-once, and a retry here would be a second
//! message into a real mailbox.
//!
//! ## The idempotency label
//!
//! Postmark documents no idempotency-key field, so there is nowhere honest
//! to put one. The upstream raw key still never reaches the wire: it is
//! HMAC-SHA256'd (keyed by an injected ≥32-byte secret, bound per tenant) and
//! travels as a metadata label under `archava_attempt` — searchable in
//! outbound search, but not `'archava-postmark-attempt-v2'` — and the
//! reconciliation port looks it up the same way. The raw key appears nowhere.
//!
//! ## The sync seam
//!
//! The gateway races the transport against its own 15_000 ms bound, ported
//! like [`crate::gemini`] — deadline in, elapsed re-check out. A transport
//! that answers late is dropped (`unknown mail_provider_did_not_answer`);
//! one that throws is `unknown mail_provider_unreachable`; both enter the
//! transport exactly once.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// The one endpoint: `POST /email/withTemplate` (gateway.ts line 75). The
/// one sent is the one WITHOUT a trailing slash.
pub const POSTMARK_SEND_URL: &str = "https://api.postmarkapp.com/email/withTemplate";

/// The default message stream. Postmark defaults to `outbound` when omitted.
const DEFAULT_MESSAGE_STREAM: &str = "outbound";

/// The metadata key the opaque attempt label travels under. Not a secret.
pub const POSTMARK_ATTEMPT_KEY: &str = "archava_attempt";

/// The adapter's own bound (`gateway.ts` line 90).
pub const POSTMARK_SEND_TIMEOUT_MS: u64 = 15_000;

/// Which gateway this is, for the audit trail. Not a secret.
pub const GATEWAY_ID: &str = "postmark:withTemplate@1";

/// The status/reconciliation port's own identity (`status.ts`).
pub const POSTMARK_STATUS_PORT_ID: &str = "postmark:status@1";

/// The one lookup endpoint (`status.ts`): outbound message search.
pub const POSTMARK_STATUS_URL: &str = "https://api.postmarkapp.com/messages/outbound";

/// Machine reasons this adapter produced on its own — never provider text.
pub mod reason {
    pub const TIMEOUT: &str = "mail_provider_did_not_answer";
    pub const UNREACHABLE: &str = "mail_provider_unreachable";
    pub const NOT_JSON: &str = "mail_provider_reply_unreadable";
    pub const NO_MESSAGE_ID: &str = "mail_provider_receipt_absent";
    pub const UNRECOGNISED: &str = "mail_provider_reply_unrecognised";
    pub const UNUSABLE_REQUEST: &str = "mail_request_not_sendable";
    pub const STATUS_NOT_AVAILABLE: &str = "status_not_available";
}

/// The tenant-scoped key a boundary returns when a key arrives carrying a
/// different recipient or template (`ports.ts` `EMAIL_IDEMPOTENCY_CONFLICT`).
/// A constant because the executor must recognise it and say something
/// different about it than about a provider's own refusal.
pub const EMAIL_IDEMPOTENCY_CONFLICT: &str = "email_idempotency_key_conflict";

// The refusal maps: keyed by the body's ErrorCode (`gateway.ts` lines
// 126–142) and by the HTTP status (`gateway.ts` lines 152–158). Every code,
// its meaning and its wording come from Postmark's own API error codes
// list; a code that is not listed is not interpreted — "the provider
// declined" is a claim, and this adapter only makes claims it has checked.
//
// The two maps are kept separate because 413 means different things on each
// side: as an HTTP status it is payload size (`mail_provider_rejected_
// request` — a documented non-refusal line), as an ErrorCode it is an
// account not approved to send. A documented non-refusal that arrives as a
// code (100 "offline for maintenance", 101) therefore falls through to
// `unknown`, which is where an outage belongs.
fn refusal_by_code(code: u32) -> Option<&'static str> {
    match code {
        10 => Some("mail_provider_rejected_credential"),
        300 | 402 | 403 | 410 | 411 => Some("mail_provider_rejected_request"),
        406 => Some("mail_provider_rejected_recipient"),
        412 | 413 => Some("mail_account_not_approved"),
        422 | 1105 => Some("mail_provider_rejected_configuration"),
        1101 => Some("mail_provider_template_unknown"),
        1235 => Some("mail_provider_stream_unknown"),
        1236 => Some("mail_provider_stream_unsupported"),
        1480 => Some("mail_provider_rejected_configuration"),
        _ => None,
    }
}

/// Refusals keyed by HTTP status for a reply whose body carries no usable
/// `ErrorCode` (`gateway.ts` lines 152–158). 429 is here because a
/// rate-limited message is one Postmark explicitly did not accept.
fn refusal_by_status(status: u16) -> Option<&'static str> {
    match status {
        401 => Some("mail_provider_rejected_credential"),
        413 | 415 | 422 => Some("mail_provider_rejected_request"),
        429 => Some("mail_provider_rate_limited"),
        _ => None,
    }
}

/// Statuses this adapter will not interpret (`gateway.ts` line 168): 404 is
/// "Entity doesn't exist", 500 is Postmark's own server failure, 503 is
/// "planned service outages" — none says anything about THIS message.
const SILENT_STATUSES: [u16; 3] = [404, 500, 503];

// ---------------------------------------------------------------------------
// The request vocabulary this seam receives (ports of `act-email`'s types).
// ---------------------------------------------------------------------------

/// A tenant-approved template: the only sender source there is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedEmailTemplate {
    pub approved: bool,
    pub tenant_id: String,
    pub template_id: String,
    pub provider_template_id: String,
    pub sender_address: String,
}

/// One send request, as the outbox boundary hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailSendRequest {
    pub tenant_id: String,
    pub recipient: String,
    pub template: ApprovedEmailTemplate,
    pub idempotency_key: String,
}

/// The only acceptance this package will ever report: the provider took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailGatewayOutcome {
    /// Opaque to this package: never parsed, never reconstructed, only
    /// compared. The id Postmark issued is the receipt.
    Accepted { provider_message_id: String },
    /// Flattened to a machine reason before anyone outside sees it.
    Rejected { reason: &'static str },
    /// Neither acceptance nor refusal: silence, named.
    Unknown { reason: &'static str },
}

/// Whether a value is a non-blank string (`ports.ts` `isNonblank`): blank is
/// not a value — accepting a blank tenant id is how a request ends up scoped
/// to nothing.
pub fn is_nonblank(value: &str) -> bool {
    !value.trim().is_empty()
}

/// Whether a string is a segment of an address this package is willing to
/// put on the wire: no whitespace and none of the reserved mail routing
/// characters (`ports.ts`'s `EMAIL_SHAPE` class
/// `[^\s@,;:<>()[\]\\"]`).
fn email_shape_char(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            '@' | ',' | ';' | ':' | '<' | '>' | '(' | ')' | '[' | ']' | '\\' | '"'
        )
}

/// Whether a value is an address (shape and 254 length only; authority over
/// it stays with the resolver). TS: `/seg@seg\.seg/` with three non-empty
/// segments.
pub fn is_email_address(value: &str) -> bool {
    if value.chars().count() > 254 {
        return false;
    }
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or("");
    let tail = match (parts.next(), parts.next()) {
        (Some(tail), None) => tail,
        _ => return false,
    };
    if !email_all_ok(local) {
        return false;
    }
    // The tail must contain a '.' with non-empty, well-shaped text on both
    // sides (the regex's greedy segment sizing).
    let Some((head, domain)) = tail.rsplit_once('.') else {
        return false;
    };
    !head.is_empty() && !domain.is_empty() && email_all_ok(head) && email_all_ok(domain)
}

fn email_all_ok(segment: &str) -> bool {
    !segment.is_empty() && segment.chars().all(email_shape_char)
}

/// Whether a value is a well-formed opaque provider id ([A-Za-z0-9._:-],
/// 1..=128) — never an address echo, never a receipt for something else.
pub fn is_provider_message_id(value: &str) -> bool {
    let length = value.chars().count();
    (1..=128).contains(&length)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

/// The structural half of template trust (`ports.ts`
/// `isTrustedTemplateShape`): approved by somebody typed in server
/// configuration, tenant/template ids non-blank, and a sender that is an
/// address — a forged sender would be a message from an address the tenant
/// never approved.
pub fn is_trusted_template_shape(template: &ApprovedEmailTemplate) -> bool {
    template.approved
        && is_nonblank(&template.tenant_id)
        && is_nonblank(&template.template_id)
        && is_nonblank(&template.provider_template_id)
        && is_email_address(&template.sender_address)
}

// ---------------------------------------------------------------------------
// The transport seam (a port of `transport.ts`).
// ---------------------------------------------------------------------------

/// One request, as the gateway hands it to a transport. Header names are
/// lower-case so a mock comparing them sees one spelling.
#[derive(Clone, PartialEq, Eq)]
pub struct PostmarkHTTPRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl core::fmt::Debug for PostmarkHTTPRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let headers = self
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name,
                    if name.eq_ignore_ascii_case("x-postmark-server-token")
                        || name.eq_ignore_ascii_case("authorization")
                    {
                        "<redacted>"
                    } else {
                        value.as_str()
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        f.debug_struct("PostmarkHTTPRequest")
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body", &self.body)
            .finish()
    }
}

/// One reply, read already: a string body, not a stream. Reading inside the
/// seam leaves the gateway holding a plain value and the transport free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostmarkHTTPResponse {
    pub status: u16,
    pub body: String,
}

/// A transport that did not complete — carrying nothing, with the one fixed
/// message (`transport.ts`'s `PostmarkTransportFailure`). A native network
/// error's message can carry the request (the URL, the proxy, the headers);
/// this package does not relay it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostmarkTransportFailure;

impl core::fmt::Display for PostmarkTransportFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the request to the mail provider did not complete")
    }
}

impl std::error::Error for PostmarkTransportFailure {}

/// The outbound boundary. The only `fetch`-equivalent this crate owns; a
/// real client is composed once, at the host, against this trait — which is
/// also what lets every witness run against a fake with no network.
///
/// `deadline` is the gateway's bound; a transport that cannot honor it must
/// still return so the gateway's elapsed re-check can hold the race down.
pub trait PostmarkTransport: Send + Sync {
    fn post(
        &self,
        request: &PostmarkHTTPRequest,
        deadline: Instant,
    ) -> Result<PostmarkHTTPResponse, PostmarkTransportFailure>;
}

/// The read boundary, for the reconciliation port only (`transport.ts`'s
/// read transport): a token never follows redirects anywhere else.
pub trait PostmarkReadTransport: Send + Sync {
    fn get(
        &self,
        request: &PostmarkHTTPRequest,
        deadline: Instant,
    ) -> Result<PostmarkHTTPResponse, PostmarkTransportFailure>;
}

// ---------------------------------------------------------------------------
// The attempt label (a port of `label.ts`).
// ---------------------------------------------------------------------------

/// HMAC-SHA256 (RFC 2104) over sha2 — the same primitive node's
/// `createHmac('sha256', ...)` runs, so the wire label stays byte-for-byte
/// the v2 one. Block size 64; a key longer than a block is hashed first.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let inner = {
        let mut inner = Sha256::new();
        for byte in key_block {
            inner.update([byte ^ 0x36]);
        }
        inner.update(message);
        inner.finalize()
    };
    let mut outer = Sha256::new();
    for byte in key_block {
        outer.update([byte ^ 0x5c]);
    }
    outer.update(inner);
    outer.finalize().into()
}

/// Base64url without padding, base64's URL-safe alphabet — node's
/// `digest('base64url')`.
fn base64url(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        br"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[triple as usize & 0x3f] as char);
        }
    }
    out
}

/// The opaque attempt label — keyed, tenant-separated, and deliberately NOT
/// an idempotency promise (`label.ts` byte-for-byte): the wire list is
/// `["archava-postmark-attempt-v2", tenantId, idempotencyKey]`, compact JSON.
pub fn postmark_attempt_label(key: &[u8], tenant_id: &str, idempotency_key: &str) -> String {
    let message = format!(
        "[{},{},{}]",
        serde_json::to_string("archava-postmark-attempt-v2").expect("a fixed literal is a string"),
        serde_json::to_string(tenant_id).expect("tenant ids are strings"),
        serde_json::to_string(idempotency_key).expect("keys are strings"),
    );
    base64url(&hmac_sha256(key, message.as_bytes()))
}

/// TS: the metadata comparison is `object(message.Metadata) &&
/// message.Metadata[POSTMARK_ATTEMPT_KEY] !== label` — a missing or
/// non-object Metadata yields an undefined entry, which matches nothing.
fn label_matches(expected: &str, received: Option<&Value>) -> bool {
    received.and_then(Value::as_str) == Some(expected)
}

// ---------------------------------------------------------------------------
// The gateway (a port of `gateway.ts`'s class half).
// ---------------------------------------------------------------------------

/// A construction refusal (`gateway.ts`'s three constructor `TypeError`s):
/// a deployment that forgot its credential is refused BEFORE any send can
/// be attempted, because a missing token would otherwise surface as a 401 —
/// a *refusal* — when the truth is that nothing was ever sendable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostmarkConfigError {
    ServerTokenRequired,
    /// A label key under 32 bytes is not a secret.
    LabelKeyTooShort,
    /// A timeout that could not bound anything.
    TimeoutInvalid,
}

/// Construction options (TS `PostmarkEmailGatewayOptions`).
pub struct PostmarkEmailGatewayOptions {
    pub server_token: String,
    pub transport: std::sync::Arc<dyn PostmarkTransport>,
    /// A server-only secret of at least 32 bytes, stable for as long as any
    /// attempt can replay. Copied at construction.
    pub attempt_label_key: Vec<u8>,
    pub message_stream: Option<String>,
    pub timeout_ms: Option<u64>,
}

/// The gateway: one send per call, one call per attempt. Stateless except
/// for the credential and the label key.
#[derive(Clone)]
pub struct PostmarkEmailGateway {
    transport: std::sync::Arc<dyn PostmarkTransport>,
    server_token: String,
    attempt_label_key: Vec<u8>,
    message_stream: String,
    timeout_ms: u64,
}

impl PostmarkEmailGateway {
    pub fn new(options: PostmarkEmailGatewayOptions) -> Result<Self, PostmarkConfigError> {
        if !is_nonblank(&options.server_token) {
            return Err(PostmarkConfigError::ServerTokenRequired);
        }
        if options.attempt_label_key.len() < 32 {
            return Err(PostmarkConfigError::LabelKeyTooShort);
        }
        if let Some(timeout_ms) = options.timeout_ms {
            if timeout_ms == 0 {
                return Err(PostmarkConfigError::TimeoutInvalid);
            }
        }
        Ok(Self {
            transport: options.transport,
            server_token: options.server_token,
            attempt_label_key: options.attempt_label_key,
            message_stream: options
                .message_stream
                .unwrap_or_else(|| DEFAULT_MESSAGE_STREAM.to_string()),
            timeout_ms: options.timeout_ms.unwrap_or(POSTMARK_SEND_TIMEOUT_MS),
        })
    }

    /// TS: `send(request)` (lines 235–250) — the defensive half of the
    /// executor's checks first; then the ONE call; then the interpretation.
    pub fn send(&self, request: &EmailSendRequest) -> EmailGatewayOutcome {
        if !sendable(request) {
            return EmailGatewayOutcome::Rejected {
                reason: reason::UNUSABLE_REQUEST,
            };
        }
        let payload =
            build_send_payload(request, &self.message_stream, &self.attempt_label(request));
        let started = Instant::now();
        let bound = Duration::from_millis(self.timeout_ms);
        match self
            .transport
            .post(&self.build_http_request(&payload), started + bound)
        {
            Ok(reply) => {
                if over_deadline(started, bound) {
                    return unknown(reason::TIMEOUT);
                }
                let parsed = parse_json_body(&reply.body);
                interpret(reply.status, parsed.as_ref())
            }
            Err(_) => unknown(timeout_map(started, bound)),
        }
    }

    fn build_http_request(&self, payload: &str) -> PostmarkHTTPRequest {
        let mut headers = BTreeMap::new();
        headers.insert("accept".to_string(), "application/json".to_string());
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert(
            "x-postmark-server-token".to_string(),
            self.server_token.clone(),
        );
        PostmarkHTTPRequest {
            url: POSTMARK_SEND_URL.to_string(),
            headers,
            body: payload.to_string(),
        }
    }

    /// The opaque label derived from the upstream raw idempotency key.
    fn attempt_label(&self, request: &EmailSendRequest) -> String {
        postmark_attempt_label(
            &self.attempt_label_key,
            &request.tenant_id,
            &request.idempotency_key,
        )
    }
}

/// TS: `unknown(reason)` — silence, named.
fn unknown(reason: &'static str) -> EmailGatewayOutcome {
    EmailGatewayOutcome::Unknown { reason }
}

/// The deadline re-check, same shape as [`crate::gemini`].
fn over_deadline(started: Instant, bound: Duration) -> bool {
    started.elapsed() >= bound
}

fn timeout_map(started: Instant, bound: Duration) -> &'static str {
    if over_deadline(started, bound) {
        reason::TIMEOUT
    } else {
        reason::UNREACHABLE
    }
}

/// The defensive half of the executor's checks (`gateway.ts`
/// `sendable`, lines 335–349): non-blank tenant and key, an address this
/// package would carry, a trusted template that belongs to the requesting
/// tenant, and a numeric template id that `Number` would not round into
/// another id.
fn sendable(request: &EmailSendRequest) -> bool {
    let template_id = &request.template.provider_template_id;
    is_nonblank(&request.tenant_id)
        && is_nonblank(&request.idempotency_key)
        && is_email_address(&request.recipient)
        && is_trusted_template_shape(&request.template)
        && request.template.tenant_id == request.tenant_id
        && (!digit_only(template_id) || safe_positive_integer(template_id))
}

fn digit_only(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

/// `Number.isSafeInteger(Number(x)) && Number(x) > 0` for a digit-only
/// string: representable inside 2^53-1 and positive. ("000" → 0 → refused.)
fn safe_positive_integer(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|number| number <= 9_007_199_254_740_991 && number > 0)
}

/// The request body the endpoint takes (`gateway.ts`
/// `buildSendPayload`, lines 366–387): only what a template send requires,
/// and nothing else — no `HtmlBody`, no `TextBody`, no `Subject`. One
/// address, never a joined list. `TemplateModel` is required and empty.
fn build_send_payload(
    request: &EmailSendRequest,
    message_stream: &str,
    attempt_label: &str,
) -> String {
    let template = &request.template;
    let mut body = Map::new();
    body.insert("To".into(), Value::String(request.recipient.clone()));
    body.insert(
        "From".into(),
        Value::String(template.sender_address.clone()),
    );
    body.insert("TemplateModel".into(), json!({}));
    body.insert(
        "MessageStream".into(),
        Value::String(message_stream.to_string()),
    );
    body.insert(
        "Metadata".into(),
        json!({ POSTMARK_ATTEMPT_KEY: attempt_label }),
    );
    // The documented alternatives travel alone: one field per request.
    if digit_only(&template.provider_template_id) {
        body.insert(
            "TemplateId".into(),
            Value::Number(
                template
                    .provider_template_id
                    .parse::<u64>()
                    .expect("digit-only ids parse")
                    .into(),
            ),
        );
    } else {
        body.insert(
            "TemplateAlias".into(),
            Value::String(template.provider_template_id.clone()),
        );
    }
    Value::Object(body).to_string()
}

/// Read a reply that crossed a boundary. Anything this package cannot name
/// is not an answer (`gateway.ts` `interpret`, lines 400–424).
fn interpret(status: u16, body: Option<&Value>) -> EmailGatewayOutcome {
    // A success-shaped body attached to a failed HTTP response is not a
    // receipt. In particular Postmark documents 500/503 as server/outage
    // failures.
    if SILENT_STATUSES.contains(&status) {
        return unknown(reason::UNRECOGNISED);
    }
    let send = body.and_then(send_response);
    if let Some(send) = send.as_ref() {
        if send.error_code != 0 {
            return match refusal_by_code(send.error_code) {
                Some(refusal) => EmailGatewayOutcome::Rejected { reason: refusal },
                None => unknown(reason::UNRECOGNISED),
            };
        }
    }
    if let Some(refusal) = refusal_by_status(status) {
        return EmailGatewayOutcome::Rejected { reason: refusal };
    }
    // A status this adapter does not interpret, or a body that is not JSON:
    // neither is a refusal, and neither is a receipt.
    let send = match send {
        Some(send) => send,
        None => {
            return unknown(if status != 200 {
                reason::UNRECOGNISED
            } else {
                reason::NOT_JSON
            })
        }
    };
    // A success, and only with the id Postmark issued.
    if status != 200 {
        return unknown(reason::UNRECOGNISED);
    }
    let message_id = send.message_id.unwrap_or("");
    if !is_provider_message_id(message_id) {
        return unknown(reason::NO_MESSAGE_ID);
    }
    EmailGatewayOutcome::Accepted {
        provider_message_id: message_id.to_string(),
    }
}

/// A reply body, parsed, or `None` when it is not JSON. Nothing about the
/// raw text travels onward: a provider error page is not a reason string,
/// and this package does not quote one.
fn parse_json_body(body: &str) -> Option<Value> {
    serde_json::from_str(body).ok()
}

/// A body carrying the fields the send endpoint returns, or `None`:
/// `ErrorCode` must be an integer (TS: `Number.isInteger`); anything else is
/// not a send response this adapter can name.
struct PostmarkSendResponse<'a> {
    error_code: u32,
    message_id: Option<&'a str>,
}

fn send_response(body: &Value) -> Option<PostmarkSendResponse<'_>> {
    let map = body.as_object()?;
    let error_code = map.get("ErrorCode")?;
    // An integer, not a float that happens to be whole.
    let error_code = match error_code {
        Value::Number(number) if number.is_u64() => number.as_u64()?,
        _ => return None,
    };
    if error_code > u32::MAX as u64 {
        return None;
    }
    let message_id = match map.get("MessageID") {
        Some(Value::String(id)) => Some(id.as_str()),
        _ => None,
    };
    Some(PostmarkSendResponse {
        error_code: error_code as u32,
        message_id,
    })
}

// ---------------------------------------------------------------------------
// The reconciliation port (a port of `status.ts`).
// ---------------------------------------------------------------------------

/// Tenant-bound safety for a string that may ride a URL or header:
/// 1..=1000 chars, no control carriage.
fn safe_text(value: &str) -> bool {
    let length = value.chars().count();
    (1..=1000).contains(&length) && !value.contains(['\r', '\n', '\0'])
}

/// `^[a-zA-Z0-9_-]{1,100}$`.
fn is_message_stream(value: &str) -> bool {
    let length = value.chars().count();
    (1..=100).contains(&length)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `^[a-f0-9]{64}$` — the fingerprint shape.
fn is_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| (b as char).is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// `^[a-fA-F0-9]{8}-...` — the UUID shape the status port reads back.
fn is_uuid_shape(value: &str) -> bool {
    const GROUP_LENS: [usize; 5] = [8, 4, 4, 4, 12];
    if !value.is_ascii() {
        return false;
    }
    let groups: Vec<&[u8]> = value.as_bytes().split(|&b| b == b'-').collect();
    groups.len() == 5
        && groups.iter().zip(GROUP_LENS).all(|(group, expected)| {
            group.len() == expected && group.iter().all(u8::is_ascii_hexdigit)
        })
}

/// What a lookup carries (`act-email`'s `EmailDeliveryStatusLookup`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailDeliveryStatusLookup {
    pub tenant_id: String,
    pub idempotency_key: String,
    pub fingerprint: String,
}

/// The tenant-bound read-only recovery configuration (`status.ts`
/// `PostmarkStatusRecovery`), with its read seam injected — the port can't
/// construct a default HTTP client the way the TS default did.
#[derive(Clone)]
pub struct PostmarkStatusRecovery {
    pub tenant_id: String,
    pub server_token: String,
    pub attempt_label_key: Vec<u8>,
    pub message_stream: Option<String>,
    pub sandboxed: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub transport: std::sync::Arc<dyn PostmarkReadTransport>,
}

/// A recovery configuration that failed its shape check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusConfigError;

/// The status port: tenant-bound positive-acceptance lookup only — never
/// sends, never reports a rejection or delivery, and answers
/// `unknown('status_not_available')` for every question it cannot answer.
pub struct PostmarkStatusPort {
    status_port_id: String,
    recovery: Option<Recovery>,
}

struct Recovery {
    tenant_id: String,
    server_token: String,
    attempt_label_key: Vec<u8>,
    message_stream: String,
    sandboxed: bool,
    timeout_ms: u64,
    transport: std::sync::Arc<dyn PostmarkReadTransport>,
}

/// Port construction options (`status.ts` `PostmarkStatusPortOptions`).
pub struct PostmarkStatusPortOptions {
    pub deployment: Option<String>,
    pub recovery: Option<PostmarkStatusRecovery>,
}

impl PostmarkStatusPort {
    pub fn new(options: PostmarkStatusPortOptions) -> Result<Self, StatusConfigError> {
        let status_port_id = match options.deployment {
            Some(deployment) => format!("{POSTMARK_STATUS_PORT_ID}({deployment})"),
            None => POSTMARK_STATUS_PORT_ID.to_string(),
        };
        let recovery = match options.recovery {
            Some(recovery) => {
                if !safe_text(&recovery.tenant_id)
                    || !safe_text(&recovery.server_token)
                    || recovery.attempt_label_key.len() < 32
                    || recovery
                        .message_stream
                        .as_deref()
                        .is_some_and(|stream| !is_message_stream(stream))
                    || recovery
                        .timeout_ms
                        .is_some_and(|timeout| !(1..=30_000).contains(&timeout))
                {
                    return Err(StatusConfigError);
                }
                Some(Recovery {
                    tenant_id: recovery.tenant_id,
                    server_token: recovery.server_token,
                    attempt_label_key: recovery.attempt_label_key,
                    message_stream: recovery
                        .message_stream
                        .unwrap_or_else(|| DEFAULT_MESSAGE_STREAM.to_string()),
                    sandboxed: recovery.sandboxed.unwrap_or(false),
                    timeout_ms: recovery.timeout_ms.unwrap_or(15_000),
                    transport: recovery.transport,
                })
            }
            None => None,
        };
        Ok(Self {
            status_port_id,
            recovery,
        })
    }

    pub fn status_port_id(&self) -> &str {
        &self.status_port_id
    }

    /// TS: `status(lookup)` — every gate is `unknown` before the call, and
    /// every mismatch in the reply is `unknown` after it. The one success
    /// path in the whole port is a SINGLE matching message whose metadata
    /// label matches the freshly-derived one, on the configured stream,
    /// with a state Postmark documents as "accepted".
    pub fn status(&self, lookup: &EmailDeliveryStatusLookup) -> EmailGatewayOutcome {
        let unknown = || unknown(reason::STATUS_NOT_AVAILABLE);
        let Some(recovery) = &self.recovery else {
            return unknown();
        };
        if lookup.tenant_id != recovery.tenant_id
            || lookup.idempotency_key.chars().count() < 1
            || lookup.idempotency_key.chars().count() > 65_536
            || !is_fingerprint(&lookup.fingerprint)
        {
            return unknown();
        }
        let label = postmark_attempt_label(
            &recovery.attempt_label_key,
            &lookup.tenant_id,
            &lookup.idempotency_key,
        );
        let url = format!(
            "{POSTMARK_STATUS_URL}?count=2&offset=0&messagestream={stream}&metadata_{POSTMARK_ATTEMPT_KEY}={label}",
            stream = recovery.message_stream,
        );
        let started = Instant::now();
        let bound = Duration::from_millis(recovery.timeout_ms);
        let response = match recovery
            .transport
            .get(&read_request(&url, &recovery.server_token), started + bound)
        {
            Ok(response) => response,
            Err(_) => return unknown(),
        };
        if response.status != 200 || response.body.len() > 65_536 {
            return unknown();
        }
        let reply: Value = match serde_json::from_str(&response.body) {
            Ok(reply) => reply,
            Err(_) => return unknown(),
        };
        let map = match reply.as_object() {
            Some(map) => map,
            None => return unknown(),
        };
        if map.get("TotalCount") != Some(&Value::Number(1.into())) {
            return unknown();
        }
        let messages = match map.get("Messages") {
            Some(Value::Array(messages)) if messages.len() == 1 => messages,
            _ => return unknown(),
        };
        let message = match messages.first().and_then(Value::as_object) {
            Some(message) => message,
            None => return unknown(),
        };
        let message_id = match message.get("MessageID").and_then(Value::as_str) {
            Some(message_id) if is_uuid_shape(message_id) => message_id,
            _ => return unknown(),
        };
        if !label_matches(
            &label,
            message
                .get("Metadata")
                .and_then(|m| m.get(POSTMARK_ATTEMPT_KEY)),
        ) || message.get("MessageStream").and_then(Value::as_str)
            != Some(recovery.message_stream.as_str())
            || message.get("Sandboxed").and_then(Value::as_bool) != Some(recovery.sandboxed)
            || !matches!(
                message.get("Status").and_then(Value::as_str),
                Some("Sent") | Some("Processed") | Some("Queued")
            )
        {
            return unknown();
        }
        EmailGatewayOutcome::Accepted {
            provider_message_id: message_id.to_string(),
        }
    }
}

fn read_request(url: &str, server_token: &str) -> PostmarkHTTPRequest {
    let mut headers = BTreeMap::new();
    headers.insert("accept".to_string(), "application/json".to_string());
    headers.insert(
        "x-postmark-server-token".to_string(),
        server_token.to_string(),
    );
    PostmarkHTTPRequest {
        url: url.to_string(),
        headers,
        body: String::new(),
    }
}
