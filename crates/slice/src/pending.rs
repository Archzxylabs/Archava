//! The app-owned pending marker.
//!
//! ## Why this exists
//!
//! The 19 Oktober 2026 audit reproduced a lost identity: a follow-up was
//! submitted, the transport died AFTER the server committed it, the page
//! showed the unknown outcome — and then a RELOAD replaced the request id
//! with a fresh one. The original key was nowhere: not in the tab, not in
//! the URL, not in storage. The product had forgotten which operation might
//! have happened, so it could only guess.
//!
//! The marker is the application's own memory of that operation. It is
//! deliberately the SMALLEST thing that can answer "which key might still be
//! running?": the request id, the workspace it was sent to, the account that
//! sent it, and when it was recorded. No business payload, no secret, no
//! contact, no summary — the marker names an operation, it does not carry
//! its content.
//!
//! ## What the marker is NOT
//!
//! It is not an approval, not a grant and not a receipt. It does not say the
//! operation happened: only the server's own status route does that, with
//! the ORIGINAL key. The marker's whole job is to keep that key reachable
//! after a reload, so the recovery path can ask the server the question it
//! already knows how to answer.
//!
//! ## Storage failure
//!
//! If the marker cannot be written, the submission does NOT happen. A
//! submission that cannot record its own pending identity is a submission
//! that would leave the application unable to recover it — which is the
//! defect this module exists to remove. Refusing to send is the honest
//! answer, not a degraded send.

/// The prefix every marker key carries.
#[cfg(any(target_arch = "wasm32", test))]
const KEY_PREFIX: &str = "archava.pending.";

/// What the application remembers about a submission whose outcome it does
/// not know.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingMarker {
    /// The idempotency key the submission used — the thing that must
    /// survive a reload.
    pub request_id: String,
    /// The workspace the submission was sent to.
    pub tenant: String,
    /// The domain/view scope (e.g. "contacts", "contact", "draft", "owner")
    /// used to find pending work for this view.
    pub domain: String,
    /// The account that sent it.
    pub principal: String,
    /// When the marker was recorded (RFC 3339, browser clock).
    pub created_at: String,
}

impl PendingMarker {
    /// The marker for a submission that is about to be sent.
    #[allow(dead_code)]
    pub fn new(request_id: String, tenant: String, principal: String) -> Self {
        Self::with_domain(request_id, tenant, "", principal)
    }

    /// The marker for a submission scoped to a specific domain or workflow.
    pub fn with_domain(
        request_id: String,
        tenant: String,
        domain: impl Into<String>,
        principal: String,
    ) -> Self {
        Self {
            request_id,
            tenant,
            domain: domain.into(),
            principal,
            created_at: now_rfc3339(),
        }
    }
}

/// One key per account, scope and request. Separate tabs cannot replace each other's intent.
#[cfg(any(target_arch = "wasm32", test))]
fn key_for(marker: &PendingMarker) -> String {
    format!(
        "{KEY_PREFIX}v2.{}",
        serde_json::json!([
            marker.tenant,
            marker.principal,
            marker.domain,
            marker.request_id
        ])
    )
}

#[cfg(any(target_arch = "wasm32", test))]
fn parse_marker(raw: &str) -> Option<PendingMarker> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    Some(PendingMarker {
        request_id: value["requestId"].as_str()?.into(),
        tenant: value["tenant"].as_str()?.into(),
        domain: value["domain"].as_str().unwrap_or_default().into(),
        principal: value["principal"].as_str()?.into(),
        created_at: value["createdAt"].as_str()?.into(),
    })
}

#[cfg(any(target_arch = "wasm32", test))]
fn oldest_in_scope(
    markers: Vec<PendingMarker>,
    tenant: &str,
    domain: &str,
    principal: &str,
) -> Option<PendingMarker> {
    markers
        .into_iter()
        .filter(|m| {
            m.tenant == tenant
                && m.principal == principal
                && (domain.is_empty() || m.domain.is_empty() || m.domain == domain)
        })
        .min_by(|a, b| (&a.created_at, &a.request_id).cmp(&(&b.created_at, &b.request_id)))
}

/// The browser clock, as RFC 3339 — the same shape the host's contracts use.
fn now_rfc3339() -> String {
    // The wasm build has `js_sys::Date`; the SSR build never records a
    // marker, so the fallback only has to compile.
    #[cfg(target_arch = "wasm32")]
    {
        let millis = js_sys::Date::now() as i64;
        // `chrono` is not a slice dependency; the marker only needs a
        // sortable stamp, and the ISO shape is built by hand.
        let seconds = millis / 1000;
        let (days, rest) = (seconds / 86_400, seconds % 86_400);
        let (hours, rest) = (rest / 3_600, rest % 3_600);
        let (minutes, secs) = (rest / 60, rest % 60);
        let (year, month, day) = civil_from_days(days);
        format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{secs:02}.000Z")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        String::new()
    }
}

/// Days since the Unix epoch → a civil (year, month, day). Howard
/// Hinnant's algorithm, the standard one; the marker only needs a readable
/// stamp, not a calendar service.
#[cfg(target_arch = "wasm32")]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Return the oldest unresolved marker in this account/scope, including legacy keys.
#[cfg(target_arch = "wasm32")]
pub(crate) fn read_scoped(tenant: &str, domain: &str, principal: &str) -> Option<PendingMarker> {
    oldest_in_scope(read_markers(), tenant, domain, principal)
}

