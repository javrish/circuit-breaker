//! # Circuit Breaker API Server
//!
//! REST and WebSocket API for the Circuit Breaker workflow engine.
//!
//! This crate provides:
//! - REST API endpoints for workflow CRUD operations
//! - REST API endpoints for run management
//! - WebSocket support for real-time event streaming
//! - Health and metrics endpoints
//! - NATS integration for task dispatch
//!
//! ## Architecture
//!
//! The API server is built with Axum and provides:
//! - `/api/v1/workflows` - Workflow management
//! - `/api/v1/runs` - Run management
//! - `/api/v1/events/ws` - WebSocket event stream
//! - `/health` - Health check
//! - `/metrics` - Prometheus metrics
//!
//! ## Event Sourcing
//!
//! Run state is event-sourced from NATS JetStream. The `state` module provides
//! reconstruction of run state by replaying events.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod state;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use jsonschema::JSONSchema;

use futures::StreamExt;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use cb_core::events::{
    RunRef, TokenData, TokenInjectedPayload, TokenProducedPayload, TokenConsumedPayload,
    TokenUpdatedPayload, TransitionRef, WorkflowRef, WorkflowStartedPayload, TriggerType,
};
use cb_core::workflow::Workflow;
use cb_nats::{streams, NatsClient, NatsConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// API server configuration.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Host to bind to.
    pub host: IpAddr,
    /// Port to listen on.
    pub port: u16,
    /// NATS server URL.
    pub nats_url: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port: 9000,
            nats_url: "nats://localhost:4222".to_string(),
        }
    }
}

/// Stored workflow with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredWorkflow {
    /// Unique workflow ID.
    pub workflow_id: Uuid,
    /// Workflow name.
    pub name: String,
    /// Workflow namespace.
    pub namespace: String,
    /// The workflow definition.
    pub definition: Workflow,
    /// Creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Workflow run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    /// Unique run ID.
    pub run_id: Uuid,
    /// Workflow ID this run belongs to.
    pub workflow_id: Uuid,
    /// Workflow name.
    pub workflow_name: String,
    /// Current status.
    pub status: RunStatus,
    /// Start timestamp.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Completion timestamp.
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Current marking (token distribution).
    pub current_marking: HashMap<String, u32>,
    /// Token data for each place (data attached to tokens).
    #[serde(default)]
    pub token_data: HashMap<String, serde_json::Value>,
    /// Transition statuses.
    pub transitions: Vec<TransitionStatus>,
    /// Error info if failed.
    pub error: Option<ErrorInfo>,
}

/// Run status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// Run is pending.
    Pending,
    /// Run is executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed.
    Failed,
    /// Run was cancelled.
    Cancelled,
}

/// Transition status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionStatus {
    /// Transition ID.
    pub transition_id: String,
    /// Current status.
    pub status: String,
    /// Attempt number.
    pub attempt: u32,
    /// Start timestamp.
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Completion timestamp.
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Error message.
    pub error: Option<String>,
}

/// Error information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInfo {
    /// Error code.
    pub code: String,
    /// Error message.
    pub message: String,
    /// Failed transition.
    pub transition: Option<String>,
}

/// Request to inject a token into a place.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectTokenRequest {
    /// Place ID to inject token into.
    pub place_id: String,
    /// Optional token data (must conform to place's token_schema if defined).
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    /// Optional reason for injection.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Token schema information for a place.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceSchemaInfo {
    /// Place ID.
    pub place_id: String,
    /// Token schema (JSON Schema) if defined.
    pub token_schema: Option<serde_json::Value>,
    /// Current token count.
    pub token_count: u32,
    /// Whether tokens require data.
    pub requires_data: bool,
}

/// Response after injecting a token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectTokenResponse {
    /// Run ID.
    pub run_id: Uuid,
    /// Place where token was injected.
    pub place_id: String,
    /// New token count in that place.
    pub token_count: u32,
    /// Transitions that are now enabled.
    pub enabled_transitions: Vec<String>,
    /// Token schema for the place (if any).
    pub token_schema: Option<serde_json::Value>,
}

/// Request to update token data in a place (gate pattern).
/// This allows updating token data to satisfy guards without adding new tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTokenRequest {
    /// Place ID where tokens should be updated.
    pub place_id: String,
    /// New token data (will be merged with existing data).
    pub data: serde_json::Value,
    /// Optional reason for update.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Response after updating token data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTokenResponse {
    /// Run ID.
    pub run_id: Uuid,
    /// Place where token data was updated.
    pub place_id: String,
    /// Token count in that place.
    pub token_count: u32,
    /// Transitions that are now enabled (guards satisfied).
    pub enabled_transitions: Vec<String>,
    /// Transitions still waiting on guards.
    pub waiting_transitions: Vec<String>,
}

/// Request to resume a workflow by updating token data to satisfy a failed guard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeRequest {
    /// Place ID where the token is waiting.
    pub place_id: String,
    /// Updated token data to satisfy the guard.
    pub data: serde_json::Value,
    /// Optional reason for the resume.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Response after resuming a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeResponse {
    /// Run ID.
    pub run_id: Uuid,
    /// Place where token was updated.
    pub place_id: String,
    /// Token count in that place.
    pub token_count: u32,
    /// Whether a new token was injected (true) or existing token updated (false).
    pub injected: bool,
    /// Transitions that are now enabled (guards passed).
    pub enabled_transitions: Vec<String>,
    /// Transitions still waiting on guards.
    pub waiting_transitions: Vec<String>,
}

/// Response with place schema information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribePlacesResponse {
    /// Run ID.
    pub run_id: Uuid,
    /// Workflow name.
    pub workflow_name: String,
    /// Information about each place.
    pub places: Vec<PlaceSchemaInfo>,
}

/// Task execution log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLog {
    /// Task ID.
    pub task_id: Uuid,
    /// Run ID.
    pub run_id: Uuid,
    /// Transition ID.
    pub transition_id: String,
    /// Task status.
    pub status: String,
    /// Task output.
    pub output: Option<serde_json::Value>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Start timestamp.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Completion timestamp.
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Task dispatch message sent to NATS for runners.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDispatchMessage {
    /// Unique task ID.
    pub task_id: Uuid,
    /// Reference to the transition.
    pub transition_ref: TransitionRef,
    /// Action to execute.
    pub action: serde_json::Value,
    /// Resource requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<TaskResources>,
    /// Timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// Runner pool to use.
    pub runner_pool: String,
    /// Environment variables.
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// Input tokens.
    #[serde(default)]
    pub input_tokens: Vec<TokenData>,
    /// Policy gate to evaluate after action completes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<cb_core::workflow::PolicyGate>,
}

/// Task resource requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResources {
    /// CPU request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    /// Memory request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    /// Stored workflows (definitions, not event-sourced).
    pub workflows: Arc<RwLock<HashMap<Uuid, StoredWorkflow>>>,
    /// Workflow runs (in-memory cache, will be deprecated in favor of event sourcing).
    pub runs: Arc<RwLock<HashMap<Uuid, WorkflowRun>>>,
    /// Task logs indexed by run_id.
    pub task_logs: Arc<RwLock<HashMap<Uuid, Vec<TaskLog>>>>,
    /// NATS client.
    pub nats: Option<Arc<NatsClient>>,
    /// Event-sourced state store.
    pub event_store: Option<Arc<state::EventSourcedStateStore>>,
    /// Shutdown signal sender.
    pub shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            task_logs: Arc::new(RwLock::new(HashMap::new())),
            nats: None,
            event_store: None,
            shutdown_tx: None,
        }
    }
}

