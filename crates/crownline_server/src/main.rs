use std::{env, net::SocketAddr};

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone)]
struct ServerConfig {
    bind_address: SocketAddr,
    json_logs: bool,
}

impl ServerConfig {
    fn from_env() -> Result<Self> {
        let bind_address = env::var("CROWNLINE_BIND")
            .unwrap_or_else(|_| "127.0.0.1:5000".to_owned())
            .parse()
            .context("CROWNLINE_BIND must be a socket address such as 127.0.0.1:5000")?;
        let json_logs = env::var("CROWNLINE_LOG_FORMAT")
            .unwrap_or_else(|_| "pretty".to_owned())
            .eq_ignore_ascii_case("json");

        Ok(Self {
            bind_address,
            json_logs,
        })
    }
}

fn init_tracing(json_logs: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt().with_env_filter(filter);
    if json_logs {
        subscriber.json().init();
    } else {
        subscriber.init();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = ServerConfig::from_env()?;
    init_tracing(config.json_logs);

    let limits = crownline_server::limits::ServerLimits::from_env().map_err(anyhow::Error::msg)?;
    let database_path =
        env::var("CROWNLINE_DATABASE_PATH").unwrap_or_else(|_| "crownline.sqlite3".to_owned());
    let durability = match env::var("CROWNLINE_DATABASE_DURABILITY")
        .unwrap_or_else(|_| "full".to_owned())
        .to_ascii_lowercase()
        .as_str()
    {
        "full" => crownline_server::database::Durability::Full,
        "normal" => crownline_server::database::Durability::Normal,
        _ => anyhow::bail!("CROWNLINE_DATABASE_DURABILITY must be full or normal"),
    };
    let app = crownline_server::app_with_database(limits, database_path, durability)?
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(config.bind_address)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_address))?;

    info!(address = %config.bind_address, "Crownlines server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("server terminated unexpectedly")?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to listen for shutdown signal");
    }
}
