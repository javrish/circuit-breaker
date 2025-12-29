//! # Circuit Breaker Petri Net Engine
//!
//! The core execution engine for Circuit Breaker workflows.
//!
//! This crate provides:
//! - Async workflow execution loop
//! - Token management and marking state
//! - Transition firing logic with guard evaluation
//! - Event sourcing for state recovery
//! - Concurrent transition execution
//!
//! ## Architecture
//!
//! The engine operates as an event-sourced state machine:
//!
//! 1. Load workflow definition
//! 2. Initialize marking from initial tokens
//! 3. Find enabled transitions
//! 4. Fire transitions (dispatch to scheduler)
//! 5. Update marking based on results
//! 6. Emit events for all state changes
//! 7. Repeat until no transitions enabled or terminal state

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use cb_core::petri::Marking;
use cb_core::workflow::Workflow;
use cb_core::error::{Error, ErrorKind, Result};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Configuration for the workflow engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Maximum concurrent transitions per run.
    pub max_concurrent_transitions: usize,
    /// Enable detailed execution tracing.
    pub enable_tracing: bool,
    /// Snapshot interval for state persistence (in events).
    pub snapshot_interval: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_transitions: 10,
            enable_tracing: true,
            snapshot_interval: 100,
        }
    }
}

/// Status of a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Run is pending start.
    Pending,
    /// Run is actively executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed with an error.
    Failed,
    /// Run was cancelled.
    Cancelled,
}

impl RunStatus {
    /// Check if this is a terminal status.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// State of a single workflow run.
#[derive(Debug)]
pub struct RunState {
    /// Unique identifier for this run.
    pub run_id: Uuid,
    /// The workflow being executed.
    pub workflow: Workflow,
    /// Current marking (token distribution).
    pub marking: Marking,
    /// Current status.
    pub status: RunStatus,
    /// Error message if failed.
    pub error: Option<String>,
    /// Number of events processed.
    pub event_count: u64,
}

impl RunState {
    /// Create a new run state from a workflow.
    #[must_use]
    pub fn new(workflow: Workflow) -> Self {
        let marking = Marking::from_workflow(&workflow);
        Self {
            run_id: Uuid::new_v4(),
            workflow,
            marking,
            status: RunStatus::Pending,
            error: None,
            event_count: 0,
        }
    }

    /// Create a new run state with a specific run ID.
    #[must_use]
    pub fn with_id(run_id: Uuid, workflow: Workflow) -> Self {
        let marking = Marking::from_workflow(&workflow);
        Self {
            run_id,
            workflow,
            marking,
            status: RunStatus::Pending,
            error: None,
            event_count: 0,
        }
    }
}

/// Handle to interact with a running engine.
#[derive(Clone)]
pub struct EngineHandle {
    runs: Arc<RwLock<HashMap<Uuid, RunState>>>,
}

impl EngineHandle {
    /// Get the status of a run.
    pub async fn get_run_status(&self, run_id: Uuid) -> Result<RunStatus> {
        let runs = self.runs.read().await;
        runs.get(&run_id)
            .map(|r| r.status)
            .ok_or_else(|| Error::run_not_found(run_id.to_string()))
    }

    /// Get the current marking of a run.
    pub async fn get_marking(&self, run_id: Uuid) -> Result<HashMap<String, usize>> {
        let runs = self.runs.read().await;
        runs.get(&run_id)
            .map(|r| r.marking.as_counts())
            .ok_or_else(|| Error::run_not_found(run_id.to_string()))
    }
}

/// The main workflow execution engine.
pub struct Engine {
    config: EngineConfig,
    runs: Arc<RwLock<HashMap<Uuid, RunState>>>,
}

impl Engine {
    /// Create a new engine with the given configuration.
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            runs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new engine with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(EngineConfig::default())
    }

    /// Get a handle to interact with the engine.
    #[must_use]
    pub fn handle(&self) -> EngineHandle {
        EngineHandle {
            runs: Arc::clone(&self.runs),
        }
    }

