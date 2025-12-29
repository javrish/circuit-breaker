//! # Circuit Breaker NATS Integration
//!
//! This crate provides NATS JetStream integration for the Circuit Breaker
//! workflow engine, enabling event-driven communication between services.
//!
//! ## Features
//!
//! - JetStream stream and consumer management
//! - Event publishing and subscribing
//! - KV store for workflow state
//! - Object store for artifacts
//! - Durable subscriptions for reliable delivery
//!
//! ## Architecture
//!
//! The NATS integration uses JetStream for:
//! - **Streams**: Durable event logs for workflow and run events
//! - **KV Buckets**: Fast state storage for markings and metadata
//! - **Object Store**: Large artifact storage (logs, build outputs)
//!
//! ## Streams
//!
//! - `CB_WORKFLOWS`: Workflow lifecycle events (submitted, started, completed, failed)
//! - `CB_RUNS`: Run execution events (transitions, marking updates)
//! - `CB_SYSTEM`: System events (runner heartbeats, metrics)
//!
//! ## Subject Hierarchy
//!
//! ```text
//! cb.
//! ├── workflows.{namespace}.submitted
//! ├── workflows.{namespace}.{workflow_id}.*
//! ├── runs.{run_id}.status
//! ├── runs.{run_id}.marking
//! ├── runs.{run_id}.transitions.{transition_id}.*
//! └── system.*
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use thiserror::Error;

/// NATS-specific error type.
#[derive(Debug, Error)]
pub enum NatsError {
    /// Connection error.
    #[error("NATS connection error: {0}")]
    Connection(String),

    /// JetStream error.
    #[error("JetStream error: {0}")]
    JetStream(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Timeout error.
    #[error("Operation timed out")]
    Timeout,

    /// Stream not found.
    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    /// Consumer not found.
    #[error("Consumer not found: {0}")]
    ConsumerNotFound(String),
}

/// Result type for NATS operations.
pub type Result<T> = std::result::Result<T, NatsError>;

/// Configuration for NATS client.
#[derive(Debug, Clone)]
pub struct NatsConfig {
    /// NATS server URL(s).
    pub urls: Vec<String>,
    /// Optional credentials file path.
    pub credentials_path: Option<String>,
    /// Connection name for identification.
    pub name: Option<String>,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: u64,
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            urls: vec!["nats://localhost:4222".to_string()],
            credentials_path: None,
            name: Some("circuit-breaker".to_string()),
            connect_timeout_secs: 10,
            request_timeout_secs: 30,
        }
    }
}

/// NATS client wrapper with JetStream support.
pub struct NatsClient {
    /// The underlying NATS connection.
    client: async_nats::Client,
    /// JetStream context.
    jetstream: async_nats::jetstream::Context,
}

impl NatsClient {
    /// Connect to NATS with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(config: &NatsConfig) -> Result<Self> {
        let mut options = async_nats::ConnectOptions::new();

        if let Some(ref name) = config.name {
            options = options.name(name);
        }

        let urls = config.urls.join(",");
        let client = options
            .connect(&urls)
            .await
            .map_err(|e| NatsError::Connection(e.to_string()))?;

        let jetstream = async_nats::jetstream::new(client.clone());

        Ok(Self { client, jetstream })
    }

    /// Get a reference to the underlying NATS client.
    #[must_use]
    pub fn client(&self) -> &async_nats::Client {
        &self.client
    }

    /// Get a reference to the JetStream context.
    #[must_use]
    pub fn jetstream(&self) -> &async_nats::jetstream::Context {
        &self.jetstream
    }

    /// Publish an event to a subject.
    ///
    /// # Errors
    ///
    /// Returns an error if publishing fails.
    pub async fn publish<T: serde::Serialize>(&self, subject: &str, event: &T) -> Result<()> {
        let payload = serde_json::to_vec(event)?;
        self.client
            .publish(subject.to_string(), payload.into())
            .await
            .map_err(|e| NatsError::Connection(e.to_string()))?;
        Ok(())
    }

    /// Publish an event to JetStream.
    ///
    /// # Errors
    ///
    /// Returns an error if publishing fails.
    pub async fn publish_jetstream<T: serde::Serialize>(
        &self,
        subject: &str,
        event: &T,
    ) -> Result<async_nats::jetstream::context::PublishAckFuture> {
        let payload = serde_json::to_vec(event)?;
        self.jetstream
            .publish(subject.to_string(), payload.into())
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))
    }
}

