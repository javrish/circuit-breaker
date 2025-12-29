//! Event types for the NATS event-driven architecture.
//!
//! These types match the JSON Schema contract in `/schemas/events.schema.json`
//! and are used for communication between services via NATS JetStream.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Schema version for events.
pub const EVENT_VERSION: &str = "1.0";

/// All possible event types in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    // Workflow lifecycle events
    #[serde(rename = "workflow.submitted")]
    WorkflowSubmitted,
    #[serde(rename = "workflow.started")]
    WorkflowStarted,
    #[serde(rename = "workflow.completed")]
    WorkflowCompleted,
    #[serde(rename = "workflow.failed")]
    WorkflowFailed,
    #[serde(rename = "workflow.cancelled")]
    WorkflowCancelled,

    // Transition lifecycle events
    #[serde(rename = "transition.enabled")]
    TransitionEnabled,
    #[serde(rename = "transition.fired")]
    TransitionFired,
    #[serde(rename = "transition.completed")]
    TransitionCompleted,
    #[serde(rename = "transition.failed")]
    TransitionFailed,
    #[serde(rename = "transition.retrying")]
    TransitionRetrying,

    // Token events
    #[serde(rename = "token.produced")]
    TokenProduced,
    #[serde(rename = "token.consumed")]
    TokenConsumed,

    // Task events
    #[serde(rename = "task.dispatched")]
    TaskDispatched,
    #[serde(rename = "task.completed")]
    TaskCompleted,
    #[serde(rename = "task.failed")]
    TaskFailed,
}

impl EventType {
    /// Get the NATS subject for this event type.
    #[must_use]
    pub fn subject(&self) -> &'static str {
        match self {
            Self::WorkflowSubmitted => "workflow.submitted",
            Self::WorkflowStarted => "workflow.started",
            Self::WorkflowCompleted => "workflow.completed",
            Self::WorkflowFailed => "workflow.failed",
            Self::WorkflowCancelled => "workflow.cancelled",
            Self::TransitionEnabled => "transition.enabled",
            Self::TransitionFired => "transition.fired",
            Self::TransitionCompleted => "transition.completed",
            Self::TransitionFailed => "transition.failed",
            Self::TransitionRetrying => "transition.retrying",
            Self::TokenProduced => "token.produced",
            Self::TokenConsumed => "token.consumed",
            Self::TaskDispatched => "task.dispatched",
            Self::TaskCompleted => "task.completed",
            Self::TaskFailed => "task.failed",
        }
    }

    /// Check if this is a workflow event.
    #[must_use]
    pub fn is_workflow_event(&self) -> bool {
        matches!(
            self,
            Self::WorkflowSubmitted
                | Self::WorkflowStarted
                | Self::WorkflowCompleted
                | Self::WorkflowFailed
                | Self::WorkflowCancelled
        )
    }

    /// Check if this is a transition event.
    #[must_use]
    pub fn is_transition_event(&self) -> bool {
        matches!(
            self,
            Self::TransitionEnabled
                | Self::TransitionFired
                | Self::TransitionCompleted
                | Self::TransitionFailed
                | Self::TransitionRetrying
        )
    }

    /// Check if this is a token event.
    #[must_use]
    pub fn is_token_event(&self) -> bool {
        matches!(self, Self::TokenProduced | Self::TokenConsumed)
    }

    /// Check if this is a task event.
    #[must_use]
    pub fn is_task_event(&self) -> bool {
        matches!(
            self,
            Self::TaskDispatched | Self::TaskCompleted | Self::TaskFailed
        )
    }

    /// Check if this is a terminal event (workflow ended).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::WorkflowCompleted | Self::WorkflowFailed | Self::WorkflowCancelled
        )
    }
}

