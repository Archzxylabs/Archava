//! Live preview generator route for Ava consultation sessions.
//!
//! Exposes `POST /api/preview/generate` which parses sector and brand inputs
//! and synthesizes an interactive commercial projection for immediate client
//! rendering.
//!
//! Two things changed after the audit. The route sits behind the host's
//! session and CSRF chain instead of being anonymous, and both refusals are
//! fixed messages: the previous version returned
//! `unknown sector '<input>'; available sectors: …`, which reflected the
//! caller's own string back at it — the D09 shape the command surface had
//! already removed.

use archava_domain::preview::{generate_preview, PreviewProjection, Sector};
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::router::{require_session, AppState};

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
    pub available_sectors: Option<Vec<String>>,
}

pub async fn generate_preview_route(
    state: axum::extract::State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<PreviewRequest>, JsonRejection>,
) -> (StatusCode, Json<PreviewResponse>) {
    if let Err((status, _reason)) = require_session(&state, &headers, true).await {
        return (
            status,
            Json(PreviewResponse {
                ok: false,
                preview: None,
                error: Some("unauthenticated".to_string()),
                available_sectors: None,
            }),
        );
    }
    let payload = match payload {
        Ok(Json(payload)) => payload,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(PreviewResponse {
                    ok: false,
                    preview: None,
                    error: Some("malformed_submission".to_string()),
                    available_sectors: None,
                }),
            )
        }
    };
    let Some(sector) = Sector::parse(&payload.sector) else {
        // The available list is a fixed catalogue; the submitted value is not
        // repeated anywhere in the refusal.
        return (
            StatusCode::BAD_REQUEST,
            Json(PreviewResponse {
                ok: false,
                preview: None,
                error: Some("unknown_sector".to_string()),
                available_sectors: Some(
                    Sector::ALL
                        .map(|sector| sector.as_str().to_string())
                        .to_vec(),
                ),
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
            available_sectors: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;

    /// The generator itself is a pure function; these cases exercise it
    /// directly because the route now requires a session this unit test has no
    /// database to mint.
    #[test]
    fn the_sector_catalogue_resolves_a_known_sector() {
        let sector = Sector::parse("clinic_esthetics").expect("a catalogued sector");
        let projection = generate_preview(sector, "Aura Clinic", Some("Aesthetic & Laser Studio"));
        assert_eq!(projection.brand.name, "Aura Clinic");
        assert_eq!(projection.brand.primary_color, "#be185d");
    }

    #[test]
    fn an_unknown_sector_is_not_in_the_catalogue() {
        assert!(Sector::parse("unknown_sector").is_none());
    }

    #[tokio::test]
    async fn an_unauthenticated_preview_is_refused_before_extraction() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://archava:archava@127.0.0.1:1/archava_test")
            .expect("a lazy pool must construct without connecting");
        let identity = crate::identity::Identity::unchecked(pool.clone(), &[7u8; 32]);
        let state = State(AppState::new(pool, crate::router::test_ledger(), identity));
        let (status, Json(response)) = generate_preview_route(
            state,
            HeaderMap::new(),
            Ok(Json(PreviewRequest {
                sector: "clinic_esthetics".to_string(),
                brand_name: "Aura Clinic".to_string(),
                tagline: None,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some("unauthenticated"));
    }
}