impl AppState {
    /// Create a new AppState with NATS client.
    pub fn with_nats(nats: NatsClient) -> Self {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        let nats_arc = Arc::new(nats);
        let event_store = state::EventSourcedStateStore::new(Arc::clone(&nats_arc));
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            task_logs: Arc::new(RwLock::new(HashMap::new())),
            nats: Some(nats_arc),
            event_store: Some(Arc::new(event_store)),
            shutdown_tx: Some(shutdown_tx),
        }
    }
}

/// Message received when a transition completes.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionCompletedMessage {
    transition_ref: TransitionRef,
    #[serde(default)]
    produced_tokens: Vec<TokenData>,
    #[serde(default)]
    outputs: Option<serde_json::Value>,
    #[serde(default)]
    resource_usage: Option<ResourceUsageMessage>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceUsageMessage {
    #[serde(default)]
    duration_ms: Option<u64>,
}

/// Message received when a transition fails.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionFailedMessage {
    transition_ref: TransitionRef,
    error: TransitionError,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionError {
    code: String,
    message: String,
}

/// Start the event subscriber that listens for transition completions.
pub async fn start_event_subscriber(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let nats = match &state.nats {
        Some(n) => n.clone(),
        None => {
            warn!("NATS not connected, event subscriber not started");
            return Ok(());
        }
    };

    let js = nats.jetstream();

    // Create consumer for transition completed/failed events
    let consumer_config = async_nats::jetstream::consumer::pull::Config {
        durable_name: Some("cb-api-events".to_string()),
        filter_subject: "cb.runs.*.transitions.*.>".to_string(),
        ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
        ack_wait: Duration::from_secs(30),
        ..Default::default()
    };

    let stream = js.get_stream(streams::RUNS).await?;
    let consumer = stream
        .get_or_create_consumer("cb-api-events", consumer_config)
        .await?;

    info!("Event subscriber started, listening for transition events");

    let mut messages = consumer.messages().await?;
    let mut shutdown_rx = state.shutdown_tx.as_ref().map(|tx| tx.subscribe());

    loop {
        tokio::select! {
            Some(msg) = messages.next() => {
                match msg {
                    Ok(msg) => {
                        let subject = msg.subject.as_str();

                        // Parse run_id and transition_id from subject
                        // Format: cb.runs.{run_id}.transitions.{transition_id}.{event}
                        let parts: Vec<&str> = subject.split('.').collect();
                        if parts.len() >= 6 {
                            let run_id_str = parts[2];
                            let event_type = parts.last().unwrap_or(&"");

                            if let Ok(run_id) = Uuid::parse_str(run_id_str) {
                                match *event_type {
                                    "completed" => {
                                        if let Ok(completed) = serde_json::from_slice::<TransitionCompletedMessage>(&msg.payload) {
                                            handle_transition_completed(&state, run_id, completed).await;
                                        }
                                    }
                                    "failed" => {
                                        if let Ok(failed) = serde_json::from_slice::<TransitionFailedMessage>(&msg.payload) {
                                            handle_transition_failed(&state, run_id, failed).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        let _ = msg.ack().await;
                    }
                    Err(e) => {
                        error!(error = %e, "Error receiving message");
                    }
                }
            }
            _ = async {
                if let Some(ref mut rx) = shutdown_rx {
                    let _ = rx.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("Event subscriber shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_transition_completed(
    state: &AppState,
    run_id: Uuid,
    msg: TransitionCompletedMessage,
) {
    let now = chrono::Utc::now();
    let duration_ms = msg.resource_usage.as_ref().and_then(|r| r.duration_ms);
    let transition_id = msg.transition_ref.transition_id.clone();

    // Store task log
    {
        let mut logs = state.task_logs.write().await;
        let run_logs = logs.entry(run_id).or_insert_with(Vec::new);
        run_logs.push(TaskLog {
            task_id: msg.transition_ref.execution_id.unwrap_or_else(Uuid::new_v4),
            run_id,
            transition_id: transition_id.clone(),
            status: "completed".to_string(),
            output: msg.outputs.clone(),
            error: None,
            started_at: now - chrono::Duration::milliseconds(duration_ms.unwrap_or(0) as i64),
            completed_at: Some(now),
            duration_ms,
        });
    }

    // Get workflow definition for this run
    let workflow_def = {
        let runs = state.runs.read().await;
        let run = match runs.get(&run_id) {
            Some(r) => r,
            None => {
                warn!(%run_id, "Run not found for transition completion");
                return;
            }
        };

        let workflows = state.workflows.read().await;
        match workflows.get(&run.workflow_id) {
            Some(w) => w.definition.clone(),
            None => {
                warn!(%run_id, "Workflow not found for run");
                return;
            }
        }
    };

    // Find the transition that completed
    let transition = match workflow_def.transitions.iter().find(|t| t.id == transition_id) {
        Some(t) => t.clone(),
        None => {
            warn!(%run_id, %transition_id, "Transition not found in workflow");
            return;
        }
    };

    // Get workflow_id for event payloads
    let workflow_id = {
        let runs = state.runs.read().await;
        runs.get(&run_id).map(|r| r.workflow_id).unwrap_or(run_id)
    };

    // Publish token.consumed events for input places
    if let Some(ref nats) = state.nats {
        for input in &transition.inputs {
            for _ in 0..input.weight {
                let token = TokenData::new(input.place.clone());
                let consumed_payload = TokenConsumedPayload {
                    run_ref: RunRef {
                        run_id,
                        workflow_id,
                        attempt: 1,
                    },
                    token,
                    consumed_by: transition_id.clone(),
                };

                let subject = cb_nats::subjects::token_consumed(&run_id, &input.place);
                if let Err(e) = nats.publish_jetstream(&subject, &consumed_payload).await {
                    warn!(error = %e, place = %input.place, "Failed to publish token.consumed event");
                } else {
                    debug!(%run_id, place = %input.place, "Published token.consumed event");
                }
            }
        }

        // Publish token.produced events for output places
        let output_data = msg.outputs.clone();
        for output in &transition.outputs {
            for _ in 0..output.weight {
                let token = if let Some(ref data) = output_data {
                    TokenData::with_data(output.place.clone(), data.clone())
                } else {
                    TokenData::new(output.place.clone())
                };
                let produced_payload = TokenProducedPayload {
                    run_ref: RunRef {
                        run_id,
                        workflow_id,
                        attempt: 1,
                    },
                    token,
                    produced_by: transition_id.clone(),
                };

                let subject = cb_nats::subjects::token_produced(&run_id, &output.place);
                if let Err(e) = nats.publish_jetstream(&subject, &produced_payload).await {
                    warn!(error = %e, place = %output.place, "Failed to publish token.produced event");
                } else {
                    debug!(%run_id, place = %output.place, "Published token.produced event");
                }
            }
        }
    }

    let mut runs = state.runs.write().await;

    if let Some(run) = runs.get_mut(&run_id) {
        // Update transition status
        if let Some(ts) = run
            .transitions
            .iter_mut()
            .find(|t| t.transition_id == transition_id)
        {
            ts.status = "completed".to_string();
            ts.completed_at = Some(now);
        }

        // Move tokens: consume from input places, produce to output places
        for input in &transition.inputs {
            let count = run.current_marking.entry(input.place.clone()).or_insert(0);
            *count = count.saturating_sub(input.weight);
            // Clear token data from consumed places
            if *count == 0 {
                run.token_data.remove(&input.place);
            }
        }

        // Store transition output as token data for output places
        let output_data = msg.outputs.clone();
        for output in &transition.outputs {
            let count = run.current_marking.entry(output.place.clone()).or_insert(0);
            *count += output.weight;
            // Store the transition output as token data for this place
            if let Some(ref data) = output_data {
                run.token_data.insert(output.place.clone(), data.clone());
            }
        }

        info!(
            %run_id,
            %transition_id,
            marking = ?run.current_marking,
            token_data = ?run.token_data,
            "Tokens moved after transition completion"
        );

        // Check if workflow is complete (no tokens in non-terminal places or specific completion condition)
        // A simple heuristic: if there are tokens only in places with no outgoing transitions, we're done
        let terminal_places: std::collections::HashSet<_> = workflow_def
            .places
            .iter()
            .filter(|p| {
                !workflow_def
                    .transitions
                    .iter()
                    .any(|t| t.inputs.iter().any(|i| i.place == p.id))
            })
            .map(|p| p.id.clone())
            .collect();

        let all_tokens_in_terminal = run.current_marking.iter().all(|(place, &count)| {
            count == 0 || terminal_places.contains(place)
        });

        if all_tokens_in_terminal && run.current_marking.values().any(|&c| c > 0) {
            run.status = RunStatus::Completed;
            run.completed_at = Some(now);
            info!(run_id = %run_id, "Workflow run completed");
        } else {
            // Find and publish newly enabled transitions
            let enabled = find_enabled_transitions(&workflow_def, &run.current_marking, &run.token_data);

            // Collect token data for enabled transitions and drop the lock before publishing
            let token_data_snapshot = run.token_data.clone();
            drop(runs);

            if !enabled.is_empty() {
                info!(%run_id, enabled = ?enabled, "New transitions enabled");

                // Publish enabled events
                if let Some(nats) = &state.nats {
                    for tid in enabled {
                        // Find the transition to get its input places
                        let trans = workflow_def.transitions.iter().find(|t| t.id == tid);

                        // Build input tokens from token data of input places
                        // Format must match TokenData struct: token_id, place_id, data, created_at
                        let input_tokens: Vec<serde_json::Value> = trans
                            .map(|t| {
                                t.inputs.iter().filter_map(|input| {
                                    token_data_snapshot.get(&input.place).map(|data| {
                                        serde_json::json!({
                                            "tokenId": Uuid::new_v4().to_string(),
                                            "placeId": input.place,
                                            "data": data,
                                            "createdAt": chrono::Utc::now().to_rfc3339()
                                        })
                                    })
                                }).collect()
                            })
                            .unwrap_or_default();

                        let subject = cb_nats::subjects::transition_enabled(&run_id, &tid);
                        let payload = serde_json::json!({
                            "transitionRef": {
                                "runId": run_id.to_string(),
                                "transitionId": tid,
                            },
                            "taskId": Uuid::new_v4().to_string(),
                            "action": trans.map(|t| &t.action),
                            "inputTokens": input_tokens,
                            "runnerPool": "default",
                            "environment": {},
                        });

                        if let Err(e) = nats.publish_jetstream(&subject, &payload).await {
                            error!(error = %e, %subject, "Failed to publish TransitionEnabled");
                        } else {
                            debug!(%subject, "Published TransitionEnabled");
                        }
                    }
                }
            }

            return;
        }
    }
}

async fn handle_transition_failed(state: &AppState, run_id: Uuid, msg: TransitionFailedMessage) {
    let now = chrono::Utc::now();

    // Store task log
    {
        let mut logs = state.task_logs.write().await;
        let run_logs = logs.entry(run_id).or_insert_with(Vec::new);
        run_logs.push(TaskLog {
            task_id: msg.transition_ref.execution_id.unwrap_or_else(Uuid::new_v4),
            run_id,
            transition_id: msg.transition_ref.transition_id.clone(),
            status: "failed".to_string(),
            output: None,
            error: Some(msg.error.message.clone()),
            started_at: now,
            completed_at: Some(now),
            duration_ms: None,
        });
    }

    let mut runs = state.runs.write().await;

    if let Some(run) = runs.get_mut(&run_id) {
        // Update transition status
        if let Some(ts) = run
            .transitions
            .iter_mut()
            .find(|t| t.transition_id == msg.transition_ref.transition_id)
        {
            ts.status = "failed".to_string();
            ts.completed_at = Some(now);
            ts.error = Some(msg.error.message.clone());
        }

        // Mark run as failed
        run.status = RunStatus::Failed;
        run.completed_at = Some(now);
        run.error = Some(ErrorInfo {
            code: msg.error.code,
            message: msg.error.message,
            transition: Some(msg.transition_ref.transition_id),
        });

        info!(run_id = %run_id, "Workflow run failed");
    }
}

// ============ Health & Version ============

/// Health check response.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Health status.
    pub status: String,
    /// Application version.
    pub version: String,
    /// NATS connection status.
    pub nats_connected: bool,
}

/// Health check handler.
async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        nats_connected: state.nats.is_some(),
    })
}

/// Version info response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    /// Application version.
    pub version: String,
    /// Git commit hash.
    pub git_commit: String,
    /// Build timestamp.
    pub build_time: String,
}

/// Version handler.
async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_string(),
        build_time: option_env!("BUILD_TIME").unwrap_or("unknown").to_string(),
    })
}

// ============ Workflow Endpoints ============

/// Submit workflow response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitWorkflowResponse {
    /// Workflow ID.
    pub workflow_id: Uuid,
    /// Workflow name.
    pub name: String,
    /// Workflow namespace.
    pub namespace: String,
    /// Creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Submit a new workflow.
async fn submit_workflow(
    State(state): State<AppState>,
    Json(workflow): Json<Workflow>,
) -> Result<Json<SubmitWorkflowResponse>, ApiError> {
    let workflow_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let stored = StoredWorkflow {
        workflow_id,
        name: workflow.name.clone(),
        namespace: workflow.namespace.clone(),
        definition: workflow.clone(),
        created_at: now,
    };

    let response = SubmitWorkflowResponse {
        workflow_id,
        name: workflow.name,
        namespace: workflow.namespace,
        created_at: now,
    };

    let mut workflows = state.workflows.write().await;
    workflows.insert(workflow_id, stored);

    info!(%workflow_id, "Workflow submitted");

    Ok(Json(response))
}

/// Get a workflow by ID.
async fn get_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<Uuid>,
) -> Result<Json<StoredWorkflow>, ApiError> {
    let workflows = state.workflows.read().await;

    workflows
        .get(&workflow_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("Workflow {} not found", workflow_id)))
}