/// Metadata attached to every event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EventMetadata {
    /// Unique identifier for this event.
    pub event_id: Uuid,

    /// Timestamp when event was created.
    pub timestamp: DateTime<Utc>,

    /// Event schema version.
    pub version: String,

    /// ID to correlate related events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<Uuid>,

    /// ID of the event that caused this event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<Uuid>,

    /// Service that emitted this event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl EventMetadata {
    /// Create new event metadata with a random ID and current timestamp.
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            version: EVENT_VERSION.to_string(),
            correlation_id: None,
            causation_id: None,
            source: None,
        }
    }

    /// Create new event metadata with a correlation ID.
    #[must_use]
    pub fn with_correlation(correlation_id: Uuid) -> Self {
        Self {
            correlation_id: Some(correlation_id),
            ..Self::new()
        }
    }

    /// Create new event metadata caused by another event.
    #[must_use]
    pub fn caused_by(cause: &EventMetadata) -> Self {
        Self {
            correlation_id: cause.correlation_id.or(Some(cause.event_id)),
            causation_id: Some(cause.event_id),
            ..Self::new()
        }
    }

    /// Set the source service.
    #[must_use]
    pub fn from_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

impl Default for EventMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Reference to a workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRef {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

fn default_namespace() -> String {
    "default".to_string()
}

/// Reference to a workflow run (execution instance).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunRef {
    pub run_id: Uuid,
    pub workflow_id: Uuid,
    #[serde(default = "default_attempt")]
    pub attempt: u32,
}

fn default_attempt() -> u32 {
    1
}

/// Reference to a transition execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionRef {
    pub transition_id: String,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<Uuid>,
}

/// Token data for colored Petri nets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenData {
    pub token_id: Uuid,
    pub place_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl TokenData {
    /// Create a new token with a random ID.
    #[must_use]
    pub fn new(place_id: impl Into<String>) -> Self {
        Self {
            token_id: Uuid::new_v4(),
            place_id: place_id.into(),
            data: None,
            created_at: Utc::now(),
        }
    }

    /// Create a new token with data (colored token).
    #[must_use]
    pub fn with_data(place_id: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            token_id: Uuid::new_v4(),
            place_id: place_id.into(),
            data: Some(data),
            created_at: Utc::now(),
        }
    }
}

/// Error information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInfo {
    /// Error code for programmatic handling.
    pub code: String,

    /// Human-readable error message.
    pub message: String,

    /// Additional error context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,

    /// Stack trace if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,

    /// Whether this error is retryable.
    #[serde(default)]
    pub retryable: bool,
}

impl ErrorInfo {
    /// Create a new error.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            stack_trace: None,
            retryable: false,
        }
    }

    /// Create a retryable error.
    #[must_use]
    pub fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            retryable: true,
            ..Self::new(code, message)
        }
    }

    /// Add details to the error.
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Resource usage metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    /// CPU time in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_millis: Option<u64>,

    /// Peak memory usage in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,

    /// Wall clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

// ============ Event Payloads ============

/// Payload for workflow.submitted event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSubmittedPayload {
    pub workflow: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
}

/// Payload for workflow.started event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStartedPayload {
    pub workflow_ref: WorkflowRef,
    pub run_ref: RunRef,
    pub initial_marking: HashMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<TriggerType>,
}

/// How a workflow was triggered.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    Manual,
    Schedule,
    Webhook,
    Event,
    Api,
}

/// Payload for workflow.completed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCompletedPayload {
    pub run_ref: RunRef,
    pub final_marking: HashMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_usage: Option<ResourceUsage>,
}

/// Payload for workflow.failed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowFailedPayload {
    pub run_ref: RunRef,
    pub error: ErrorInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_transition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marking: Option<HashMap<String, u32>>,
}

/// Payload for workflow.cancelled event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCancelledPayload {
    pub run_ref: RunRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marking: Option<HashMap<String, u32>>,
}

/// Payload for transition.enabled event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionEnabledPayload {
    pub transition_ref: TransitionRef,
    pub input_tokens: Vec<TokenData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_result: Option<bool>,
}