/// Event publisher for Circuit Breaker events.
pub struct EventPublisher {
    client: NatsClient,
}

impl EventPublisher {
    /// Create a new event publisher.
    #[must_use]
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }

    /// Publish a workflow event.
    ///
    /// # Errors
    ///
    /// Returns an error if publishing fails.
    pub async fn publish_workflow_event<T: serde::Serialize>(
        &self,
        workflow_id: &uuid::Uuid,
        event_type: &str,
        event: &T,
    ) -> Result<()> {
        let subject = format!("workflow.{}.{}", workflow_id, event_type);
        self.client.publish(&subject, event).await
    }

    /// Publish a run event.
    ///
    /// # Errors
    ///
    /// Returns an error if publishing fails.
    pub async fn publish_run_event<T: serde::Serialize>(
        &self,
        run_id: &uuid::Uuid,
        event_type: &str,
        event: &T,
    ) -> Result<()> {
        let subject = format!("run.{}.{}", run_id, event_type);
        self.client.publish(&subject, event).await
    }

    /// Publish a task event.
    ///
    /// # Errors
    ///
    /// Returns an error if publishing fails.
    pub async fn publish_task_event<T: serde::Serialize>(
        &self,
        task_id: &uuid::Uuid,
        event_type: &str,
        event: &T,
    ) -> Result<()> {
        let subject = format!("task.{}.{}", task_id, event_type);
        self.client.publish(&subject, event).await
    }
}

/// Stream names used by Circuit Breaker.
pub mod streams {
    /// Stream for workflow lifecycle events.
    pub const WORKFLOWS: &str = "CB_WORKFLOWS";
    /// Stream for run execution events.
    pub const RUNS: &str = "CB_RUNS";
    /// Stream for system events (heartbeats, metrics).
    pub const SYSTEM: &str = "CB_SYSTEM";
    /// Stream for external events (webhooks, Kafka, cloud events).
    pub const EXTERNAL_EVENTS: &str = "CB_EXTERNAL_EVENTS";
    /// Stream for trigger processing events.
    pub const TRIGGERS: &str = "CB_TRIGGERS";

    /// Stream configuration for WORKFLOWS stream.
    pub fn workflows_subjects() -> Vec<String> {
        vec!["cb.workflows.>".to_string()]
    }

    /// Stream configuration for RUNS stream.
    pub fn runs_subjects() -> Vec<String> {
        vec!["cb.runs.>".to_string()]
    }

    /// Stream configuration for SYSTEM stream.
    pub fn system_subjects() -> Vec<String> {
        vec!["cb.system.>".to_string()]
    }

    /// Stream configuration for EXTERNAL_EVENTS stream.
    pub fn external_events_subjects() -> Vec<String> {
        vec!["cb.external.>".to_string()]
    }

    /// Stream configuration for TRIGGERS stream.
    pub fn triggers_subjects() -> Vec<String> {
        vec!["cb.triggers.>".to_string()]
    }
}

/// NATS subject builders for Circuit Breaker events.
pub mod subjects {
    use uuid::Uuid;

    /// Base prefix for all Circuit Breaker subjects.
    pub const PREFIX: &str = "cb";

    // ==================== Workflow Subjects ====================

    /// Subject for workflow submitted events.
    /// Pattern: `cb.workflows.{namespace}.submitted`
    pub fn workflow_submitted(namespace: &str) -> String {
        format!("{}.workflows.{}.submitted", PREFIX, namespace)
    }

    /// Subject for workflow started events.
    /// Pattern: `cb.workflows.{namespace}.{workflow_id}.started`
    pub fn workflow_started(namespace: &str, workflow_id: &Uuid) -> String {
        format!("{}.workflows.{}.{}.started", PREFIX, namespace, workflow_id)
    }

    /// Subject for workflow completed events.
    /// Pattern: `cb.workflows.{namespace}.{workflow_id}.completed`
    pub fn workflow_completed(namespace: &str, workflow_id: &Uuid) -> String {
        format!(
            "{}.workflows.{}.{}.completed",
            PREFIX, namespace, workflow_id
        )
    }

