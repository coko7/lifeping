//! Frontend files embedded at compile time.

use axum::http::header;
use axum::response::IntoResponse;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
}

pub async fn index() -> impl IntoResponse {
    asset("text/html; charset=utf-8", INDEX_HTML)
}

pub async fn app_js() -> impl IntoResponse {
    asset("text/javascript; charset=utf-8", APP_JS)
}

pub async fn style_css() -> impl IntoResponse {
    asset("text/css; charset=utf-8", STYLE_CSS)
}