/// List workflows response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWorkflowsResponse {
    /// List of workflows.
    pub workflows: Vec<StoredWorkflow>,
    /// Total count.
    pub total: usize,
    /// Offset.
    pub offset: usize,
    /// Limit.
    pub limit: usize,
}

/// List all workflows.
async fn list_workflows(State(state): State<AppState>) -> Json<ListWorkflowsResponse> {
    let workflows = state.workflows.read().await;
    let all: Vec<_> = workflows.values().cloned().collect();
    let total = all.len();

    Json(ListWorkflowsResponse {
        workflows: all,
        total,
        offset: 0,
        limit: 100,
    })
}

/// Delete a workflow.
async fn delete_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut workflows = state.workflows.write().await;

    if workflows.remove(&workflow_id).is_some() {
        info!(%workflow_id, "Workflow deleted");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!(
            "Workflow {} not found",
            workflow_id
        )))
    }
}

// ============ Run Endpoints ============

/// Run workflow request.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunWorkflowRequest {
    /// Input parameters.
    #[serde(default)]
    pub inputs: Option<serde_json::Value>,
    /// Labels.
    #[serde(default)]
    pub labels: Option<HashMap<String, String>>,
}

/// Run workflow response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunWorkflowResponse {
    /// Run ID.
    pub run_id: Uuid,
    /// Workflow ID.
    pub workflow_id: Uuid,
    /// Status.
    pub status: RunStatus,
    /// Start timestamp.
    pub started_at: chrono::DateTime<chrono::Utc>,
}

