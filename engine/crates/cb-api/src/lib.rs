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

#![forbid(unsafe_code)]
#![warn(missing_docs)]

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
use cb_core::events::{RunRef, TokenData, TokenInjectedPayload, TransitionRef};
use cb_core::workflow::Workflow;
use cb_nats::{streams, NatsClient, NatsConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
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
            port: 8080,
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
    /// Stored workflows.
    pub workflows: Arc<RwLock<HashMap<Uuid, StoredWorkflow>>>,
    /// Workflow runs.
    pub runs: Arc<RwLock<HashMap<Uuid, WorkflowRun>>>,
    /// Task logs indexed by run_id.
    pub task_logs: Arc<RwLock<HashMap<Uuid, Vec<TaskLog>>>>,
    /// NATS client.
    pub nats: Option<Arc<NatsClient>>,
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
            shutdown_tx: None,
        }
    }
}

impl AppState {
    /// Create a new AppState with NATS client.
    pub fn with_nats(nats: NatsClient) -> Self {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            task_logs: Arc::new(RwLock::new(HashMap::new())),
            nats: Some(Arc::new(nats)),
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

    // Get workflow definition to find arcs
    let workflow_opt = {
        let runs = state.runs.read().await;
        if let Some(run) = runs.get(&run_id) {
            let workflows = state.workflows.read().await;
            workflows.get(&run.workflow_id).map(|w| w.definition.clone())
        } else {
            None
        }
    };

    let workflow = match workflow_opt {
        Some(w) => w,
        None => {
            warn!(run_id = %run_id, "Workflow not found for run");
            return;
        }
    };

    // Find the transition that completed
    let transition = match workflow.transitions.iter().find(|t| t.id == transition_id) {
        Some(t) => t.clone(),
        None => {
            warn!(run_id = %run_id, transition_id = %transition_id, "Transition not found in workflow");
            return;
        }
    };

    let mut runs = state.runs.write().await;

    // Collect info needed for task dispatch before holding the lock
    let (enabled_transitions, nats_client) = {
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

            // Update marking according to Petri net semantics:
            // 1. Consume tokens from input places (already done when transition fired)
            // 2. Produce tokens to output places
            for arc in &transition.outputs {
                let count = run.current_marking.entry(arc.place.clone()).or_insert(0);
                *count += arc.weight;
                info!(
                    run_id = %run_id,
                    transition_id = %transition_id,
                    place = %arc.place,
                    tokens = *count,
                    "Produced token to output place"
                );
            }

            // Check if workflow is complete (all transitions completed)
            let all_complete = run.transitions.iter().all(|t| t.status == "completed");
            if all_complete {
                run.status = RunStatus::Completed;
                run.completed_at = Some(now);
                info!(run_id = %run_id, "Workflow run completed");
                (vec![], state.nats.clone())
            } else {
                // Find newly enabled transitions
                let enabled = find_enabled_transitions(&workflow, &run.current_marking);
                // Filter out already running or completed transitions
                let pending_enabled: Vec<String> = enabled
                    .into_iter()
                    .filter(|tid| {
                        run.transitions
                            .iter()
                            .find(|t| &t.transition_id == tid)
                            .map(|t| t.status == "pending")
                            .unwrap_or(false)
                    })
                    .collect();
                (pending_enabled, state.nats.clone())
            }
        } else {
            (vec![], None)
        }
    };

