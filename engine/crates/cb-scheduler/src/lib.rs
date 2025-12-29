//! # Circuit Breaker Scheduler
//!
//! Task scheduling and dispatch for Circuit Breaker workflow execution.
//!
//! This crate provides:
//! - Task queue management
//! - Priority-based scheduling
//! - Resource-aware dispatch
//! - Timeout handling
//! - Retry logic with backoff
//!
//! ## Architecture
//!
//! The scheduler receives enabled transitions from the engine and dispatches
//! them as tasks to runner pods via NATS. It handles:
//!
//! 1. **Task Queue**: Priority queue for pending tasks
//! 2. **Resource Matching**: Match task requirements to runner capabilities
//! 3. **Dispatch**: Send tasks to appropriate runner pools
//! 4. **Tracking**: Monitor task execution and timeouts
//! 5. **Retries**: Handle failures with configurable backoff

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task status in the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is queued waiting for dispatch.
    Queued,
    /// Task has been dispatched to a runner.
    Dispatched,
    /// Task is currently executing.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed and may be retried.
    Failed,
    /// Task failed and will be retried.
    Retrying,
    /// Task timed out.
    TimedOut,
    /// Task was cancelled.
    Cancelled,
}

impl TaskStatus {
    /// Check if this is a terminal status.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::TimedOut | Self::Cancelled
        )
    }

    /// Check if task is still active (not terminal).
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

/// Task priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    /// Low priority tasks.
    Low = 0,
    /// Normal priority (default).
    Normal = 50,
    /// High priority tasks.
    High = 80,
    /// Critical priority tasks.
    Critical = 100,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

impl From<u8> for TaskPriority {
    fn from(value: u8) -> Self {
        match value {
            0..=25 => Self::Low,
            26..=60 => Self::Normal,
            61..=90 => Self::High,
            _ => Self::Critical,
        }
    }
}

/// A task to be scheduled and executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier.
    pub id: Uuid,
    /// Run ID this task belongs to.
    pub run_id: Uuid,
    /// Transition ID being executed.
    pub transition_id: String,
    /// Task priority.
    pub priority: TaskPriority,
    /// Current status.
    pub status: TaskStatus,
    /// Action to execute (serialized).
    pub action: serde_json::Value,
    /// Resource requirements.
    pub resources: Option<TaskResources>,
    /// Execution timeout.
    pub timeout: Duration,
    /// Number of retry attempts.
    pub max_retries: u8,
    /// Current attempt number.
    pub attempt: u8,
    /// Runner pool to dispatch to.
    pub runner_pool: String,
    /// When the task was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the task was last updated.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Task {
    /// Create a new task.
    #[must_use]
    pub fn new(run_id: Uuid, transition_id: impl Into<String>, action: serde_json::Value) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            run_id,
            transition_id: transition_id.into(),
            priority: TaskPriority::default(),
            status: TaskStatus::Queued,
            action,
            resources: None,
            timeout: Duration::from_secs(300), // 5 minutes default
            max_retries: 0,
            attempt: 1,
            runner_pool: "default".to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Set task priority.
    #[must_use]
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set task timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set max retries.
    #[must_use]
    pub fn with_retries(mut self, max_retries: u8) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set runner pool.
    #[must_use]
    pub fn with_runner_pool(mut self, pool: impl Into<String>) -> Self {
        self.runner_pool = pool.into();
        self
    }

    /// Set resource requirements.
    #[must_use]
    pub fn with_resources(mut self, resources: TaskResources) -> Self {
        self.resources = Some(resources);
        self
    }

    /// Check if task can be retried.
    #[must_use]
    pub fn can_retry(&self) -> bool {
        self.attempt <= self.max_retries
    }
}

/// Resource requirements for a task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskResources {
    /// CPU request (e.g., "100m", "2").
    pub cpu: Option<String>,
    /// Memory request (e.g., "256Mi", "4Gi").
    pub memory: Option<String>,
    /// GPU request.
    pub gpu: Option<String>,
}

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Maximum concurrent tasks.
    pub max_concurrent: usize,
    /// Default task timeout.
    pub default_timeout: Duration,
    /// How often to check for timed out tasks.
    pub timeout_check_interval: Duration,
    /// Base delay for exponential backoff.
    pub retry_base_delay: Duration,
    /// Maximum delay for exponential backoff.
    pub retry_max_delay: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 100,
            default_timeout: Duration::from_secs(300),
            timeout_check_interval: Duration::from_secs(10),
            retry_base_delay: Duration::from_secs(1),
            retry_max_delay: Duration::from_secs(300),
        }
    }
}

/// Calculate exponential backoff delay.
#[must_use]
pub fn calculate_backoff(attempt: u8, base_delay: Duration, max_delay: Duration) -> Duration {
    let delay = base_delay.saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1) as u32));
    delay.min(max_delay)
}

/// Prelude for convenient imports.
pub mod prelude {
    pub use crate::{
        calculate_backoff, SchedulerConfig, Task, TaskPriority, TaskResources, TaskStatus,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_terminal() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::TimedOut.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());

        assert!(!TaskStatus::Queued.is_terminal());
        assert!(!TaskStatus::Dispatched.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Retrying.is_terminal());
    }

    #[test]
    fn test_task_status_active() {
        assert!(TaskStatus::Queued.is_active());
        assert!(TaskStatus::Running.is_active());
        assert!(!TaskStatus::Completed.is_active());
    }

    #[test]
    fn test_task_priority_from_u8() {
        assert_eq!(TaskPriority::from(0), TaskPriority::Low);
        assert_eq!(TaskPriority::from(50), TaskPriority::Normal);
        assert_eq!(TaskPriority::from(80), TaskPriority::High);
        assert_eq!(TaskPriority::from(100), TaskPriority::Critical);
    }

    #[test]
    fn test_task_creation() {
        let task = Task::new(
            Uuid::new_v4(),
            "my-transition",
            serde_json::json!({"type": "noop"}),
        );

        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.priority, TaskPriority::Normal);
        assert_eq!(task.attempt, 1);
    }

    #[test]
    fn test_task_can_retry() {
        let mut task = Task::new(Uuid::new_v4(), "test", serde_json::json!({})).with_retries(3);

        assert!(task.can_retry());

        task.attempt = 4;
        assert!(!task.can_retry());
    }

    #[test]
    fn test_calculate_backoff() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(60);

        assert_eq!(calculate_backoff(1, base, max), Duration::from_secs(1));
        assert_eq!(calculate_backoff(2, base, max), Duration::from_secs(2));
        assert_eq!(calculate_backoff(3, base, max), Duration::from_secs(4));
        assert_eq!(calculate_backoff(4, base, max), Duration::from_secs(8));
        assert_eq!(calculate_backoff(10, base, max), Duration::from_secs(60)); // capped at max
    }
}