/// Start a workflow run.
async fn run_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<Uuid>,
    Json(_request): Json<RunWorkflowRequest>,
) -> Result<Json<RunWorkflowResponse>, ApiError> {
    let workflows = state.workflows.read().await;

    let workflow = workflows
        .get(&workflow_id)
        .ok_or_else(|| ApiError::not_found(format!("Workflow {} not found", workflow_id)))?;

    let run_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // Build initial marking from workflow
    let initial_marking: HashMap<String, u32> = workflow
        .definition
        .places
        .iter()
        .filter(|p| p.initial_tokens > 0)
        .map(|p| (p.id.clone(), p.initial_tokens))
        .collect();

    // Build transition statuses
    let transitions: Vec<TransitionStatus> = workflow
        .definition
        .transitions
        .iter()
        .map(|t| TransitionStatus {
            transition_id: t.id.clone(),
            status: "pending".to_string(),
            attempt: 0,
            started_at: None,
            completed_at: None,
            error: None,
        })
        .collect();

    let run = WorkflowRun {
        run_id,
        workflow_id,
        workflow_name: workflow.name.clone(),
        status: RunStatus::Running,
        started_at: now,
        completed_at: None,
        current_marking: initial_marking.clone(),
        token_data: HashMap::new(),
        transitions,
        error: None,
    };

    let response = RunWorkflowResponse {
        run_id,
        workflow_id,
        status: RunStatus::Running,
        started_at: now,
    };

    // Find enabled transitions and dispatch tasks (no token data yet at start)
    let empty_token_data: HashMap<String, serde_json::Value> = HashMap::new();
    let enabled_transitions = find_enabled_transitions(&workflow.definition, &initial_marking, &empty_token_data);

    // Clone what we need before releasing the lock
    let definition = workflow.definition.clone();
    drop(workflows);

    // Store the run
    {
        let mut runs = state.runs.write().await;
        runs.insert(run_id, run);
    }

    info!(%run_id, %workflow_id, "Workflow run started");

    // Publish workflow.started event to NATS
    if let Some(ref nats) = state.nats {
        let started_payload = WorkflowStartedPayload {
            workflow_ref: WorkflowRef {
                workflow_id,
                workflow_name: definition.name.clone(),
                namespace: definition.namespace.clone(),
            },
            run_ref: RunRef {
                run_id,
                workflow_id,
                attempt: 1,
            },
            initial_marking: initial_marking.clone(),
            inputs: None,
            triggered_by: Some(TriggerType::Api),
        };

        let subject = format!("cb.runs.{}.started", run_id);
        if let Err(e) = nats.publish_jetstream(&subject, &started_payload).await {
            warn!(error = %e, "Failed to publish workflow.started event");
        } else {
            info!(%run_id, subject = %subject, "Published workflow.started event");
        }

        // Publish token.produced events for initial marking
        for (place_id, count) in &initial_marking {
            for _ in 0..*count {
                let token = TokenData::new(place_id.clone());
                let produced_payload = TokenProducedPayload {
                    run_ref: RunRef {
                        run_id,
                        workflow_id,
                        attempt: 1,
                    },
                    token,
                    produced_by: "workflow.start".to_string(),
                };

                let subject = cb_nats::subjects::token_produced(&run_id, place_id);
                if let Err(e) = nats.publish_jetstream(&subject, &produced_payload).await {
                    warn!(error = %e, %place_id, "Failed to publish token.produced event");
                } else {
                    debug!(%run_id, %place_id, "Published token.produced event");
                }
            }
        }

        // Dispatch tasks for enabled transitions
        for transition_id in enabled_transitions {
            if let Some(transition) = definition
                .transitions
                .iter()
                .find(|t| t.id == transition_id)
            {
                let task_id = Uuid::new_v4();

                let action_json = serde_json::to_value(&transition.action).unwrap_or_default();

                let resources = transition.resources.as_ref().map(|r| TaskResources {
                    cpu: r.cpu.clone(),
                    memory: r.memory.clone(),
                });

                let task = TaskDispatchMessage {
                    task_id,
                    transition_ref: TransitionRef {
                        transition_id: transition.id.clone(),
                        run_id,
                        execution_id: Some(task_id),
                    },
                    action: action_json,
                    resources,
                    timeout: Some(transition.timeout.clone()),
                    runner_pool: "default".to_string(),
                    environment: HashMap::new(),
                    input_tokens: vec![],
                    policy: None,
                };

                let subject = format!("cb.runs.{}.transitions.{}.enabled", run_id, transition.id);

                match nats.publish_jetstream(&subject, &task).await {
                    Ok(_) => {
                        info!(
                            %task_id,
                            %run_id,
                            transition_id = %transition.id,
                            subject = %subject,
                            "Task dispatched to NATS"
                        );
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            %run_id,
                            transition_id = %transition.id,
                            "Failed to dispatch task to NATS"
                        );
                    }
                }
            }
        }
    } else {
        warn!(%run_id, "NATS not connected, tasks not dispatched");
    }

    Ok(Json(response))
}

/// Find enabled transitions based on current marking and guard evaluation (gate semantics).
///
/// Gate semantics: A transition is enabled if:
/// 1. All input places have enough tokens (structurally enabled)
/// 2. Guard evaluates to true (if present)
///
/// If guard evaluates to false, the transition remains structurally enabled but won't fire.
/// When token data changes, guards are re-evaluated and transitions may become fully enabled.
fn find_enabled_transitions(
    workflow: &Workflow,
    marking: &HashMap<String, u32>,
    token_data: &HashMap<String, serde_json::Value>,
) -> Vec<String> {
    workflow
        .transitions
        .iter()
        .filter(|t| {
            // A transition is structurally enabled if all input places have enough tokens
            let has_tokens = t.inputs.iter().all(|arc| {
                let tokens = marking.get(&arc.place).copied().unwrap_or(0);
                tokens >= arc.weight
            });

            if !has_tokens {
                return false;
            }

            // If there's a guard, evaluate it (gate semantics)
            if let Some(ref guard_expr) = t.guard {
                match evaluate_guard(guard_expr, t, token_data) {
                    Ok(result) => {
                        if !result {
                            info!(
                                transition_id = %t.id,
                                guard = %guard_expr,
                                "Guard evaluated to false, transition waiting (gate)"
                            );
                        }
                        result
                    }
                    Err(e) => {
                        warn!(
                            transition_id = %t.id,
                            guard = %guard_expr,
                            error = %e,
                            "Guard evaluation failed, transition waiting"
                        );
                        false
                    }
                }
            } else {
                true
            }
        })
        .map(|t| t.id.clone())
        .collect()
}