/// Payload for transition.fired event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionFiredPayload {
    pub transition_ref: TransitionRef,
    pub consumed_tokens: Vec<TokenData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_id: Option<String>,
}

/// Payload for transition.completed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionCompletedPayload {
    pub transition_ref: TransitionRef,
    pub produced_tokens: Vec<TokenData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_usage: Option<ResourceUsage>,
}

/// Payload for transition.failed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionFailedPayload {
    pub transition_ref: TransitionRef,
    pub error: ErrorInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub will_retry: bool,
}

/// Payload for transition.retrying event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionRetryingPayload {
    pub transition_ref: TransitionRef,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    pub next_attempt_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<ErrorInfo>,
}

/// Payload for token.produced event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenProducedPayload {
    pub run_ref: RunRef,
    pub token: TokenData,
    pub produced_by: String,
}

/// Payload for token.consumed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenConsumedPayload {
    pub run_ref: RunRef,
    pub token: TokenData,
    pub consumed_by: String,
}

/// Payload for token.injected event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenInjectedPayload {
    pub run_ref: RunRef,
    pub token: TokenData,
    pub place_id: String,
    /// Who/what injected the token (e.g., "api", "cli", user ID)
    pub injected_by: String,
    /// Reason for injection (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for task.dispatched event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskDispatchedPayload {
    pub task_id: Uuid,
    pub transition_ref: TransitionRef,
    pub action: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<TaskResources>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default = "default_runner_pool")]
    pub runner_pool: String,
}

fn default_runner_pool() -> String {
    "default".to_string()
}

/// Resource requirements for a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskResources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
}

/// Artifact produced by a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// Payload for task.completed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskCompletedPayload {
    pub task_id: Uuid,
    pub transition_ref: TransitionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_usage: Option<ResourceUsage>,
}

/// Payload for task.failed event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskFailedPayload {
    pub task_id: Uuid,
    pub transition_ref: TransitionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_id: Option<String>,
    pub error: ErrorInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs: Option<String>,
}

// ============ Generic Event Wrapper ============

/// A generic event with type, metadata, and payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event<P> {
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub metadata: EventMetadata,
    pub payload: P,
}

impl<P> Event<P> {
    /// Create a new event.
    pub fn new(event_type: EventType, payload: P) -> Self {
        Self {
            event_type,
            metadata: EventMetadata::new(),
            payload,
        }
    }

    /// Create a new event with custom metadata.
    pub fn with_metadata(event_type: EventType, metadata: EventMetadata, payload: P) -> Self {
        Self {
            event_type,
            metadata,
            payload,
        }
    }

    /// Create an event caused by another event.
    pub fn caused_by<Q>(event_type: EventType, cause: &Event<Q>, payload: P) -> Self {
        Self {
            event_type,
            metadata: EventMetadata::caused_by(&cause.metadata),
            payload,
        }
    }
}

// ============ Convenience Type Aliases ============

pub type WorkflowSubmittedEvent = Event<WorkflowSubmittedPayload>;
pub type WorkflowStartedEvent = Event<WorkflowStartedPayload>;
pub type WorkflowCompletedEvent = Event<WorkflowCompletedPayload>;
pub type WorkflowFailedEvent = Event<WorkflowFailedPayload>;
pub type WorkflowCancelledEvent = Event<WorkflowCancelledPayload>;

pub type TransitionEnabledEvent = Event<TransitionEnabledPayload>;
pub type TransitionFiredEvent = Event<TransitionFiredPayload>;
pub type TransitionCompletedEvent = Event<TransitionCompletedPayload>;
pub type TransitionFailedEvent = Event<TransitionFailedPayload>;
pub type TransitionRetryingEvent = Event<TransitionRetryingPayload>;

pub type TokenProducedEvent = Event<TokenProducedPayload>;
pub type TokenConsumedEvent = Event<TokenConsumedPayload>;
pub type TokenInjectedEvent = Event<TokenInjectedPayload>;

