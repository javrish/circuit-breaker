//! # Circuit Breaker Webhook Server
//!
//! HTTP webhook server for ingesting external events into Circuit Breaker.
//!
//! This crate provides:
//! - Webhook endpoint registration and management
//! - Authentication (HMAC-SHA256, Bearer tokens, IP allowlists)
//! - Event normalization to CloudEvents format
//! - Rate limiting and payload validation
//! - NATS integration for event publishing
//!
//! ## Architecture
//!
//! The webhook server receives HTTP POST requests from external services
//! (GitHub, GitLab, Docker Hub, etc.), validates the request authenticity,
//! normalizes the event to CloudEvents format, and publishes it to NATS
//! for downstream processing by the trigger matcher.
//!
//! ```text
//! External Service → Webhook Server → NATS → Trigger Matcher → Workflow Engine
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
pub mod cloudevents;
pub mod config;
pub mod endpoints;
pub mod error;
pub mod metrics;
pub mod server;

use std::net::IpAddr;
use std::sync::Arc;

use async_nats::Client as NatsClient;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

pub use auth::{AuthConfig, AuthMethod, AuthValidator};
pub use cloudevents::CloudEvent;
pub use config::{EndpointConfig, WebhookConfig};
pub use error::{WebhookError, Result};
pub use server::serve;

/// Webhook server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Host to bind to.
    pub host: IpAddr,
    /// Port to listen on.
    pub port: u16,
    /// NATS server URL.
    pub nats_url: String,
    /// Path to endpoint configuration file.
    pub config_path: Option<String>,
    /// Enable metrics endpoint.
    pub metrics_enabled: bool,
    /// Metrics port.
    pub metrics_port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".parse().unwrap(),
            port: 8081,
            nats_url: "nats://localhost:4222".to_string(),
            config_path: None,
            metrics_enabled: true,
            metrics_port: 9091,
        }
    }
}

/// Registered webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpoint {
    /// Unique identifier for this endpoint.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Namespace for this endpoint.
    pub namespace: String,
    /// URL path for this endpoint (e.g., "/webhooks/github").
    pub path: String,
    /// Authentication configuration.
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    /// Event type mappings.
    #[serde(default)]
    pub triggers: Vec<TriggerMapping>,
    /// Maximum payload size in bytes.
    #[serde(default = "default_max_payload_bytes")]
    pub max_payload_bytes: usize,
    /// Rate limit configuration.
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
    /// IP allowlist (empty means allow all).
    #[serde(default)]
    pub ip_allowlist: Vec<String>,
    /// Whether endpoint is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last updated timestamp.
    pub updated_at: DateTime<Utc>,
}

fn default_max_payload_bytes() -> usize {
    1_048_576 // 1MB
}

fn default_enabled() -> bool {
    true
}

/// Trigger mapping configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerMapping {
    /// Event type to match (e.g., "push", "pull_request").
    pub event: String,
    /// Optional CEL filter expression.
    #[serde(default)]
    pub filter: Option<String>,
    /// Workflow to trigger.
    pub workflow: String,
    /// Workflow namespace (defaults to endpoint namespace).
    #[serde(default)]
    pub workflow_namespace: Option<String>,
    /// Input mappings (CEL expressions or template strings).
    #[serde(default)]
    pub inputs: std::collections::HashMap<String, String>,
}

/// Rate limit configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests allowed.
    pub requests: u32,
    /// Time period (e.g., "1m", "1h").
    pub period: String,
    /// Key for rate limiting (e.g., "source.ip", "event.repository").
    #[serde(default = "default_rate_limit_key")]
    pub key: String,
}

fn default_rate_limit_key() -> String {
    "source.ip".to_string()
}