/// Find transitions that are structurally enabled (have tokens) but waiting on guards.
/// These transitions should be re-evaluated when token data changes.
fn find_waiting_transitions(
    workflow: &Workflow,
    marking: &HashMap<String, u32>,
    token_data: &HashMap<String, serde_json::Value>,
) -> Vec<String> {
    workflow
        .transitions
        .iter()
        .filter(|t| {
            // Must have tokens in all input places
            let has_tokens = t.inputs.iter().all(|arc| {
                let tokens = marking.get(&arc.place).copied().unwrap_or(0);
                tokens >= arc.weight
            });

            if !has_tokens {
                return false;
            }

            // Must have a guard that evaluates to false
            if let Some(ref guard_expr) = t.guard {
                match evaluate_guard(guard_expr, t, token_data) {
                    Ok(result) => !result, // Waiting if guard is false
                    Err(_) => true, // Also waiting if guard errored
                }
            } else {
                false // No guard means not waiting
            }
        })
        .map(|t| t.id.clone())
        .collect()
}

/// Evaluate a guard expression using CEL.
fn evaluate_guard(
    guard_expr: &str,
    transition: &cb_core::workflow::Transition,
    token_data: &HashMap<String, serde_json::Value>,
) -> Result<bool, String> {
    use cel_interpreter::{Context, Program, Value, objects::{Key, Map}};
    use std::sync::Arc;

    // Compile the guard expression
    let program = Program::compile(guard_expr)
        .map_err(|e| format!("Failed to compile guard expression: {}", e))?;

    // Build context with token data from input places
    let mut context = Context::default();

    // Merge all input token data into ctx
    let mut ctx_map: HashMap<Key, Value> = HashMap::new();
    for input in &transition.inputs {
        if let Some(data) = token_data.get(&input.place) {
            if let Some(obj) = data.as_object() {
                for (k, v) in obj {
                    ctx_map.insert(Key::String(k.clone().into()), json_to_cel_value(v));
                }
            }
        }
    }

    // Add ctx as a variable
    context.add_variable("ctx", Value::Map(Map { map: Arc::new(ctx_map) }))
        .map_err(|e| format!("Failed to add ctx variable: {}", e))?;

    // Execute the guard expression
    let result = program.execute(&context)
        .map_err(|e| format!("Failed to execute guard expression: {}", e))?;

    // The result should be a boolean
    match result {
        Value::Bool(b) => Ok(b),
        _ => Err(format!("Guard expression did not return a boolean: {:?}", result)),
    }
}

/// Convert a serde_json::Value to a CEL Value.
fn json_to_cel_value(v: &serde_json::Value) -> cel_interpreter::Value {
    use cel_interpreter::{Value, objects::{Key, Map}};
    use std::sync::Arc;

    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(u) = n.as_u64() {
                Value::UInt(u)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone().into()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(json_to_cel_value).collect::<Vec<_>>().into())
        }
        serde_json::Value::Object(obj) => {
            let map: HashMap<Key, Value> = obj.iter()
                .map(|(k, v)| (Key::String(k.clone().into()), json_to_cel_value(v)))
                .collect();
            Value::Map(Map { map: Arc::new(map) })
        }
    }
}

/// Get run status.
async fn get_run_status(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<WorkflowRun>, ApiError> {
    let runs = state.runs.read().await;

    runs.get(&run_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("Run {} not found", run_id)))
}

/// List runs response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRunsResponse {
    /// List of runs.
    pub runs: Vec<WorkflowRun>,
    /// Total count.
    pub total: usize,
    /// Offset.
    pub offset: usize,
    /// Limit.
    pub limit: usize,
}

/// List all runs.
async fn list_runs(State(state): State<AppState>) -> Json<ListRunsResponse> {
    let runs = state.runs.read().await;
    let all: Vec<_> = runs.values().cloned().collect();
    let total = all.len();

    Json(ListRunsResponse {
        runs: all,
        total,
        offset: 0,
        limit: 100,
    })
}

/// Get logs response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLogsResponse {
    /// Run ID.
    pub run_id: Uuid,
    /// Task logs.
    pub logs: Vec<TaskLog>,
}