    /// Subject for workflow failed events.
    /// Pattern: `cb.workflows.{namespace}.{workflow_id}.failed`
    pub fn workflow_failed(namespace: &str, workflow_id: &Uuid) -> String {
        format!("{}.workflows.{}.{}.failed", PREFIX, namespace, workflow_id)
    }

    /// Subject for workflow cancelled events.
    /// Pattern: `cb.workflows.{namespace}.{workflow_id}.cancelled`
    pub fn workflow_cancelled(namespace: &str, workflow_id: &Uuid) -> String {
        format!(
            "{}.workflows.{}.{}.cancelled",
            PREFIX, namespace, workflow_id
        )
    }

    // ==================== Run Subjects ====================

    /// Subject for run status updates.
    /// Pattern: `cb.runs.{run_id}.status`
    pub fn run_status(run_id: &Uuid) -> String {
        format!("{}.runs.{}.status", PREFIX, run_id)
    }

    /// Subject for marking updates.
    /// Pattern: `cb.runs.{run_id}.marking`
    pub fn run_marking(run_id: &Uuid) -> String {
        format!("{}.runs.{}.marking", PREFIX, run_id)
    }

    // ==================== Transition Subjects ====================

    /// Subject for transition enabled events.
    /// Pattern: `cb.runs.{run_id}.transitions.{transition_id}.enabled`
    pub fn transition_enabled(run_id: &Uuid, transition_id: &str) -> String {
        format!(
            "{}.runs.{}.transitions.{}.enabled",
            PREFIX, run_id, transition_id
        )
    }

    /// Subject for transition fired events.
    /// Pattern: `cb.runs.{run_id}.transitions.{transition_id}.fired`
    pub fn transition_fired(run_id: &Uuid, transition_id: &str) -> String {
        format!(
            "{}.runs.{}.transitions.{}.fired",
            PREFIX, run_id, transition_id
        )
    }

    /// Subject for transition completed events.
    /// Pattern: `cb.runs.{run_id}.transitions.{transition_id}.completed`
    pub fn transition_completed(run_id: &Uuid, transition_id: &str) -> String {
        format!(
            "{}.runs.{}.transitions.{}.completed",
            PREFIX, run_id, transition_id
        )
    }

    /// Subject for transition failed events.
    /// Pattern: `cb.runs.{run_id}.transitions.{transition_id}.failed`
    pub fn transition_failed(run_id: &Uuid, transition_id: &str) -> String {
        format!(
            "{}.runs.{}.transitions.{}.failed",
            PREFIX, run_id, transition_id
        )
    }

    // ==================== System Subjects ====================

    /// Subject for runner heartbeats.
    /// Pattern: `cb.system.runners.{runner_id}.heartbeat`
    pub fn runner_heartbeat(runner_id: &str) -> String {
        format!("{}.system.runners.{}.heartbeat", PREFIX, runner_id)
    }

    /// Subject for scheduler assignments.
    /// Pattern: `cb.system.scheduler.assignments`
    pub fn scheduler_assignments() -> String {
        format!("{}.system.scheduler.assignments", PREFIX)
    }

    /// Subject for system metrics.
    /// Pattern: `cb.system.metrics`
    pub fn system_metrics() -> String {
        format!("{}.system.metrics", PREFIX)
    }

    // ==================== External Event Subjects ====================

    /// Subject for raw webhook events received.
    /// Pattern: `cb.external.webhook.{endpoint}.received`
    pub fn webhook_received(endpoint: &str) -> String {
        format!("{}.external.webhook.{}.received", PREFIX, endpoint)
    }

    /// Subject for raw Kafka events received.
    /// Pattern: `cb.external.kafka.{topic}.received`
    pub fn kafka_received(topic: &str) -> String {
        format!("{}.external.kafka.{}.received", PREFIX, topic)
    }

    /// Subject for raw AWS events received.
    /// Pattern: `cb.external.aws.{source}.received`
    pub fn aws_received(source: &str) -> String {
        format!("{}.external.aws.{}.received", PREFIX, source)
    }

