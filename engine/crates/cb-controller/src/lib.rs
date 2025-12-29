//! # Circuit Breaker Kubernetes Controller
//!
//! Kubernetes operator for Circuit Breaker using kube-rs.
//!
//! This crate provides:
//! - Custom Resource Definitions (CRDs) for workflows and runs
//! - Reconciliation loops for workflow execution
//! - Integration with Karpenter for autoscaling runner pods
//! - Runner pod lifecycle management
//!
//! ## Custom Resources
//!
//! - `Workflow`: A workflow definition (Petri net)
//! - `WorkflowRun`: An execution instance of a workflow
//! - `Runner`: A Dagger runner pod specification
//!
//! ## Architecture
//!
//! The controller watches for changes to Circuit Breaker CRDs and:
//! 1. Validates workflow definitions
//! 2. Creates/updates WorkflowRun resources when triggered
//! 3. Spawns runner pods to execute transitions
//! 4. Updates status based on execution results
//! 5. Cleans up completed/failed resources

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Labels applied to Circuit Breaker resources.
pub mod labels {
    /// Label for the workflow name.
    pub const WORKFLOW_NAME: &str = "circuit-breaker.io/workflow-name";
    /// Label for the workflow namespace.
    pub const WORKFLOW_NAMESPACE: &str = "circuit-breaker.io/workflow-namespace";
    /// Label for the run ID.
    pub const RUN_ID: &str = "circuit-breaker.io/run-id";
    /// Label for the transition ID.
    pub const TRANSITION_ID: &str = "circuit-breaker.io/transition-id";
    /// Label indicating a runner pod.
    pub const RUNNER: &str = "circuit-breaker.io/runner";
    /// Label for the runner pool.
    pub const RUNNER_POOL: &str = "circuit-breaker.io/runner-pool";
}

/// Annotations applied to Circuit Breaker resources.
pub mod annotations {
    /// Annotation for the workflow definition hash.
    pub const WORKFLOW_HASH: &str = "circuit-breaker.io/workflow-hash";
    /// Annotation for the last reconciled time.
    pub const LAST_RECONCILED: &str = "circuit-breaker.io/last-reconciled";
    /// Annotation for Karpenter node pool selection.
    pub const KARPENTER_NODE_POOL: &str = "karpenter.sh/nodepool";
}

/// Finalizers used by the controller.
pub mod finalizers {
    /// Finalizer for workflow cleanup.
    pub const WORKFLOW: &str = "circuit-breaker.io/workflow-finalizer";
    /// Finalizer for run cleanup.
    pub const RUN: &str = "circuit-breaker.io/run-finalizer";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_labels() {
        assert!(labels::WORKFLOW_NAME.starts_with("circuit-breaker.io/"));
        assert!(labels::RUN_ID.starts_with("circuit-breaker.io/"));
    }

    #[test]
    fn test_finalizers() {
        assert!(finalizers::WORKFLOW.starts_with("circuit-breaker.io/"));
        assert!(finalizers::RUN.starts_with("circuit-breaker.io/"));
    }
}