/// Get logs for a run.
async fn get_run_logs(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<GetLogsResponse>, ApiError> {
    // First check if run exists
    {
        let runs = state.runs.read().await;
        if !runs.contains_key(&run_id) {
            return Err(ApiError::not_found(format!("Run {} not found", run_id)));
        }
    }

    let logs = state.task_logs.read().await;
    let run_logs = logs.get(&run_id).cloned().unwrap_or_default();

    Ok(Json(GetLogsResponse {
        run_id,
        logs: run_logs,
    }))
}

/// Cancel request.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRunRequest {
    /// Cancellation reason.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Inject a token into a specific place in a run.
/// This can be used to trigger specific transitions for testing.
async fn inject_token(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(request): Json<InjectTokenRequest>,
) -> Result<Json<InjectTokenResponse>, ApiError> {
    // Get the run and its workflow
    let (workflow_def, current_marking) = {
        let runs = state.runs.read().await;
        let run = runs
            .get(&run_id)
            .ok_or_else(|| ApiError::not_found(format!("Run {} not found", run_id)))?;

        if run.status != RunStatus::Running && run.status != RunStatus::Pending {
            return Err(ApiError::conflict(format!(
                "Run {} is in terminal state {:?}",
                run_id, run.status
            )));
        }

        let workflows = state.workflows.read().await;
        let workflow = workflows
            .get(&run.workflow_id)
            .ok_or_else(|| ApiError::not_found("Workflow not found"))?;

        (workflow.definition.clone(), run.current_marking.clone())
    };

    // Find the place and validate it exists
    let place = workflow_def
        .places
        .iter()
        .find(|p| p.id == request.place_id)
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "Place '{}' not found in workflow",
                request.place_id
            ))
        })?;

    // Validate token data against schema if defined
    if let Some(ref schema) = place.token_schema {
        match JSONSchema::compile(schema) {
            Ok(compiled) => {
                let data_to_validate = request.data.clone().unwrap_or(serde_json::Value::Null);
                let validation_result = compiled.validate(&data_to_validate);
                if let Err(errors) = validation_result {
                    let error_messages: Vec<String> = errors.map(|e| format!("{}", e)).collect();
                    return Err(ApiError::bad_request(format!(
                        "Token data does not match schema for place '{}': {}",
                        request.place_id,
                        error_messages.join(", ")
                    )));
                }
            }
            Err(e) => {
                warn!(error = %e, "Invalid token schema in workflow");
            }
        }
    }

    let token_schema = place.token_schema.clone();

    // Publish TokenInjected event to NATS
    if let Some(ref nats) = state.nats {
        let token = if let Some(ref data) = request.data {
            TokenData::with_data(request.place_id.clone(), data.clone())
        } else {
            TokenData::new(request.place_id.clone())
        };

        let payload = TokenInjectedPayload {
            run_ref: RunRef {
                run_id,
                workflow_id: run_id, // Use run_id as workflow_id for now
                attempt: 1,
            },
            token,
            place_id: request.place_id.clone(),
            injected_by: "api".to_string(),
            reason: request.reason.clone(),
        };

        let subject = cb_nats::subjects::token_injected(&run_id, &request.place_id);
        if let Err(e) = nats.publish_jetstream(&subject, &payload).await {
            warn!(error = %e, "Failed to publish TokenInjected event");
        } else {
            info!(subject = %subject, "Published TokenInjected event");
        }
    }

    // Update marking and token data
    let (new_token_count, token_data) = {
        let mut runs = state.runs.write().await;
        let run = runs.get_mut(&run_id).unwrap();

        // Update token count
        let count = run
            .current_marking
            .entry(request.place_id.clone())
            .or_insert(0);
        *count += 1;
        let new_count = *count;

        // Store token data if provided (this enables gate pattern - updating data to satisfy guards)
        if let Some(ref data) = request.data {
            run.token_data.insert(request.place_id.clone(), data.clone());
            info!(
                place_id = %request.place_id,
                data = ?data,
                "Token data stored for place"
            );
        }

        (new_count, run.token_data.clone())
    };

    // Find newly enabled transitions (including those that were waiting on guards)
    let mut new_marking = current_marking.clone();
    *new_marking.entry(request.place_id.clone()).or_insert(0) += 1;
    let enabled_transitions = find_enabled_transitions(&workflow_def, &new_marking, &token_data);

    // Also find transitions that are still waiting
    let waiting_transitions = find_waiting_transitions(&workflow_def, &new_marking, &token_data);
    if !waiting_transitions.is_empty() {
        info!(
            run_id = %run_id,
            waiting = ?waiting_transitions,
            "Transitions waiting on guards"
        );
    }

    info!(
        run_id = %run_id,
        place_id = %request.place_id,
        token_count = new_token_count,
        enabled = ?enabled_transitions,
        "Token injected"
    );

    // Dispatch tasks for newly enabled transitions
    if let Some(ref nats) = state.nats {
        for transition_id in &enabled_transitions {
            if let Some(transition) = workflow_def
                .transitions
                .iter()
                .find(|t| &t.id == transition_id)
            {
                let task_id = Uuid::new_v4();
                let action_json = serde_json::to_value(&transition.action).unwrap_or_default();

                let resources = transition.resources.as_ref().map(|r| TaskResources {
                    cpu: r.cpu.clone(),
                    memory: r.memory.clone(),
                });

                // Build input tokens with current token data from input places
                let input_tokens: Vec<TokenData> = transition.inputs.iter().filter_map(|input| {
                    token_data.get(&input.place).map(|data| TokenData {
                        token_id: Uuid::new_v4(),
                        place_id: input.place.clone(),
                        data: Some(data.clone()),
                        created_at: chrono::Utc::now(),
                    })
                }).collect();

                let task = TaskDispatchMessage {
                    task_id,
                    transition_ref: TransitionRef {
                        transition_id: transition.id.clone(),
                        run_id,
                        execution_id: Some(task_id),
                    },
                    action: action_json,
                    resources,
                    timeout: Some(transition.timeout.clone()),
                    runner_pool: "default".to_string(),
                    environment: HashMap::new(),
                    input_tokens,
                    policy: None,
                };

                let subject = format!("cb.runs.{}.transitions.{}.enabled", run_id, transition.id);

                if let Err(e) = nats.publish_jetstream(&subject, &task).await {
                    error!(error = %e, "Failed to dispatch task");
                } else {
                    info!(task_id = %task_id, transition_id = %transition.id, "Task dispatched");
                }
            }
        }
    }

    Ok(Json(InjectTokenResponse {
        run_id,
        place_id: request.place_id,
        token_count: new_token_count,
        enabled_transitions,
        token_schema,
    }))
}

/// Update token data in a place (gate pattern).
/// This allows updating token data to satisfy guards without adding new tokens.
/// When token data is updated, guards are re-evaluated and transitions may become enabled.
async fn update_token(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(request): Json<UpdateTokenRequest>,
) -> Result<Json<UpdateTokenResponse>, ApiError> {
    // Get the run and its workflow
    let (workflow_def, workflow_id, current_marking) = {
        let runs = state.runs.read().await;
        let run = runs
            .get(&run_id)
            .ok_or_else(|| ApiError::not_found(format!("Run {} not found", run_id)))?;

        if run.status != RunStatus::Running && run.status != RunStatus::Pending {
            return Err(ApiError::conflict(format!(
                "Run {} is in terminal state {:?}",
                run_id, run.status
            )));
        }

        let workflows = state.workflows.read().await;
        let workflow = workflows
            .get(&run.workflow_id)
            .ok_or_else(|| ApiError::not_found("Workflow not found"))?;

        (workflow.definition.clone(), run.workflow_id, run.current_marking.clone())
    };

    // Validate the place exists
    let _place = workflow_def
        .places
        .iter()
        .find(|p| p.id == request.place_id)
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "Place '{}' not found in workflow",
                request.place_id
            ))
        })?;

    // Check that there are tokens in the place to update
    let token_count = current_marking.get(&request.place_id).copied().unwrap_or(0);
    if token_count == 0 {
        return Err(ApiError::bad_request(format!(
            "No tokens in place '{}' to update",
            request.place_id
        )));
    }

    // Get previous token data for audit trail
    let previous_data = {
        let runs = state.runs.read().await;
        runs.get(&run_id)
            .and_then(|r| r.token_data.get(&request.place_id).cloned())
    };

    // Publish TokenUpdated event to NATS
    if let Some(ref nats) = state.nats {
        let token = TokenData::with_data(request.place_id.clone(), request.data.clone());

        let payload = TokenUpdatedPayload {
            run_ref: RunRef {
                run_id,
                workflow_id,
                attempt: 1,
            },
            token,
            place_id: request.place_id.clone(),
            previous_data,
            updated_by: "api".to_string(),
            reason: request.reason.clone(),
        };

        let subject = cb_nats::subjects::token_updated(&run_id, &request.place_id);
        if let Err(e) = nats.publish_jetstream(&subject, &payload).await {
            warn!(error = %e, "Failed to publish TokenUpdated event");
        } else {
            info!(subject = %subject, "Published TokenUpdated event");
        }
    }

    // Update token data in memory (merge with existing data)
    let token_data = {
        let mut runs = state.runs.write().await;
        let run = runs.get_mut(&run_id).unwrap();

        // Merge new data with existing data
        let existing = run.token_data.entry(request.place_id.clone()).or_insert_with(|| serde_json::json!({}));
        if let (Some(existing_obj), Some(new_obj)) = (existing.as_object_mut(), request.data.as_object()) {
            for (k, v) in new_obj {
                existing_obj.insert(k.clone(), v.clone());
            }
        } else {
            // If not objects, replace entirely
            *existing = request.data.clone();
        }

        info!(
            place_id = %request.place_id,
            data = ?run.token_data.get(&request.place_id),
            "Token data updated for place"
        );

        run.token_data.clone()
    };

    // Re-evaluate guards and find enabled transitions
    let enabled_transitions = find_enabled_transitions(&workflow_def, &current_marking, &token_data);
    let waiting_transitions = find_waiting_transitions(&workflow_def, &current_marking, &token_data);

    info!(
        run_id = %run_id,
        place_id = %request.place_id,
        enabled = ?enabled_transitions,
        waiting = ?waiting_transitions,
        "Guards re-evaluated after token update"
    );

    // Dispatch tasks for newly enabled transitions
    if let Some(ref nats) = state.nats {
        for transition_id in &enabled_transitions {
            if let Some(transition) = workflow_def
                .transitions
                .iter()
                .find(|t| &t.id == transition_id)
            {
                let task_id = Uuid::new_v4();
                let action_json = serde_json::to_value(&transition.action).unwrap_or_default();

                let resources = transition.resources.as_ref().map(|r| TaskResources {
                    cpu: r.cpu.clone(),
                    memory: r.memory.clone(),
                });

                // Build input tokens with current token data
                let input_tokens: Vec<TokenData> = transition.inputs.iter().filter_map(|input| {
                    token_data.get(&input.place).map(|data| {
                        TokenData::with_data(input.place.clone(), data.clone())
                    })
                }).collect();

                let task = TaskDispatchMessage {
                    task_id,
                    transition_ref: TransitionRef {
                        transition_id: transition.id.clone(),
                        run_id,
                        execution_id: Some(task_id),
                    },
                    action: action_json,
                    resources,
                    timeout: Some(transition.timeout.clone()),
                    runner_pool: "default".to_string(),
                    environment: HashMap::new(),
                    input_tokens,
                    policy: None,
                };

                let subject = cb_nats::subjects::transition_enabled(&run_id, &transition.id);

                if let Err(e) = nats.publish_jetstream(&subject, &task).await {
                    error!(error = %e, "Failed to dispatch task");
                } else {
                    info!(task_id = %task_id, transition_id = %transition.id, "Task dispatched after guard satisfied");
                }
            }
        }
    }

    Ok(Json(UpdateTokenResponse {
        run_id,
        place_id: request.place_id,
        token_count,
        enabled_transitions,
        waiting_transitions,
    }))
}

