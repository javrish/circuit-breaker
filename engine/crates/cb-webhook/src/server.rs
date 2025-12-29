//! Axum server setup and routing for the webhook server.
//!
//! This module provides the HTTP server configuration, middleware setup,
//! and route definitions for the Circuit Breaker webhook server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::config::WebhookConfig;
use crate::endpoints::{
    create_endpoint_handler, delete_endpoint_handler, get_endpoint_handler, get_event_handler,
    health_handler, list_endpoints_handler, list_events_handler, live_handler, ready_handler,
    replay_event_handler, update_endpoint_handler, webhook_handler,
};
use crate::error::Result;
use crate::metrics::init_metrics;
use crate::{AppState, ServerConfig};

/// Build the Axum router with all routes configured.
pub fn build_router(state: Arc<AppState>) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(3600));

    // Build the router
    Router::new()
        // Health check endpoints
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/live", get(live_handler))
        // Webhook endpoints - catch-all for dynamic paths
        .route("/webhooks/{*path}", post(webhook_handler))
        // Endpoint management API
        .route("/api/v1/endpoints", get(list_endpoints_handler))
        .route("/api/v1/endpoints", post(create_endpoint_handler))
        .route("/api/v1/endpoints/{id}", get(get_endpoint_handler))
        .route("/api/v1/endpoints/{id}", put(update_endpoint_handler))
        .route("/api/v1/endpoints/{id}", delete(delete_endpoint_handler))
        // Event management API
        .route("/api/v1/events", get(list_events_handler))
        .route("/api/v1/events/{id}", get(get_event_handler))
        .route("/api/v1/events/{id}/replay", post(replay_event_handler))
        // Add middleware
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
        // Add state
        .with_state(state)
}

/// Start the webhook server.
pub async fn serve(config: ServerConfig, state: AppState) -> Result<()> {
    // Initialize metrics
    init_metrics();

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Create shared state
    let mut state = state;
    state.shutdown_tx = Some(shutdown_tx.clone());
    state.config = config.clone();
    let state = Arc::new(state);

    // Load endpoint configuration if specified
    if let Some(ref config_path) = config.config_path {
        match load_endpoints_from_config(config_path, state.clone()).await {
            Ok(count) => {
                tracing::info!(count = count, path = %config_path, "Loaded endpoint configurations");
            }
            Err(e) => {
                tracing::warn!(error = %e, path = %config_path, "Failed to load endpoint configurations");
            }
        }
    }

    // Build the router
    let app = build_router(state.clone());

    // Create the socket address
    let addr = SocketAddr::new(config.host, config.port);

    tracing::info!(
        host = %config.host,
        port = %config.port,
        "Starting webhook server"
    );

    // Create the listener
    let listener = TcpListener::bind(addr).await?;

    tracing::info!(address = %addr, "Webhook server listening");

    // Start the metrics server if enabled
    let metrics_handle = if config.metrics_enabled {
        let metrics_addr = SocketAddr::new(config.host, config.metrics_port);
        Some(tokio::spawn(start_metrics_server(metrics_addr)))
    } else {
        None
    };

    // Run the server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
            tracing::info!("Shutdown signal received, stopping server");
        })
        .await?;

    // Wait for metrics server to stop
    if let Some(handle) = metrics_handle {
        handle.abort();
    }

    tracing::info!("Webhook server stopped");
    Ok(())
}

/// Load endpoints from configuration file or directory.
async fn load_endpoints_from_config(path: &str, state: Arc<AppState>) -> Result<usize> {
    let path = std::path::Path::new(path);

    let config = if path.is_dir() {
        WebhookConfig::from_directory(path)?
    } else {
        WebhookConfig::from_file(path)?
    };

    let endpoints = config.to_endpoints()?;
    let count = endpoints.len();

    for endpoint in endpoints {
        tracing::debug!(
            endpoint_id = %endpoint.id,
            path = %endpoint.path,
            "Registering endpoint from config"
        );
        state.register_endpoint(endpoint);
    }

    // Update metrics
    crate::metrics::set_active_endpoints(state.endpoints.len());

    Ok(count)
}

/// Start the Prometheus metrics server.
async fn start_metrics_server(addr: SocketAddr) {
    use metrics_exporter_prometheus::PrometheusBuilder;

    let builder = PrometheusBuilder::new();

    match builder.with_http_listener(addr).install() {
        Ok(_) => {
            tracing::info!(address = %addr, "Metrics server started");
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to start metrics server");
        }
    }

    // Keep the task running
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

/// Gracefully shutdown the server.
pub async fn shutdown(state: &AppState) {
    if let Some(ref tx) = state.shutdown_tx {
        let _ = tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn create_test_state() -> Arc<AppState> {
        Arc::new(AppState::default())
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_live_endpoint() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(Request::builder().uri("/live").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_endpoint() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Without NATS, ready should still be OK if no endpoints
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_endpoints_empty() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/endpoints")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_webhook_endpoint_not_found() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhooks/nonexistent")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_router_has_cors() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/health")
                    .header("Origin", "http://localhost:3000")
                    .header("Access-Control-Request-Method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // CORS preflight should succeed
        assert!(response.status().is_success() || response.status() == StatusCode::NO_CONTENT);
    }
}
