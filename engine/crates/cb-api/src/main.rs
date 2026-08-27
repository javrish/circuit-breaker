//! Circuit Breaker API Server
//!
//! HTTP API server for workflow management and execution.

use cb_api::{connect_nats, init_nats_streams, serve, ApiConfig, AppState};
use clap::Parser;
use std::net::IpAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Circuit Breaker API Server
#[derive(Parser, Debug)]
#[command(name = "cb-api")]
#[command(about = "Circuit Breaker API Server", long_about = None)]
struct Args {
    /// Host to bind to
    #[arg(long, env = "CB_API_HOST", default_value = "0.0.0.0")]
    host: IpAddr,

    /// Port to listen on
    #[arg(short, long, env = "CB_API_PORT", default_value_t = 9000)]
    port: u16,

    /// NATS server URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    /// Log level
    #[arg(long, env = "CB_LOG_LEVEL", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("cb_api={},tower_http=debug", args.log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        host = %args.host,
        port = %args.port,
        nats_url = %args.nats_url,
        "Starting Circuit Breaker API server"
    );

    // Connect to NATS
    let nats = match connect_nats(&args.nats_url).await {
        Ok(client) => {
            // Initialize streams
            if let Err(e) = init_nats_streams(&client).await {
                tracing::warn!(error = %e, "Failed to initialize NATS streams");
            }
            Some(client)
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to connect to NATS - running in standalone mode"
            );
            None
        }
    };

    // Create configuration
    let config = ApiConfig {
        host: args.host,
        port: args.port,
        nats_url: args.nats_url,
    };

    // Create application state
    let state = match nats {
        Some(client) => AppState::with_nats(client),
        None => AppState::default(),
    };

    // Start server
    serve(config, state).await?;

    Ok(())
}