/// Resume a workflow by updating token data to satisfy a failed guard.
/// If no tokens exist in the place, injects one. If tokens exist, updates their data.
/// After updating, guards are re-evaluated and transitions may fire.
async fn resume_workflow(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(request): Json<ResumeRequest>,
) -> Result<Json<ResumeResponse>, ApiError> {
    // Get the run and its workflow
    let (workflow_def, workflow_id, current_marking) = {
        let runs = state.runs.read().await;
        let run = runs
            .get(&run_id)
            .ok_or_else(|| ApiError::not_found(format!("Run {} not found", run_id)))?;

        if run.status != RunStatus::Running && run.status != RunStatus::Pending {
            return Err(ApiError::conflict(format!(
                "Run {} is in terminal state {:?}",
                run_id, run.status
            )));
        }

        let workflows = state.workflows.read().await;
        let workflow = workflows
            .get(&run.workflow_id)
            .ok_or_else(|| ApiError::not_found("Workflow not found"))?;

        (workflow.definition.clone(), run.workflow_id, run.current_marking.clone())
    };

    // Validate the place exists
    let _place = workflow_def
        .places
        .iter()
        .find(|p| p.id == request.place_id)
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "Place '{}' not found in workflow",
                request.place_id
            ))
        })?;

    // Check if tokens already exist in this place
    let existing_token_count = current_marking.get(&request.place_id).copied().unwrap_or(0);
    let injected = existing_token_count == 0;

    // Get previous token data for audit trail (if updating)
    let previous_data = if !injected {
        let runs = state.runs.read().await;
        runs.get(&run_id)
            .and_then(|r| r.token_data.get(&request.place_id).cloned())
    } else {
        None
    };

    // Publish the appropriate event to NATS
    if let Some(ref nats) = state.nats {
        let token = TokenData::with_data(request.place_id.clone(), request.data.clone());

        if injected {
            // Inject new token
            let payload = TokenInjectedPayload {
                run_ref: RunRef {
                    run_id,
                    workflow_id,
                    attempt: 1,
                },
                token,
                place_id: request.place_id.clone(),
                injected_by: "api".to_string(),
                reason: request.reason.clone(),
            };

            let subject = cb_nats::subjects::token_injected(&run_id, &request.place_id);
            if let Err(e) = nats.publish_jetstream(&subject, &payload).await {
                warn!(error = %e, "Failed to publish TokenInjected event");
            } else {
                info!(subject = %subject, "Published TokenInjected event (set_token)");
            }
        } else {
            // Update existing token
            let payload = TokenUpdatedPayload {
                run_ref: RunRef {
                    run_id,
                    workflow_id,
                    attempt: 1,
                },
                token,
                place_id: request.place_id.clone(),
                previous_data,
                updated_by: "api".to_string(),
                reason: request.reason.clone(),
            };

            let subject = cb_nats::subjects::token_updated(&run_id, &request.place_id);
            if let Err(e) = nats.publish_jetstream(&subject, &payload).await {
                warn!(error = %e, "Failed to publish TokenUpdated event");
            } else {
                info!(subject = %subject, "Published TokenUpdated event (set_token)");
            }
        }
    }

    // Update in-memory state
    let (token_count, token_data) = {
        let mut runs = state.runs.write().await;
        let run = runs.get_mut(&run_id).unwrap();

        if injected {
            // Add new token
            let count = run.current_marking.entry(request.place_id.clone()).or_insert(0);
            *count += 1;
        }

        // Set/merge token data
        let existing = run.token_data.entry(request.place_id.clone()).or_insert_with(|| serde_json::json!({}));
        if let (Some(existing_obj), Some(new_obj)) = (existing.as_object_mut(), request.data.as_object()) {
            for (k, v) in new_obj {
                existing_obj.insert(k.clone(), v.clone());
            }
        } else {
            *existing = request.data.clone();
        }

        let count = run.current_marking.get(&request.place_id).copied().unwrap_or(0);
        info!(
            place_id = %request.place_id,
            injected = injected,
            token_count = count,
            data = ?run.token_data.get(&request.place_id),
            "Token set for place"
        );

        (count, run.token_data.clone())
    };

    // Get updated marking
    let new_marking = {
        let runs = state.runs.read().await;
        runs.get(&run_id).map(|r| r.current_marking.clone()).unwrap_or_default()
    };

    // Re-evaluate guards and find enabled transitions
    let enabled_transitions = find_enabled_transitions(&workflow_def, &new_marking, &token_data);
    let waiting_transitions = find_waiting_transitions(&workflow_def, &new_marking, &token_data);

    info!(
        run_id = %run_id,
        place_id = %request.place_id,
        injected = injected,
        enabled = ?enabled_transitions,
        waiting = ?waiting_transitions,
        "Guards evaluated after set_token"
    );

    // Dispatch tasks for enabled transitions
    if let Some(ref nats) = state.nats {
        for transition_id in &enabled_transitions {
            if let Some(transition) = workflow_def
                .transitions
                .iter()
                .find(|t| &t.id == transition_id)
            {
                let task_id = Uuid::new_v4();
                let action_json = serde_json::to_value(&transition.action).unwrap_or_default();

                let resources = transition.resources.as_ref().map(|r| TaskResources {
                    cpu: r.cpu.clone(),
                    memory: r.memory.clone(),
                });

                // Build input tokens with current token data
                let input_tokens: Vec<TokenData> = transition.inputs.iter().filter_map(|input| {
                    token_data.get(&input.place).map(|data| {
                        TokenData::with_data(input.place.clone(), data.clone())
                    })
                }).collect();

                let task = TaskDispatchMessage {
                    task_id,
                    transition_ref: TransitionRef {
                        transition_id: transition.id.clone(),
                        run_id,
                        execution_id: Some(task_id),
                    },
                    action: action_json,
                    resources,
                    timeout: Some(transition.timeout.clone()),
                    runner_pool: "default".to_string(),
                    environment: HashMap::new(),
                    input_tokens,
                    policy: None,
                };

                let subject = cb_nats::subjects::transition_enabled(&run_id, &transition.id);

                if let Err(e) = nats.publish_jetstream(&subject, &task).await {
                    error!(error = %e, "Failed to dispatch task");
                } else {
                    info!(task_id = %task_id, transition_id = %transition.id, "Task dispatched after set_token");
                }
            }
        }
    }

    Ok(Json(ResumeResponse {
        run_id,
        place_id: request.place_id,
        token_count,
        injected,
        enabled_transitions,
        waiting_transitions,
    }))
}

