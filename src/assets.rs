//! Frontend files embedded at compile time.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;

use crate::AppState;

const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");
/// Dancing Script Bold, subset to Latin-1 (see web/fonts/README.md).
const TITLE_FONT: &[u8] = include_bytes!("../web/fonts/lifeping-title.woff2");

fn asset<B: IntoResponse>(content_type: &'static str, body: B) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
}

pub async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    asset("text/html; charset=utf-8", state.index_html.clone())
}

pub async fn app_js() -> impl IntoResponse {
    asset("text/javascript; charset=utf-8", APP_JS)
}

pub async fn style_css() -> impl IntoResponse {
    asset("text/css; charset=utf-8", STYLE_CSS)
}

pub async fn title_font() -> impl IntoResponse {
    asset("font/woff2", TITLE_FONT)
}