/// Received webhook event record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// Unique event ID.
    pub id: Uuid,
    /// Endpoint that received this event.
    pub endpoint_id: String,
    /// Endpoint name.
    pub endpoint_name: String,
    /// Source type (e.g., "github", "gitlab").
    pub source_type: String,
    /// Event type from the source.
    pub event_type: String,
    /// Raw payload.
    pub payload: serde_json::Value,
    /// HTTP headers (selected).
    pub headers: std::collections::HashMap<String, String>,
    /// Source IP address.
    pub source_ip: String,
    /// Received timestamp.
    pub received_at: DateTime<Utc>,
    /// Whether authentication succeeded.
    pub auth_valid: bool,
    /// Processing status.
    pub status: EventStatus,
}

/// Event processing status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    /// Event received but not yet processed.
    Received,
    /// Event is being validated.
    Validating,
    /// Event passed validation.
    Validated,
    /// Event is being normalized.
    Normalizing,
    /// Event has been normalized to CloudEvent format.
    Normalized,
    /// Event has been published to NATS.
    Published,
    /// Event was filtered out (no matching trigger).
    Filtered,
    /// Event processing failed.
    Failed,
}

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    /// Registered webhook endpoints.
    pub endpoints: Arc<DashMap<String, WebhookEndpoint>>,
    /// Endpoint path to ID mapping.
    pub path_to_endpoint: Arc<DashMap<String, String>>,
    /// Recent events for debugging.
    pub recent_events: Arc<DashMap<Uuid, WebhookEvent>>,
    /// NATS client.
    pub nats: Option<NatsClient>,
    /// Shutdown signal sender.
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    /// Server configuration.
    pub config: ServerConfig,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            endpoints: Arc::new(DashMap::new()),
            path_to_endpoint: Arc::new(DashMap::new()),
            recent_events: Arc::new(DashMap::new()),
            nats: None,
            shutdown_tx: None,
            config: ServerConfig::default(),
        }
    }
}

impl AppState {
    /// Create new app state with NATS client.
    pub fn with_nats(nats: NatsClient, config: ServerConfig) -> Self {
        Self {
            endpoints: Arc::new(DashMap::new()),
            path_to_endpoint: Arc::new(DashMap::new()),
            recent_events: Arc::new(DashMap::new()),
            nats: Some(nats),
            shutdown_tx: None,
            config,
        }
    }

    /// Register a webhook endpoint.
    pub fn register_endpoint(&self, endpoint: WebhookEndpoint) {
        let path = endpoint.path.clone();
        let id = endpoint.id.clone();
        self.path_to_endpoint.insert(path, id.clone());
        self.endpoints.insert(id, endpoint);
    }

    /// Unregister a webhook endpoint.
    pub fn unregister_endpoint(&self, id: &str) -> Option<WebhookEndpoint> {
        if let Some((_, endpoint)) = self.endpoints.remove(id) {
            self.path_to_endpoint.remove(&endpoint.path);
            Some(endpoint)
        } else {
            None
        }
    }

    /// Get endpoint by ID.
    pub fn get_endpoint(&self, id: &str) -> Option<WebhookEndpoint> {
        self.endpoints.get(id).map(|e| e.clone())
    }

    /// Get endpoint by path.
    pub fn get_endpoint_by_path(&self, path: &str) -> Option<WebhookEndpoint> {
        self.path_to_endpoint
            .get(path)
            .and_then(|id| self.endpoints.get(id.value()).map(|e| e.clone()))
    }

    /// Store a received event.
    pub fn store_event(&self, event: WebhookEvent) {
        // Keep only the last 1000 events
        if self.recent_events.len() >= 1000 {
            // Remove oldest events
            let mut to_remove = Vec::new();
            for entry in self.recent_events.iter() {
                to_remove.push(*entry.key());
                if to_remove.len() >= 100 {
                    break;
                }
            }
            for id in to_remove {
                self.recent_events.remove(&id);
            }
        }
        self.recent_events.insert(event.id, event);
    }

    /// Update event status.
    pub fn update_event_status(&self, event_id: Uuid, status: EventStatus) {
        if let Some(mut event) = self.recent_events.get_mut(&event_id) {
            event.status = status;
        }
    }
}