#[cfg(target_arch = "wasm32")]
fn read_markers() -> Vec<PendingMarker> {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return Vec::new();
    };
    let mut markers = Vec::new();
    for index in 0..storage.length().unwrap_or(0) {
        let Some(key) = storage.key(index).ok().flatten() else {
            continue;
        };
        if !key.starts_with(KEY_PREFIX) {
            continue;
        }
        if let Some(marker) = storage
            .get_item(&key)
            .ok()
            .flatten()
            .and_then(|raw| parse_marker(&raw))
        {
            markers.push(marker);
        }
    }
    markers
}

/// Recover a bounded workflow whose scope carries only its resource reference.
#[cfg(target_arch = "wasm32")]
pub(crate) fn read_family(tenant: &str, family: &str, principal: &str) -> Option<PendingMarker> {
    let prefix = format!("{family}:");
    oldest_in_scope(
        read_markers()
            .into_iter()
            .filter(|m| m.domain.starts_with(&prefix))
            .collect(),
        tenant,
        "",
        principal,
    )
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_family(_tenant: &str, _family: &str, _principal: &str) -> Option<PendingMarker> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_scoped(_tenant: &str, _domain: &str, _principal: &str) -> Option<PendingMarker> {
    None
}

/// Record the marker. Returns the failure reason when the browser would not
/// take it — and a submission must not proceed when this fails.
#[cfg(target_arch = "wasm32")]
pub(crate) fn write(marker: &PendingMarker) -> Result<(), String> {
    let storage = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or_else(|| "this browser has no local storage".to_string())?;
    let payload = serde_json::json!({
        "requestId": marker.request_id,
        "tenant": marker.tenant,
        "domain": marker.domain,
        "principal": marker.principal,
        "createdAt": marker.created_at,
    });
    storage
        .set_item(&key_for(marker), &payload.to_string())
        .map_err(|error| format!("the browser refused the pending marker: {error:?}"))
}

/// The SSR build cannot record anything, and says so.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write(_marker: &PendingMarker) -> Result<(), String> {
    Err("this browser has no local storage".to_string())
}

/// Clear any marker across domains matching a specific idempotency request_id.
#[cfg(target_arch = "wasm32")]
pub(crate) fn clear_by_request_id(tenant: &str, principal: &str, request_id: &str) {
    let Some(storage) = web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    else {
        return;
    };
    let Ok(length) = storage.length() else {
        return;
    };
    let mut to_remove = Vec::new();
    for index in 0..length {
        let Ok(Some(key)) = storage.key(index) else {
            continue;
        };
        if key.starts_with(KEY_PREFIX) {
            if let Ok(Some(raw)) = storage.get_item(&key) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if val["requestId"].as_str() == Some(request_id)
                        && val["tenant"].as_str() == Some(tenant)
                        && val["principal"].as_str() == Some(principal)
                    {
                        to_remove.push(key);
                    }
                }
            }
        }
    }
    for key in to_remove {
        let _ = storage.remove_item(&key);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clear_by_request_id(_tenant: &str, _principal: &str, _request_id: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_requests_and_accounts_keep_distinct_storage_keys() {
        let marker = PendingMarker::with_domain(
            "a".into(),
            "workspace:one".into(),
            "contact",
            "alice".into(),
        );
        let mut second = marker.clone();
        second.request_id = "b".into();
        let mut other_account = marker.clone();
        other_account.principal = "bob".into();
        assert_ne!(key_for(&marker), key_for(&second));
        assert_ne!(key_for(&marker), key_for(&other_account));
        assert!(oldest_in_scope(
            vec![other_account, second.clone(), marker.clone()],
            "workspace:one",
            "contact",
            "alice"
        )
        .is_some_and(|m| m.request_id == "a"));
        assert_eq!(
            oldest_in_scope(vec![second.clone()], "workspace:one", "contact", "alice"),
            Some(second)
        );
        assert!(oldest_in_scope(vec![marker], "workspace:one", "owner", "alice").is_none());
    }

    #[test]
    fn legacy_unscoped_pending_keys_remain_recoverable_only_by_their_account() {
        let marker = parse_marker(r#"{"requestId":"old-key","tenant":"old","principal":"alice","createdAt":"2026-10-29T00:00:00Z"}"#).expect("legacy marker");
        assert_eq!(
            oldest_in_scope(vec![marker.clone()], "old", "draft", "alice"),
            Some(marker.clone())
        );
        assert!(oldest_in_scope(vec![marker], "old", "draft", "bob").is_none());
        assert!(parse_marker(r#"{"requestId":"bad"}"#).is_none());
    }

    #[test]
    fn marker_with_domain_preserves_scope_and_idempotency_key() {
        let marker = PendingMarker::with_domain(
            "req-123".into(),
            "acme-corp".into(),
            "contacts",
            "user-1".into(),
        );
        assert_eq!(marker.request_id, "req-123");
        assert_eq!(marker.tenant, "acme-corp");
        assert_eq!(marker.domain, "contacts");
        assert_eq!(marker.principal, "user-1");
    }

    #[test]
    fn unscoped_marker_defaults_to_empty_domain() {
        let marker = PendingMarker::new("req-456".into(), "acme-corp".into(), "user-2".into());
        assert_eq!(marker.domain, "");
    }
}