/// Get schema information for all places in a run.
async fn describe_places(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<DescribePlacesResponse>, ApiError> {
    let runs = state.runs.read().await;
    let run = runs
        .get(&run_id)
        .ok_or_else(|| ApiError::not_found(format!("Run {} not found", run_id)))?;

    let workflows = state.workflows.read().await;
    let workflow = workflows
        .get(&run.workflow_id)
        .ok_or_else(|| ApiError::not_found("Workflow not found"))?;

    let places: Vec<PlaceSchemaInfo> = workflow
        .definition
        .places
        .iter()
        .map(|p| {
            let token_count = run.current_marking.get(&p.id).copied().unwrap_or(0);
            PlaceSchemaInfo {
                place_id: p.id.clone(),
                token_schema: p.token_schema.clone(),
                token_count,
                requires_data: p.token_schema.is_some(),
            }
        })
        .collect();

    Ok(Json(DescribePlacesResponse {
        run_id,
        workflow_name: run.workflow_name.clone(),
        places,
    }))
}

/// Cancel a run.
async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(_request): Json<CancelRunRequest>,
) -> Result<StatusCode, ApiError> {
    let mut runs = state.runs.write().await;

    let run = runs
        .get_mut(&run_id)
        .ok_or_else(|| ApiError::not_found(format!("Run {} not found", run_id)))?;

    if matches!(
        run.status,
        RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
    ) {
        return Err(ApiError::conflict(format!(
            "Run {} is already in terminal state {:?}",
            run_id, run.status
        )));
    }

    run.status = RunStatus::Cancelled;
    run.completed_at = Some(chrono::Utc::now());

    info!(%run_id, "Run cancelled");

    Ok(StatusCode::NO_CONTENT)
}

// ============ Error Handling ============

/// API error response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    /// Error code.
    pub code: String,
    /// Error message.
    pub message: String,
    /// Additional details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// API error type.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    /// Create a not found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: message.into(),
        }
    }

    /// Create a conflict error.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "CONFLICT".to_string(),
            message: message.into(),
        }
    }

    /// Create a bad request error.
    #[allow(dead_code)]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "BAD_REQUEST".to_string(),
            message: message.into(),
        }
    }

    /// Create an internal server error.
    #[allow(dead_code)]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = ApiErrorResponse {
            code: self.code,
            message: self.message,
            details: None,
        };
        (self.status, Json(body)).into_response()
    }
}

// ============ Router ============

/// Build the API router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    let health_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/version", get(version_handler));

    let workflow_routes = Router::new()
        .route("/", post(submit_workflow).get(list_workflows))
        .route("/{workflow_id}", get(get_workflow).delete(delete_workflow))
        .route("/{workflow_id}/runs", post(run_workflow));

    let run_routes = Router::new()
        .route("/", get(list_runs))
        .route("/{run_id}", get(get_run_status))
        .route("/{run_id}/logs", get(get_run_logs))
        .route("/{run_id}/cancel", post(cancel_run))
        .route("/{run_id}/inject", post(inject_token))
        .route("/{run_id}/update", post(update_token))
        .route("/{run_id}/resume", post(resume_workflow))
        .route("/{run_id}/places", get(describe_places));

    let api_v1 = Router::new()
        .nest("/workflows", workflow_routes)
        .nest("/runs", run_routes);

    Router::new()
        .merge(health_routes)
        .nest("/api/v1", api_v1)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Start the API server.
///
/// # Errors
///
/// Returns an error if the server fails to bind or encounters a runtime error.
pub async fn serve(config: ApiConfig, state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let addr = SocketAddr::new(config.host, config.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("API server listening on {}", addr);

    // Start event subscriber in background
    let subscriber_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = start_event_subscriber(subscriber_state).await {
            error!(error = %e, "Event subscriber failed");
        }
    });

    let router = build_router(state);

    axum::serve(listener, router).await?;

    Ok(())
}

/// Initialize NATS streams required for the workflow engine.
pub async fn init_nats_streams(nats: &NatsClient) -> Result<(), Box<dyn std::error::Error>> {
    let js = nats.jetstream();

    // Create RUNS stream for task dispatch and transition events
    let stream_config = async_nats::jetstream::stream::Config {
        name: streams::RUNS.to_string(),
        subjects: vec!["cb.runs.>".to_string()],
        retention: async_nats::jetstream::stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
        storage: async_nats::jetstream::stream::StorageType::File,
        ..Default::default()
    };

    match js.get_or_create_stream(stream_config).await {
        Ok(stream) => {
            info!(stream = %stream.cached_info().config.name, "RUNS stream ready");
        }
        Err(e) => {
            warn!(error = %e, "Failed to create RUNS stream");
        }
    }

    // Create WORKFLOWS stream for workflow lifecycle events
    let workflow_stream_config = async_nats::jetstream::stream::Config {
        name: streams::WORKFLOWS.to_string(),
        subjects: vec!["cb.workflows.>".to_string()],
        retention: async_nats::jetstream::stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(30 * 24 * 60 * 60), // 30 days
        storage: async_nats::jetstream::stream::StorageType::File,
        ..Default::default()
    };

    match js.get_or_create_stream(workflow_stream_config).await {
        Ok(stream) => {
            info!(stream = %stream.cached_info().config.name, "WORKFLOWS stream ready");
        }
        Err(e) => {
            warn!(error = %e, "Failed to create WORKFLOWS stream");
        }
    }

    Ok(())
}

/// Connect to NATS and initialize the API state.
pub async fn connect_nats(nats_url: &str) -> Result<NatsClient, Box<dyn std::error::Error>> {
    let config = NatsConfig {
        urls: vec![nats_url.to_string()],
        name: Some("cb-api".to_string()),
        ..Default::default()
    };

    let client = NatsClient::connect(&config).await?;
    info!("Connected to NATS at {}", nats_url);

    Ok(client)
}

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::{
        build_router, connect_nats, init_nats_streams, serve, start_event_subscriber, ApiConfig,
        AppState,
    };
}