/// Connect to NATS server.
pub async fn connect_nats(url: &str) -> Result<NatsClient> {
    tracing::info!(url = %url, "Connecting to NATS");

    let client = async_nats::connect(url)
        .await
        .map_err(|e| WebhookError::Nats(e.to_string()))?;

    tracing::info!("Connected to NATS successfully");
    Ok(client)
}

/// Initialize NATS streams for webhook events.
pub async fn init_nats_streams(client: &NatsClient) -> Result<()> {
    use async_nats::jetstream;

    let jetstream = jetstream::new(client.clone());

    // Create EXTERNAL_EVENTS stream if it doesn't exist
    let stream_config = jetstream::stream::Config {
        name: cb_nats::streams::EXTERNAL_EVENTS.to_string(),
        subjects: cb_nats::streams::external_events_subjects(),
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_messages: 1_000_000,
        max_bytes: 1024 * 1024 * 1024, // 1GB
        max_age: std::time::Duration::from_secs(7 * 24 * 60 * 60), // 7 days
        storage: jetstream::stream::StorageType::File,
        num_replicas: 1,
        ..Default::default()
    };

    match jetstream.get_stream(&stream_config.name).await {
        Ok(_) => {
            tracing::info!(stream = %stream_config.name, "Stream already exists");
        }
        Err(_) => {
            jetstream
                .create_stream(stream_config.clone())
                .await
                .map_err(|e| WebhookError::Nats(format!("Failed to create stream: {}", e)))?;
            tracing::info!(stream = %stream_config.name, "Created stream");
        }
    }

    // Create TRIGGERS stream if it doesn't exist
    let triggers_config = jetstream::stream::Config {
        name: cb_nats::streams::TRIGGERS.to_string(),
        subjects: cb_nats::streams::triggers_subjects(),
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_messages: 100_000,
        max_bytes: 256 * 1024 * 1024, // 256MB
        max_age: std::time::Duration::from_secs(24 * 60 * 60), // 1 day
        storage: jetstream::stream::StorageType::File,
        num_replicas: 1,
        ..Default::default()
    };

    match jetstream.get_stream(&triggers_config.name).await {
        Ok(_) => {
            tracing::info!(stream = %triggers_config.name, "Stream already exists");
        }
        Err(_) => {
            jetstream
                .create_stream(triggers_config.clone())
                .await
                .map_err(|e| WebhookError::Nats(format!("Failed to create stream: {}", e)))?;
            tracing::info!(stream = %triggers_config.name, "Created stream");
        }
    }

    Ok(())
}

/// Prelude for convenient imports.
pub mod prelude {
    pub use crate::{
        AppState, AuthConfig, AuthMethod, CloudEvent, EndpointConfig, EventStatus,
        RateLimitConfig, Result, ServerConfig, TriggerMapping, WebhookEndpoint,
        WebhookError, WebhookEvent,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_server_config() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8081);
        assert!(config.metrics_enabled);
    }

    #[test]
    fn test_app_state_endpoint_registration() {
        let state = AppState::default();

        let endpoint = WebhookEndpoint {
            id: "test-1".to_string(),
            name: "Test Endpoint".to_string(),
            namespace: "default".to_string(),
            path: "/webhooks/test".to_string(),
            auth: None,
            triggers: vec![],
            max_payload_bytes: 1_048_576,
            rate_limit: None,
            ip_allowlist: vec![],
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        state.register_endpoint(endpoint.clone());

        assert!(state.get_endpoint("test-1").is_some());
        assert!(state.get_endpoint_by_path("/webhooks/test").is_some());

        let removed = state.unregister_endpoint("test-1");
        assert!(removed.is_some());
        assert!(state.get_endpoint("test-1").is_none());
    }

    #[test]
    fn test_event_status_serialization() {
        let status = EventStatus::Published;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"published\"");
    }
}