    /// Start a new workflow run.
    ///
    /// Returns the run ID for the new execution.
    pub async fn start_run(&self, workflow: Workflow) -> Result<Uuid> {
        let mut state = RunState::new(workflow);
        state.status = RunStatus::Running;
        let run_id = state.run_id;

        let mut runs = self.runs.write().await;
        runs.insert(run_id, state);

        tracing::info!(%run_id, "Started workflow run");

        Ok(run_id)
    }

    /// Start a new workflow run with a specific run ID.
    pub async fn start_run_with_id(&self, run_id: Uuid, workflow: Workflow) -> Result<Uuid> {
        let mut state = RunState::with_id(run_id, workflow);
        state.status = RunStatus::Running;

        let mut runs = self.runs.write().await;
        if runs.contains_key(&run_id) {
            return Err(Error::with_message(
                ErrorKind::WorkflowExists,
                format!("Run with ID {} already exists", run_id),
            ));
        }
        runs.insert(run_id, state);

        tracing::info!(%run_id, "Started workflow run");

        Ok(run_id)
    }

    /// Cancel a running workflow.
    pub async fn cancel_run(&self, run_id: Uuid) -> Result<()> {
        let mut runs = self.runs.write().await;
        let state = runs
            .get_mut(&run_id)
            .ok_or_else(|| Error::run_not_found(run_id.to_string()))?;

        if state.status.is_terminal() {
            return Err(Error::with_message(
                ErrorKind::InvalidWorkflowState,
                format!("Run {} is already in terminal state {:?}", run_id, state.status),
            ));
        }

        state.status = RunStatus::Cancelled;
        tracing::info!(%run_id, "Cancelled workflow run");

        Ok(())
    }

    /// Get the configuration.
    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}

/// Prelude for common imports.
pub mod prelude {
    pub use crate::{Engine, EngineConfig, EngineHandle, RunState, RunStatus};
    pub use cb_core::prelude::*;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_core::workflow::{Action, Arc as WfArc, Place, Transition};

    fn create_test_workflow() -> Workflow {
        Workflow {
            version: "1.0".to_string(),
            name: "test-workflow".to_string(),
            namespace: "default".to_string(),
            metadata: None,
            places: vec![
                Place {
                    id: "start".to_string(),
                    initial_tokens: 1,
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
            transitions: vec![Transition {
                id: "process".to_string(),
                inputs: vec![WfArc {
                    place: "start".to_string(),
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
            }],
        }
    }

    #[tokio::test]
    async fn test_engine_creation() {
        let engine = Engine::with_defaults();
        assert_eq!(engine.config().max_concurrent_transitions, 10);
    }

    #[tokio::test]
    async fn test_start_run() {
        let engine = Engine::with_defaults();
        let workflow = create_test_workflow();

        let run_id = engine.start_run(workflow).await.unwrap();
        let handle = engine.handle();

        let status = handle.get_run_status(run_id).await.unwrap();
        assert_eq!(status, RunStatus::Running);
    }

    #[tokio::test]
    async fn test_cancel_run() {
        let engine = Engine::with_defaults();
        let workflow = create_test_workflow();

        let run_id = engine.start_run(workflow).await.unwrap();
        engine.cancel_run(run_id).await.unwrap();

        let handle = engine.handle();
        let status = handle.get_run_status(run_id).await.unwrap();
        assert_eq!(status, RunStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_get_marking() {
        let engine = Engine::with_defaults();
        let workflow = create_test_workflow();

        let run_id = engine.start_run(workflow).await.unwrap();
        let handle = engine.handle();

        let marking = handle.get_marking(run_id).await.unwrap();
        assert_eq!(marking.get("start"), Some(&1));
        assert_eq!(marking.get("end"), None);
    }

    #[test]
    fn test_run_status_terminal() {
        assert!(!RunStatus::Pending.is_terminal());
        assert!(!RunStatus::Running.is_terminal());
        assert!(RunStatus::Completed.is_terminal());
        assert!(RunStatus::Failed.is_terminal());
        assert!(RunStatus::Cancelled.is_terminal());
    }
}