pub type TaskDispatchedEvent = Event<TaskDispatchedPayload>;
pub type TaskCompletedEvent = Event<TaskCompletedPayload>;
pub type TaskFailedEvent = Event<TaskFailedPayload>;

// ============ NATS Subject Helpers ============

/// NATS subject patterns for subscribing to events.
pub mod subjects {
    /// All workflow events.
    pub const WORKFLOWS: &str = "workflow.>";

    /// Workflow events for a specific workflow.
    pub fn workflow(workflow_id: &uuid::Uuid) -> String {
        format!("workflow.{workflow_id}.>")
    }

    /// All run events.
    pub const RUNS: &str = "run.>";

    /// Run events for a specific run.
    pub fn run(run_id: &uuid::Uuid) -> String {
        format!("run.{run_id}.>")
    }

    /// All task dispatch events (for runners).
    pub const TASK_DISPATCH: &str = "task.dispatch.>";

    /// Task dispatch for a specific runner pool.
    pub fn task_dispatch_pool(pool: &str) -> String {
        format!("task.dispatch.{pool}")
    }

    /// All task result events.
    pub const TASK_RESULTS: &str = "task.result.>";

    /// Task result for a specific task.
    pub fn task_result(task_id: &uuid::Uuid) -> String {
        format!("task.result.{task_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_subjects() {
        assert_eq!(EventType::WorkflowStarted.subject(), "workflow.started");
        assert_eq!(EventType::TransitionFired.subject(), "transition.fired");
        assert_eq!(EventType::TaskCompleted.subject(), "task.completed");
    }

    #[test]
    fn test_event_type_categories() {
        assert!(EventType::WorkflowStarted.is_workflow_event());
        assert!(!EventType::WorkflowStarted.is_transition_event());

        assert!(EventType::TransitionFired.is_transition_event());
        assert!(!EventType::TransitionFired.is_workflow_event());

        assert!(EventType::WorkflowCompleted.is_terminal());
        assert!(EventType::WorkflowFailed.is_terminal());
        assert!(!EventType::WorkflowStarted.is_terminal());
    }

    #[test]
    fn test_event_metadata() {
        let meta = EventMetadata::new();
        assert_eq!(meta.version, EVENT_VERSION);
        assert!(meta.correlation_id.is_none());

        let correlated = EventMetadata::with_correlation(Uuid::new_v4());
        assert!(correlated.correlation_id.is_some());

        let caused = EventMetadata::caused_by(&meta);
        assert_eq!(caused.causation_id, Some(meta.event_id));
    }

    #[test]
    fn test_event_serialization() {
        let event = Event::new(
            EventType::WorkflowStarted,
            WorkflowStartedPayload {
                workflow_ref: WorkflowRef {
                    workflow_id: Uuid::new_v4(),
                    workflow_name: "test-workflow".to_string(),
                    namespace: "default".to_string(),
                },
                run_ref: RunRef {
                    run_id: Uuid::new_v4(),
                    workflow_id: Uuid::new_v4(),
                    attempt: 1,
                },
                initial_marking: HashMap::from([("start".to_string(), 1)]),
                inputs: None,
                triggered_by: Some(TriggerType::Api),
            },
        );

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"workflow.started\""));

        let parsed: WorkflowStartedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type, EventType::WorkflowStarted);
    }

    #[test]
    fn test_error_info() {
        let error = ErrorInfo::new("TIMEOUT", "Operation timed out")
            .with_details(serde_json::json!({"timeout_ms": 5000}));

        assert_eq!(error.code, "TIMEOUT");
        assert!(!error.retryable);

        let retryable = ErrorInfo::retryable("NETWORK_ERROR", "Connection failed");
        assert!(retryable.retryable);
    }

    #[test]
    fn test_token_data() {
        let token = TokenData::new("place-1");
        assert_eq!(token.place_id, "place-1");
        assert!(token.data.is_none());

        let colored = TokenData::with_data("place-2", serde_json::json!({"key": "value"}));
        assert!(colored.data.is_some());
    }
}