    /// Subject for raw GCP Pub/Sub events received.
    /// Pattern: `cb.external.gcp.{subscription}.received`
    pub fn gcp_received(subscription: &str) -> String {
        format!("{}.external.gcp.{}.received", PREFIX, subscription)
    }

    /// Subject for normalized CloudEvents.
    /// Pattern: `cb.external.normalized`
    pub fn external_normalized() -> String {
        format!("{}.external.normalized", PREFIX)
    }

    // ==================== Trigger Subjects ====================

    /// Subject for events that matched a trigger.
    /// Pattern: `cb.triggers.{trigger_name}.matched`
    pub fn trigger_matched(trigger_name: &str) -> String {
        format!("{}.triggers.{}.matched", PREFIX, trigger_name)
    }

    /// Subject for events that were filtered out.
    /// Pattern: `cb.triggers.filtered`
    pub fn trigger_filtered() -> String {
        format!("{}.triggers.filtered", PREFIX)
    }

    /// Subject for trigger processing errors.
    /// Pattern: `cb.triggers.errors`
    pub fn trigger_errors() -> String {
        format!("{}.triggers.errors", PREFIX)
    }

    // ==================== Subscription Patterns ====================

    /// Subscribe to all workflow events in a namespace.
    /// Pattern: `cb.workflows.{namespace}.>`
    pub fn all_workflow_events(namespace: &str) -> String {
        format!("{}.workflows.{}.>", PREFIX, namespace)
    }

    /// Subscribe to all events for a specific workflow.
    /// Pattern: `cb.workflows.{namespace}.{workflow_id}.>`
    pub fn workflow_events(namespace: &str, workflow_id: &Uuid) -> String {
        format!("{}.workflows.{}.{}.>", PREFIX, namespace, workflow_id)
    }

    /// Subscribe to all events for a specific run.
    /// Pattern: `cb.runs.{run_id}.>`
    pub fn all_run_events(run_id: &Uuid) -> String {
        format!("{}.runs.{}.>", PREFIX, run_id)
    }

    /// Subscribe to all transition events for a run.
    /// Pattern: `cb.runs.{run_id}.transitions.>`
    pub fn all_transition_events(run_id: &Uuid) -> String {
        format!("{}.runs.{}.transitions.>", PREFIX, run_id)
    }

    /// Subscribe to all enabled transitions (for runners).
    /// Pattern: `cb.runs.*.transitions.*.enabled`
    pub fn all_enabled_transitions() -> String {
        format!("{}.runs.*.transitions.*.enabled", PREFIX)
    }

    /// Subscribe to all marking updates (for scheduler).
    /// Pattern: `cb.runs.*.marking`
    pub fn all_marking_updates() -> String {
        format!("{}.runs.*.marking", PREFIX)
    }

    /// Subscribe to all external events (for trigger matcher).
    /// Pattern: `cb.external.>`
    pub fn all_external_events() -> String {
        format!("{}.external.>", PREFIX)
    }

    /// Subscribe to all normalized external events.
    /// Pattern: `cb.external.normalized`
    pub fn all_normalized_events() -> String {
        format!("{}.external.normalized", PREFIX)
    }

    /// Subscribe to all trigger matches.
    /// Pattern: `cb.triggers.*.matched`
    pub fn all_trigger_matches() -> String {
        format!("{}.triggers.*.matched", PREFIX)
    }
}

/// Consumer names used by Circuit Breaker components.
pub mod consumers {
    /// Controller consumer - processes workflow lifecycle events.
    pub const CONTROLLER: &str = "cb-controller";
    /// Scheduler consumer - processes marking updates.
    pub const SCHEDULER: &str = "cb-scheduler";
    /// Runner consumer group - processes enabled transitions.
    pub const RUNNER_GROUP: &str = "cb-runner-group";
    /// Webhook server consumer - processes raw webhook events.
    pub const WEBHOOK_SERVER: &str = "cb-webhook";
    /// Event normalizer consumer - normalizes external events.
    pub const NORMALIZER: &str = "cb-normalizer";
    /// Trigger matcher consumer - matches events to triggers.
    pub const TRIGGER_MATCHER: &str = "cb-trigger-matcher";
}

