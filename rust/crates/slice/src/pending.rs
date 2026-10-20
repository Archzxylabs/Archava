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
#[cfg(target_arch = "wasm32")]
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
    /// The account that sent it.
    pub principal: String,
    /// When the marker was recorded (RFC 3339, browser clock).
    pub created_at: String,
}

impl PendingMarker {
    /// The marker for a submission that is about to be sent.
    pub fn new(request_id: String, tenant: String, principal: String) -> Self {
        Self {
            request_id,
            tenant,
            principal,
            created_at: now_rfc3339(),
        }
    }
}

/// The storage key for one workspace's marker.
#[cfg(target_arch = "wasm32")]
fn key_for(tenant: &str) -> String {
    format!("{KEY_PREFIX}{tenant}")
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

/// The marker this workspace holds, if any.
///
/// A marker for ANOTHER account is not this account's business: the read is
/// scoped to the account and workspace that are asking.
#[cfg(target_arch = "wasm32")]
pub(crate) fn read(tenant: &str, principal: &str) -> Option<PendingMarker> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(&key_for(tenant)).ok()??;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let marker = PendingMarker {
        request_id: value["requestId"].as_str()?.to_string(),
        tenant: value["tenant"].as_str()?.to_string(),
        principal: value["principal"].as_str()?.to_string(),
        created_at: value["createdAt"].as_str()?.to_string(),
    };
    if marker.tenant != tenant || marker.principal != principal {
        return None;
    }
    Some(marker)
}

/// The SSR build has no storage and therefore no marker.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read(_tenant: &str, _principal: &str) -> Option<PendingMarker> {
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
        "principal": marker.principal,
        "createdAt": marker.created_at,
    });
    storage
        .set_item(&key_for(&marker.tenant), &payload.to_string())
        .map_err(|error| format!("the browser refused the pending marker: {error:?}"))
}

/// The SSR build cannot record anything, and says so.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write(_marker: &PendingMarker) -> Result<(), String> {
    Err("this browser has no local storage".to_string())
}

/// Forget one workspace's marker — the outcome became known, or the account
/// moved on.
#[cfg(target_arch = "wasm32")]
pub(crate) fn clear(tenant: &str) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        let _ = storage.remove_item(&key_for(tenant));
    }
}

/// The SSR build has nothing to forget.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clear(_tenant: &str) {}

/// Forget every marker this origin holds.
///
/// Signing out is one of those moments: the pending operation belonged to
/// the account that just left, and a signed-out browser must not keep
/// another account's private recovery state around.
#[cfg(target_arch = "wasm32")]
pub(crate) fn clear_all() {
    let Some(storage) = web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    else {
        return;
    };
    let Ok(length) = storage.length() else {
        return;
    };
    for index in 0..length {
        let Ok(Some(key)) = storage.key(index) else {
            continue;
        };
        if key.starts_with(KEY_PREFIX) {
            let _ = storage.remove_item(&key);
        }
    }
}

/// The SSR build has nothing to forget.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub(crate) fn clear_all() {}
