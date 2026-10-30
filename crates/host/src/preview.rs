//! Live preview generator route for Ava consultation sessions.
//!
//! Exposes `POST /api/preview/generate` which parses sector and brand inputs
//! and synthesizes an interactive commercial projection for immediate client rendering.

use archava_domain::preview::{generate_preview, PreviewProjection, Sector};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub sector: String,
    pub brand_name: String,
    pub tagline: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PreviewResponse {
    pub ok: bool,
    pub preview: Option<PreviewProjection>,
    pub error: Option<String>,
}

pub async fn generate_preview_route(
    Json(payload): Json<PreviewRequest>,
) -> (StatusCode, Json<PreviewResponse>) {
    let Some(sector) = Sector::parse(&payload.sector) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(PreviewResponse {
                ok: false,
                preview: None,
                error: Some(format!(
                    "unknown sector '{}'; available sectors: {:?}",
                    payload.sector,
                    Sector::ALL.map(|s| s.as_str())
                )),
            }),
        );
    };

    let projection = generate_preview(sector, &payload.brand_name, payload.tagline.as_deref());

    (
        StatusCode::OK,
        Json(PreviewResponse {
            ok: true,
            preview: Some(projection),
            error: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generate_preview_route_success() {
        let req = PreviewRequest {
            sector: "clinic_esthetics".to_string(),
            brand_name: "Aura Clinic".to_string(),
            tagline: Some("Aesthetic & Laser Studio".to_string()),
        };
        let (status, Json(res)) = generate_preview_route(Json(req)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(res.ok);
        assert!(res.preview.is_some());
        let prev = res.preview.unwrap();
        assert_eq!(prev.brand.name, "Aura Clinic");
        assert_eq!(prev.brand.primary_color, "#be185d");
    }

    #[tokio::test]
    async fn generate_preview_route_invalid_sector() {
        let req = PreviewRequest {
            sector: "unknown_sector".to_string(),
            brand_name: "Test".to_string(),
            tagline: None,
        };
        let (status, Json(res)) = generate_preview_route(Json(req)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!res.ok);
        assert!(res.error.is_some());
    }
}
