use std::{env, future::IntoFuture as _, net::SocketAddr, time::Duration};

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone)]
struct ServerConfig {
    bind_address: SocketAddr,
    public_url: Option<String>,
    json_logs: bool,
    shutdown_timeout: Duration,
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
        let public_url = env::var("CROWNLINE_PUBLIC_URL").ok();
        if public_url
            .as_ref()
            .is_some_and(|url| !(url.starts_with("http://") || url.starts_with("https://")))
        {
            anyhow::bail!("CROWNLINE_PUBLIC_URL must use http:// or https://");
        }
        let shutdown_seconds = env::var("CROWNLINE_SHUTDOWN_SECONDS")
            .unwrap_or_else(|_| "15".to_owned())
            .parse::<u64>()
            .context("CROWNLINE_SHUTDOWN_SECONDS must be an integer")?;
        if !(1..=300).contains(&shutdown_seconds) {
            anyhow::bail!("CROWNLINE_SHUTDOWN_SECONDS must be between 1 and 300");
        }

        Ok(Self {
            bind_address,
            public_url,
            json_logs,
            shutdown_timeout: Duration::from_secs(shutdown_seconds),
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

    info!(
        address = %config.bind_address,
        public_url = config.public_url.as_deref().unwrap_or("unset"),
        "Crownlines server listening"
    );
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = shutdown_receiver.await;
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => {
            result.context("server terminated unexpectedly")?;
        }
        () = shutdown_signal() => {
            info!(drain_seconds = config.shutdown_timeout.as_secs(), "shutdown requested; draining connections");
            let _ = shutdown_sender.send(());
            if let Ok(result) = tokio::time::timeout(config.shutdown_timeout, &mut server).await {
                result.context("server terminated unexpectedly")?;
            } else {
                warn!("graceful drain deadline reached; closing remaining connections");
            }
        }
    }
    info!("Crownlines server stopped");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => {
                tracing::error!(%error, "failed to listen for SIGTERM");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    tracing::error!(%error, "failed to listen for Ctrl-C");
                }
            }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to listen for Ctrl-C");
    }
}
