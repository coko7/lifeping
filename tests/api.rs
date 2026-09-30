use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use lifeping::{AppState, Config, build_app};
use serde_json::Value;
use tempfile::TempDir;
use time::Duration;
use tower::ServiceExt;

const TOKEN: &str = "test-token-0123456789abcdef0123456789";

fn config(data_dir: &Path, history: usize) -> Config {
    Config {
        token: TOKEN.into(),
        yellow_after: Duration::hours(12),
        red_after: Duration::hours(24),
        history,
        data_dir: data_dir.to_path_buf(),
        bind: "127.0.0.1:0".parse().unwrap(),
    }
}

fn app_in(data_dir: &Path, history: usize) -> Router {
    let state: Arc<AppState> = AppState::new(config(data_dir, history)).unwrap();
    build_app(state)
}

fn app() -> (TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path(), 10);
    (dir, app)
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("body should be JSON")
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(&name)
            .unwrap_or_else(|| panic!("missing header {name}"))
            .to_str()
            .unwrap()
    }
}

async fn send(app: &Router, method: Method, uri: &str, auth: Option<&str>) -> Reply {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(auth) = auth {
        req = req.header(header::AUTHORIZATION, auth);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: body.collect().await.unwrap().to_bytes().to_vec(),
    }
}

async fn status(app: &Router) -> Value {
    let reply = send(app, Method::GET, "/api/status", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.json()
}

async fn ping(app: &Router) -> String {
    let bearer = format!("Bearer {TOKEN}");
    let reply = send(app, Method::POST, "/api/ping", Some(&bearer)).await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.json()["timestamp"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn empty_store_is_unknown() {
    let (_dir, app) = app();
    let reply = send(&app, Method::GET, "/api/status", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CONTENT_TYPE), "application/json");
    assert_eq!(reply.header(header::CACHE_CONTROL), "no-store");

    let body = reply.json();
    assert_eq!(body["status"], "unknown");
    assert!(body["latest"].is_null());
    assert_eq!(body["history"], serde_json::json!([]));
    assert_eq!(body["total_pings"], 0);
    assert_eq!(body["thresholds"]["yellow_after_secs"], 43200);
    assert_eq!(body["thresholds"]["red_after_secs"], 86400);
    let now = body["now"].as_str().unwrap();
    assert!(now.ends_with('Z') && now.len() == 20, "now = {now}");
}

#[tokio::test]
async fn ping_rejects_bad_credentials() {
    let (dir, app) = app();
    let cases = [
        None,
        Some("Bearer wrong-token"),
        Some(&*format!("Basic {TOKEN}")),
        Some(TOKEN),
        Some("Bearer"),
    ];
    for auth in cases {
        let reply = send(&app, Method::POST, "/api/ping", auth).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "auth = {auth:?}");
        assert_eq!(reply.header(header::WWW_AUTHENTICATE), "Bearer");
        assert_eq!(reply.header(header::CACHE_CONTROL), "no-store");
        assert_eq!(reply.json(), serde_json::json!({ "error": "unauthorized" }));
    }
    assert_eq!(status(&app).await["total_pings"], 0);
    assert!(!dir.path().join("pings.log").exists());
}

#[tokio::test]
async fn valid_ping_turns_status_green() {
    let (dir, app) = app();
    let timestamp = ping(&app).await;
    assert!(
        time::OffsetDateTime::parse(&timestamp, &time::format_description::well_known::Rfc3339)
            .is_ok()
    );

    let body = status(&app).await;
    assert_eq!(body["status"], "green");
    assert_eq!(body["latest"], timestamp);
    assert_eq!(body["history"], serde_json::json!([timestamp]));
    assert_eq!(body["total_pings"], 1);

    let log = std::fs::read_to_string(dir.path().join("pings.log")).unwrap();
    assert_eq!(log, format!("{timestamp}\n"));
}

#[tokio::test]
async fn history_is_newest_first_and_truncated() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pings.log"),
        "2026-01-03T00:00:00Z\n2026-01-01T00:00:00Z\n2026-01-04T00:00:00Z\n2026-01-02T00:00:00Z\n",
    )
    .unwrap();
    let app = app_in(dir.path(), 3);

    let body = status(&app).await;
    assert_eq!(body["status"], "red");
    assert_eq!(body["latest"], "2026-01-04T00:00:00Z");
    assert_eq!(
        body["history"],
        serde_json::json!([
            "2026-01-04T00:00:00Z",
            "2026-01-03T00:00:00Z",
            "2026-01-02T00:00:00Z",
        ])
    );
    assert_eq!(body["total_pings"], 4);

    let timestamp = ping(&app).await;
    let body = status(&app).await;
    assert_eq!(body["history"].as_array().unwrap().len(), 3);
    assert_eq!(body["history"][0], timestamp);
    assert_eq!(body["history"][1], "2026-01-04T00:00:00Z");
    assert_eq!(body["total_pings"], 5);
}

#[tokio::test]
async fn pings_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let first = ping(&app_in(dir.path(), 10)).await;

    let app = app_in(dir.path(), 10);
    let body = status(&app).await;
    assert_eq!(body["latest"], first);
    assert_eq!(body["total_pings"], 1);

    ping(&app).await;
    assert_eq!(status(&app_in(dir.path(), 10)).await["total_pings"], 2);
}

#[tokio::test]
async fn healthz() {
    let (_dir, app) = app();
    let reply = send(&app, Method::GET, "/healthz", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, b"ok");
}

#[tokio::test]
async fn static_assets() {
    let (_dir, app) = app();
    let cases = [
        ("/", "text/html; charset=utf-8", "<!doctype html>"),
        (
            "/app.js",
            "text/javascript; charset=utf-8",
            "\"use strict\";",
        ),
        ("/style.css", "text/css; charset=utf-8", ":root"),
        ("/fonts/lifeping-title.woff2", "font/woff2", "wOF2"),
    ];
    for (uri, content_type, prefix) in cases {
        let reply = send(&app, Method::GET, uri, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        assert_eq!(reply.header(header::CONTENT_TYPE), content_type, "{uri}");
        assert_eq!(reply.header(header::CACHE_CONTROL), "no-cache", "{uri}");
        assert!(reply.body.starts_with(prefix.as_bytes()), "{uri}");
    }
}

#[tokio::test]
async fn unknown_paths_are_404() {
    let (_dir, app) = app();
    for uri in [
        "/nope",
        "/index.html",
        "/api",
        "/api/unknown",
        "/web/app.js",
    ] {
        let reply = send(&app, Method::GET, uri, None).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn security_headers_everywhere() {
    let (_dir, app) = app();
    let requests = [
        (Method::GET, "/"),
        (Method::GET, "/app.js"),
        (Method::GET, "/api/status"),
        (Method::POST, "/api/ping"),
        (Method::GET, "/healthz"),
        (Method::GET, "/missing"),
    ];
    for (method, uri) in requests {
        let reply = send(&app, method, uri, None).await;
        assert_eq!(
            reply.header(header::CONTENT_SECURITY_POLICY),
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
             font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            "{uri}"
        );
        assert_eq!(
            reply.header(header::X_CONTENT_TYPE_OPTIONS),
            "nosniff",
            "{uri}"
        );
        assert_eq!(
            reply.header(header::REFERRER_POLICY),
            "no-referrer",
            "{uri}"
        );
    }
}
