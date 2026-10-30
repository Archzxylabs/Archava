//! HTTP endpoints for Industry Presets and Proactive Action Feed.
//! Powers zero-config onboarding and frictionless, proactive daily operations.

use archava_domain::feed::generate_action_feed;
use archava_domain::presets::IndustryProfile;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::router::AppState;

#[derive(Debug, Deserialize)]
pub struct FeedQueryParams {
    pub tenant: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ExecuteActionResponse {
    pub status: String,
    pub card_id: String,
    pub action_type: String,
    pub summary: String,
    pub receipt_id: String,
    pub is_simulated: bool,
}

/// GET /api/v1/presets/profiles
/// Returns all available industry profile masks for zero-config onboarding.
pub async fn list_profiles() -> (StatusCode, Json<serde_json::Value>) {
    let profiles = vec![
        IndustryProfile::FnBCafe.mask(),
        IndustryProfile::ClinicHealthcare.mask(),
        IndustryProfile::HospitalityHotel.mask(),
        IndustryProfile::RetailCommerce.mask(),
        IndustryProfile::LogisticsFleet.mask(),
        IndustryProfile::ProfessionalServices.mask(),
    ];

    (StatusCode::OK, Json(json!({ "profiles": profiles })))
}

/// GET /api/v1/presets/profiles/{slug}
/// Returns a single tailored profile mask by its slug.
pub async fn get_profile(Path(slug): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    match IndustryProfile::parse(&slug) {
        Some(profile) => (StatusCode::OK, Json(json!(profile.mask()))),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "message": "Unknown industry profile." } })),
        ),
    }
}

/// GET /api/v1/feed/action-cards
/// Returns proactive action cards tailored to the tenant and industry profile.
pub async fn get_action_cards(
    Query(params): Query<FeedQueryParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    let tenant = params.tenant.as_deref().unwrap_or("default");
    let profile = params
        .profile
        .as_deref()
        .and_then(IndustryProfile::parse)
        .unwrap_or(IndustryProfile::FnBCafe);

    // Synthesize real-world metrics for the tenant
    let feed = generate_action_feed(
        tenant,
        profile,
        3,
        1_420_000_000, // Rp 14.200.000 overdue
        &["Biji Kopi Arabica 1kg", "Susu UHT Fresh"],
    );

    (StatusCode::OK, Json(json!({ "feed": feed })))
}

/// POST /api/v1/feed/action-cards/{id}/execute
/// Executes an action card trigger (1-tap dispatch).
pub async fn execute_action(
    State(_state): State<AppState>,
    Path(card_id): Path<String>,
    body: Result<Json<ExecuteActionRequest>, axum::extract::rejection::JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Ok(Json(req)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "message": "Invalid execution payload." } })),
        );
    };

    let summary = match req.action_type.as_str() {
        "dispatch_ar_reminders" => {
            "Pengingat piutang otomatis berhasil dikirim melalui WhatsApp beserta link pembayaran QRIS."
        }
        "generate_reorder_po" => {
            "Draft Purchase Order (PO) pengadaan supplier berhasil dibuat dan masuk antrean persetujuan."
        }
        "print_shift_summary" => {
            "Ringkasan omzet dan mutasi kas shift berhasil diproses ke printer thermal ESC/POS."
        }
        "blast_patient_confirmations" => {
            "Pesan konfirmasi reservasi (H-1) berhasil dikirimkan ke 12 pasien terjadwal."
        }
        "issue_milestone_invoice" => {
            "Invoice termin proyek berhasil diterbitkan dengan e-Faktur Coretax terverifikasi."
        }
        _ => "Aksi operasional berhasil dieksekusi oleh Ava.",
    };

    (
        StatusCode::OK,
        Json(json!(ExecuteActionResponse {
            status: "accepted_pending_worker".into(),
            card_id: card_id.clone(),
            action_type: req.action_type,
            summary: format!("[SIMULATED_PREVIEW] {}", summary),
            receipt_id: format!("RCPT-FEED-{}", card_id),
            is_simulated: true,
        })),
    )
}
