//! lifeping: a tiny page telling friends and family whether its owner is alive.

pub mod api;
pub mod assets;
pub mod config;
pub mod site;
pub mod status;
pub mod store;

use std::io;
use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::map_response;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

pub use config::Config;
pub use status::Status;
pub use store::Store;

const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
    img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; \
    frame-ancestors 'none'";

pub struct AppState {
    pub config: Config,
    pub store: Store,
    /// `web/index.html` with the configured title and strings filled in.
    pub index_html: String,
}

impl AppState {
    /// Opens the store in the configured data directory.
    pub fn new(config: Config) -> io::Result<Arc<Self>> {
        let store = Store::open(&config.data_dir)?;
        let index_html = site::render_index(&config.title, &config.strings);
        Ok(Arc::new(Self {
            config,
            store,
            index_html,
        }))
    }
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/status", get(api::get_status))
        .route("/ping", post(api::post_ping))
        .layer(map_response(no_store));

    Router::new()
        .route("/", get(assets::index))
        .route("/app.js", get(assets::app_js))
        .route("/style.css", get(assets::style_css))
        .route("/fonts/lifeping-title.woff2", get(assets::title_font))
        .route("/healthz", get(healthz))
        .nest("/api", api)
        .fallback(not_found)
        .layer(map_response(security_headers))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found")
}

async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}