    // Dispatch tasks for newly enabled transitions (outside the lock)
    if !enabled_transitions.is_empty() {
        if let Some(nats) = nats_client {
            for tid in enabled_transitions {
                // Mark as running before dispatch
                if let Some(run) = runs.get_mut(&run_id) {
                    if let Some(ts) = run.transitions.iter_mut().find(|t| t.transition_id == tid) {
                        ts.status = "running".to_string();
                        ts.started_at = Some(now);
                    }
                }

                // Find the transition in the workflow
                if let Some(t) = workflow.transitions.iter().find(|t| t.id == tid) {
                    // Consume tokens from input places
                    if let Some(run) = runs.get_mut(&run_id) {
                        for arc in &t.inputs {
                            if let Some(count) = run.current_marking.get_mut(&arc.place) {
                                *count = count.saturating_sub(arc.weight);
                            }
                        }
                    }

                    // Dispatch task
                    let task_id = Uuid::new_v4();
                    let task = TaskDispatchMessage {
                        task_id,
                        transition_ref: TransitionRef {
                            run_id,
                            transition_id: tid.clone(),
                            execution_id: Some(task_id),
                        },
                        action: serde_json::to_value(&t.action).unwrap_or_default(),
                        resources: None,
                        timeout: None,
                        runner_pool: "default".to_string(),
                        environment: HashMap::new(),
                        input_tokens: vec![],
                    };

                    let subject = format!("cb.runs.{}.transitions.{}.enabled", run_id, tid);
                    // publish_jetstream does its own serialization, so pass the task directly
                    if let Err(e) = nats.publish_jetstream(&subject, &task).await {
                        error!(error = %e, run_id = %run_id, transition_id = %tid, "Failed to dispatch task");
                    } else {
                        info!(run_id = %run_id, transition_id = %tid, task_id = %task_id, "Dispatched task for enabled transition");
                    }
                }
            }
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
        transitions,
        error: None,
    };

    let response = RunWorkflowResponse {
        run_id,
        workflow_id,
        status: RunStatus::Running,
        started_at: now,
    };

    // Find enabled transitions and dispatch tasks
    let enabled_transitions = find_enabled_transitions(&workflow.definition, &initial_marking);

    // Clone what we need before releasing the lock
    let definition = workflow.definition.clone();
    drop(workflows);

    // Store the run
    {
        let mut runs = state.runs.write().await;
        runs.insert(run_id, run);
    }

    info!(%run_id, %workflow_id, "Workflow run started");

    // Dispatch tasks to NATS for enabled transitions
    if let Some(ref nats) = state.nats {
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

/// Find enabled transitions based on current marking.
fn find_enabled_transitions(workflow: &Workflow, marking: &HashMap<String, u32>) -> Vec<String> {
    workflow
        .transitions
        .iter()
        .filter(|t| {
            // A transition is enabled if all input places have enough tokens
            t.inputs.iter().all(|arc| {
                let tokens = marking.get(&arc.place).copied().unwrap_or(0);
                tokens >= arc.weight
            })
        })
        .map(|t| t.id.clone())
        .collect()
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

    // Update marking
    let new_token_count = {
        let mut runs = state.runs.write().await;
        let run = runs.get_mut(&run_id).unwrap();
        let count = run
            .current_marking
            .entry(request.place_id.clone())
            .or_insert(0);
        *count += 1;
        *count
    };

    // Find newly enabled transitions
    let mut new_marking = current_marking.clone();
    *new_marking.entry(request.place_id.clone()).or_insert(0) += 1;
    let enabled_transitions = find_enabled_transitions(&workflow_def, &new_marking);

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

#[cfg(test)]
mod tests {
    use super::*;
    use cb_core::workflow::{Action, Arc as WfArc, Place, Transition, Workflow};

    fn create_sequential_workflow() -> Workflow {
        // start -> [t1] -> middle -> [t2] -> end
        Workflow {
            version: "1.0".to_string(),
            name: "sequential-test".to_string(),
            namespace: "test".to_string(),
            metadata: None,
            places: vec![
                Place {
                    id: "start".to_string(),
                    initial_tokens: 1,
                    capacity: None,
                    token_schema: None,
                },
                Place {
                    id: "middle".to_string(),
                    initial_tokens: 0,
                    capacity: None,
                    token_schema: None,
                },
                Place {
                    id: "end".to_string(),
                    initial_tokens: 0,
                    capacity: None,
                    token_schema: None,
                },
            ],
            transitions: vec![
                Transition {
                    id: "t1".to_string(),
                    inputs: vec![WfArc {
                        place: "start".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    outputs: vec![WfArc {
                        place: "middle".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    guard: None,
                    action: Action::Noop,
                    resources: None,
                    timeout: "5m".to_string(),
                    retries: 0,
                    retry_backoff: cb_core::workflow::RetryBackoff::Exponential,
                    priority: 50,
                },
                Transition {
                    id: "t2".to_string(),
                    inputs: vec![WfArc {
                        place: "middle".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    outputs: vec![WfArc {
                        place: "end".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    guard: None,
                    action: Action::Noop,
                    resources: None,
                    timeout: "5m".to_string(),
                    retries: 0,
                    retry_backoff: cb_core::workflow::RetryBackoff::Exponential,
                    priority: 50,
                },
            ],
        }
    }

    #[test]
    fn test_find_enabled_transitions_initial() {
        let workflow = create_sequential_workflow();
        let mut marking = HashMap::new();
        marking.insert("start".to_string(), 1);

        let enabled = find_enabled_transitions(&workflow, &marking);

        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0], "t1");
    }

    #[test]
    fn test_find_enabled_transitions_after_t1() {
        let workflow = create_sequential_workflow();
        let mut marking = HashMap::new();
        // After t1 fires: token moves from start to middle
        marking.insert("start".to_string(), 0);
        marking.insert("middle".to_string(), 1);

        let enabled = find_enabled_transitions(&workflow, &marking);

        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0], "t2");
    }

    #[test]
    fn test_find_enabled_transitions_none_enabled() {
        let workflow = create_sequential_workflow();
        let mut marking = HashMap::new();
        // After t2 fires: token in end, nothing enabled
        marking.insert("end".to_string(), 1);

        let enabled = find_enabled_transitions(&workflow, &marking);

        assert!(enabled.is_empty());
    }

    #[test]
    fn test_find_enabled_transitions_insufficient_tokens() {
        let workflow = create_sequential_workflow();
        let marking = HashMap::new(); // No tokens anywhere

        let enabled = find_enabled_transitions(&workflow, &marking);

        assert!(enabled.is_empty());
    }

    fn create_parallel_workflow() -> Workflow {
        // start -> [t1] -> a
        //      \-> [t2] -> b
        // Both t1 and t2 consume from start (need 2 tokens)
        Workflow {
            version: "1.0".to_string(),
            name: "parallel-test".to_string(),
            namespace: "test".to_string(),
            metadata: None,
            places: vec![
                Place {
                    id: "start".to_string(),
                    initial_tokens: 2,
                    capacity: None,
                    token_schema: None,
                },
                Place {
                    id: "a".to_string(),
                    initial_tokens: 0,
                    capacity: None,
                    token_schema: None,
                },
                Place {
                    id: "b".to_string(),
                    initial_tokens: 0,
                    capacity: None,
                    token_schema: None,
                },
            ],
            transitions: vec![
                Transition {
                    id: "t1".to_string(),
                    inputs: vec![WfArc {
                        place: "start".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    outputs: vec![WfArc {
                        place: "a".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    guard: None,
                    action: Action::Noop,
                    resources: None,
                    timeout: "5m".to_string(),
                    retries: 0,
                    retry_backoff: cb_core::workflow::RetryBackoff::Exponential,
                    priority: 50,
                },
                Transition {
                    id: "t2".to_string(),
                    inputs: vec![WfArc {
                        place: "start".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    outputs: vec![WfArc {
                        place: "b".to_string(),
                        weight: 1,
                        expression: None,
                    }],
                    guard: None,
                    action: Action::Noop,
                    resources: None,
                    timeout: "5m".to_string(),
                    retries: 0,
                    retry_backoff: cb_core::workflow::RetryBackoff::Exponential,
                    priority: 50,
                },
            ],
        }
    }

    #[test]
    fn test_find_enabled_transitions_parallel() {
        let workflow = create_parallel_workflow();
        let mut marking = HashMap::new();
        marking.insert("start".to_string(), 2);

        let enabled = find_enabled_transitions(&workflow, &marking);

        // Both t1 and t2 should be enabled
        assert_eq!(enabled.len(), 2);
        assert!(enabled.contains(&"t1".to_string()));
        assert!(enabled.contains(&"t2".to_string()));
    }

    #[test]
    fn test_find_enabled_transitions_partial_parallel() {
        let workflow = create_parallel_workflow();
        let mut marking = HashMap::new();
        // Only 1 token, both transitions need 1, but only one can fire
        marking.insert("start".to_string(), 1);

        let enabled = find_enabled_transitions(&workflow, &marking);

        // Both are still "enabled" (have enough tokens individually)
        // The actual conflict resolution happens at dispatch time
        assert_eq!(enabled.len(), 2);
    }
}
