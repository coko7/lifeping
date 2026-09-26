use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use lifeping::{AppState, Config, build_app, config};
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
    match mode.as_deref() {
        None | Some("serve") => serve(),
        Some("healthcheck") => healthcheck(),
        Some(other) => {
            eprintln!("unknown command {other:?}; usage: lifeping [serve|healthcheck]");
            ExitCode::from(2)
        }
    }
}

fn serve() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("lifeping=info")),
        )
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("invalid configuration: {e}");
            return ExitCode::FAILURE;
        }
    };
    let bind = config.bind;
    let state = match AppState::new(config) {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("cannot open ping log: {e}");
            return ExitCode::FAILURE;
        }
    };

    let runtime = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(bind).await {
            Ok(listener) => listener,
            Err(e) => {
                tracing::error!("cannot bind {bind}: {e}");
                return ExitCode::FAILURE;
            }
        };
        tracing::info!("listening on {bind}");
        match axum::serve(listener, build_app(state))
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            Ok(()) => {
                tracing::info!("shut down cleanly");
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("server error: {e}");
                ExitCode::FAILURE
            }
        }
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for SIGINT");
    };
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

/// `GET /healthz` over a raw TCP connection; the runtime image has no curl.
fn healthcheck() -> ExitCode {
    let bind = match config::parse_bind(std::env::var("LIFEPING_BIND").ok()) {
        Ok(bind) => bind,
        Err(e) => {
            eprintln!("healthcheck: {e}");
            return ExitCode::FAILURE;
        }
    };
    match probe(local_target(bind)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("healthcheck: unexpected response");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("healthcheck: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A wildcard bind address is reached through loopback; a specific one directly.
fn local_target(bind: SocketAddr) -> SocketAddr {
    let ip = match bind.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, bind.port())
}

fn probe(target: SocketAddr) -> std::io::Result<bool> {
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&target, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write!(
        stream,
        "GET /healthz HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    stream.take(1024).read_to_end(&mut response)?;
    let status_line = response.split(|&b| b == b'\n').next().unwrap_or_default();
    let mut parts = status_line.split(|&b| b == b' ');
    Ok(parts.next().is_some_and(|v| v.starts_with(b"HTTP/1.")) && parts.next() == Some(b"200"))
}