/// KV bucket names used by Circuit Breaker.
pub mod kv_buckets {
    /// Bucket for workflow definitions.
    pub const WORKFLOW_DEFINITIONS: &str = "cb_workflow_definitions";
    /// Bucket for workflow state (current marking).
    pub const WORKFLOW_STATE: &str = "cb_workflow_state";
    /// Bucket for run metadata.
    pub const RUN_METADATA: &str = "cb_run_metadata";
    /// Bucket for transition claims (locking).
    pub const TRANSITION_CLAIMS: &str = "cb_transition_claims";
}

/// Object store bucket names.
pub mod object_stores {
    /// Store for build artifacts.
    pub const ARTIFACTS: &str = "cb_artifacts";
    /// Store for execution logs.
    pub const LOGS: &str = "cb_logs";
}

/// Event envelope for all Circuit Breaker events.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope<T> {
    /// Unique event ID.
    pub event_id: uuid::Uuid,
    /// Event type name.
    pub event_type: String,
    /// Timestamp of event creation.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Event payload.
    pub data: T,
    /// Optional trace context for distributed tracing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_context: Option<TraceContext>,
}

impl<T> EventEnvelope<T> {
    /// Create a new event envelope.
    pub fn new(event_type: impl Into<String>, data: T) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4(),
            event_type: event_type.into(),
            timestamp: chrono::Utc::now(),
            data,
            trace_context: None,
        }
    }

    /// Add trace context to the event.
    #[must_use]
    pub fn with_trace(mut self, trace: TraceContext) -> Self {
        self.trace_context = Some(trace);
        self
    }
}

/// Distributed tracing context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceContext {
    /// Trace ID.
    pub trace_id: String,
    /// Span ID.
    pub span_id: String,
    /// Parent span ID (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
}

/// Prelude for convenient imports.
pub mod prelude {
    pub use crate::{
        consumers, kv_buckets, object_stores, streams, subjects, EventEnvelope, EventPublisher,
        NatsClient, NatsConfig, NatsError, Result, TraceContext,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_names() {
        assert!(streams::WORKFLOWS.starts_with("CB_"));
        assert!(streams::RUNS.starts_with("CB_"));
        assert!(streams::SYSTEM.starts_with("CB_"));
    }

    #[test]
    fn test_kv_bucket_names() {
        assert!(kv_buckets::WORKFLOW_DEFINITIONS.starts_with("cb_"));
        assert!(kv_buckets::WORKFLOW_STATE.starts_with("cb_"));
    }

    #[test]
    fn test_default_config() {
        let config = NatsConfig::default();
        assert_eq!(config.urls, vec!["nats://localhost:4222".to_string()]);
        assert_eq!(config.connect_timeout_secs, 10);
    }

    #[test]
    fn test_workflow_subjects() {
        let ns = "production";
        let wf_id = uuid::Uuid::new_v4();

        assert_eq!(
            subjects::workflow_submitted(ns),
            format!("cb.workflows.{}.submitted", ns)
        );
        assert_eq!(
            subjects::workflow_started(ns, &wf_id),
            format!("cb.workflows.{}.{}.started", ns, wf_id)
        );
    }

    #[test]
    fn test_transition_subjects() {
        let run_id = uuid::Uuid::new_v4();
        let transition_id = "build";

        assert_eq!(
            subjects::transition_enabled(&run_id, transition_id),
            format!("cb.runs.{}.transitions.{}.enabled", run_id, transition_id)
        );
        assert_eq!(
            subjects::transition_completed(&run_id, transition_id),
            format!("cb.runs.{}.transitions.{}.completed", run_id, transition_id)
        );
    }

    #[test]
    fn test_subscription_patterns() {
        assert_eq!(
            subjects::all_enabled_transitions(),
            "cb.runs.*.transitions.*.enabled"
        );
        assert_eq!(subjects::all_marking_updates(), "cb.runs.*.marking");
    }

    #[test]
    fn test_event_envelope() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct TestData {
            message: String,
        }

        let envelope = EventEnvelope::new(
            "TestEvent",
            TestData {
                message: "hello".to_string(),
            },
        );

        assert_eq!(envelope.event_type, "TestEvent");
        assert_eq!(envelope.data.message, "hello");
        assert!(envelope.trace_context.is_none());
    }
}
