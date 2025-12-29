//! Circuit Breaker Webhook Server
//!
//! HTTP webhook server for ingesting external events into Circuit Breaker.
//!
//! This server receives webhooks from external services (GitHub, GitLab, Docker Hub, etc.),
//! validates their authenticity, normalizes them to CloudEvents format, and publishes
//! them to NATS for downstream processing.
//!
//! ## Usage
//!
//! ```bash
//! cb-webhook --port 8081 --nats-url nats://localhost:4222 --config /etc/cb/webhooks
//! ```

use cb_webhook::{connect_nats, init_nats_streams, serve, AppState, ServerConfig};
use clap::Parser;
use std::net::IpAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Circuit Breaker Webhook Server
#[derive(Parser, Debug)]
#[command(name = "cb-webhook")]
#[command(version, about = "Circuit Breaker Webhook Server", long_about = None)]
struct Args {
    /// Host to bind to
    #[arg(long, env = "CB_WEBHOOK_HOST", default_value = "0.0.0.0")]
    host: IpAddr,

    /// Port to listen on
    #[arg(short, long, env = "CB_WEBHOOK_PORT", default_value_t = 8081)]
    port: u16,

    /// NATS server URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    /// Path to webhook endpoint configuration file or directory
    #[arg(short, long, env = "CB_WEBHOOK_CONFIG")]
    config: Option<String>,

    /// Enable metrics endpoint
    #[arg(long, env = "CB_WEBHOOK_METRICS_ENABLED", default_value_t = true)]
    metrics_enabled: bool,

    /// Metrics endpoint port
    #[arg(long, env = "CB_WEBHOOK_METRICS_PORT", default_value_t = 9091)]
    metrics_port: u16,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, env = "CB_LOG_LEVEL", default_value = "info")]
    log_level: String,

    /// Enable JSON log format
    #[arg(long, env = "CB_LOG_JSON", default_value_t = false)]
    log_json: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("cb_webhook={},tower_http=debug", args.log_level).into());

    if args.log_json {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    tracing::info!(
        host = %args.host,
        port = %args.port,
        nats_url = %args.nats_url,
        config = ?args.config,
        metrics_enabled = %args.metrics_enabled,
        metrics_port = %args.metrics_port,
        "Starting Circuit Breaker Webhook Server"
    );

    // Connect to NATS
    let nats = match connect_nats(&args.nats_url).await {
        Ok(client) => {
            tracing::info!("Connected to NATS");

            // Initialize streams
            if let Err(e) = init_nats_streams(&client).await {
                tracing::warn!(error = %e, "Failed to initialize NATS streams");
            }
            Some(client)
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to connect to NATS - running in standalone mode (events will not be published)"
            );
            None
        }
    };

    // Create configuration
    let config = ServerConfig {
        host: args.host,
        port: args.port,
        nats_url: args.nats_url,
        config_path: args.config,
        metrics_enabled: args.metrics_enabled,
        metrics_port: args.metrics_port,
    };

    // Create application state
    let state = match nats {
        Some(client) => AppState::with_nats(client, config.clone()),
        None => {
            let mut state = AppState::default();
            state.config = config.clone();
            state
        }
    };

    // Setup signal handlers for graceful shutdown
    let shutdown_state = state.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("Received SIGINT, initiating shutdown");
                if let Some(ref tx) = shutdown_state.shutdown_tx {
                    let _ = tx.send(());
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to listen for SIGINT");
            }
        }
    });

    // Start server
    serve(config, state).await?;

    tracing::info!("Webhook server shut down gracefully");
    Ok(())
}
