//! `/api/*` handlers.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::json;
use subtle::ConstantTimeEq;
use time::OffsetDateTime;

use crate::AppState;
use crate::status::{self, Status};
use crate::store::format_timestamp;

#[derive(Serialize)]
pub struct StatusResponse {
    now: String,
    status: Status,
    latest: Option<String>,
    history: Vec<String>,
    total_pings: usize,
    thresholds: Thresholds,
}

#[derive(Serialize)]
pub struct Thresholds {
    yellow_after_secs: i64,
    red_after_secs: i64,
}

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let config = &state.config;
    let snapshot = state.store.snapshot(config.history).await;
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("0 is a valid nanosecond");

    Json(StatusResponse {
        now: format_timestamp(now),
        status: status::compute(snapshot.latest, now, config.yellow_after, config.red_after),
        latest: snapshot.latest.map(format_timestamp),
        history: snapshot.recent.into_iter().map(format_timestamp).collect(),
        total_pings: snapshot.total,
        thresholds: Thresholds {
            yellow_after_secs: config.yellow_after.whole_seconds(),
            red_after_secs: config.red_after.whole_seconds(),
        },
    })
}

pub async fn post_ping(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !is_authorized(&headers, &state.config.token) {
        tracing::warn!("rejected ping with missing or invalid credentials");
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }

    match state.store.append().await {
        Ok(ts) => {
            let timestamp = format_timestamp(ts);
            tracing::info!(%timestamp, "ping recorded");
            Json(json!({ "timestamp": timestamp })).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to record ping");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "storage failure" })),
            )
                .into_response()
        }
    }
}

fn is_authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some((scheme, presented)) = value.split_once(' ') else {
        return false;
    };
    // The auth scheme is case-insensitive (RFC 9110 §11.1).
    scheme.eq_ignore_ascii_case("bearer")
        && bool::from(presented.trim().as_bytes().ct_eq(expected.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(auth: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, auth.parse().unwrap());
        h
    }

    #[test]
    fn authorization_parsing() {
        assert!(is_authorized(&headers("Bearer secret"), "secret"));
        assert!(is_authorized(&headers("bearer secret"), "secret"));
        assert!(!is_authorized(&HeaderMap::new(), "secret"));
        assert!(!is_authorized(&headers("Bearer"), "secret"));
        assert!(!is_authorized(&headers("Bearer "), "secret"));
        assert!(!is_authorized(&headers("Bearer secre"), "secret"));
        assert!(!is_authorized(&headers("Bearer secrets"), "secret"));
        assert!(!is_authorized(&headers("Basic secret"), "secret"));
        assert!(!is_authorized(&headers("secret"), "secret"));
    }
}
