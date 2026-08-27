//! # Engine Module
//!
//! This module provides the hybrid execution engine system for Circuit Breaker.
//! It enables workflows to execute using either local Dagger installations or
//! remote engines provisioned by the Engine Service.
//!
//! ## Module Structure
//!
//! - [`error`] - Error types for engine operations
//! - [`detect`] - Runtime detection for Dagger and container runtimes
//! - [`local`] - Local engine implementation using local Dagger
//! - [`selector`] - Engine selection logic for auto mode
//!
//! ## Architecture
//!
//! The engine system follows a layered architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     EngineSelector                          │
//! │  Chooses between local and cloud based on requirements      │
//! └─────────────────────────┬───────────────────────────────────┘
//!                           │
//!           ┌───────────────┴───────────────┐
//!           │                               │
//!           ▼                               ▼
//! ┌─────────────────────┐         ┌─────────────────────┐
//! │    LocalEngine      │         │    CloudEngine      │
//! │                     │         │    (future)         │
//! │  Uses local Dagger  │         │  Uses Engine        │
//! │  and container      │         │  Service API        │
//! │  runtime            │         │                     │
//! └─────────────────────┘         └─────────────────────┘
//!           │
//!           ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    RuntimeDetector                          │
//! │  Detects Dagger installation and container runtime          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ### Basic Local Execution
//!
//! ```rust,no_run
//! use cb_runner::engine::{LocalEngine, EngineExecutor};
//! use cb_core::engine::LocalEngineConfig;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a local engine with default configuration
//! let config = LocalEngineConfig::default();
//! let engine = LocalEngine::new(config).await?;
//!
//! // Execute a Dagger module function
//! let result = engine.execute(
//!     "github.com/myorg/ci",
//!     Some("build"),
//!     None,
//! ).await?;
//!
//! println!("Execution result: {:?}", result);
//! # Ok(())
//! # }
//! ```
//!
//! ### Runtime Detection
//!
//! ```rust,no_run
//! use cb_runner::engine::detect::RuntimeDetector;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let detector = RuntimeDetector::new();
//!
//! // Check if Dagger is available
//! if let Some(info) = detector.detect_dagger().await? {
//!     println!("Dagger {} found at {:?}", info.version, info.path);
//! }
//!
//! // Check container runtime
//! if let Some(runtime) = detector.detect_container_runtime().await? {
//!     println!("Container runtime: {}", runtime.runtime);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Engine Selection (Auto Mode)
//!
//! ```rust,no_run
//! use cb_runner::engine::{EngineSelector, SelectedEngine};
//! use cb_core::engine::{EngineConfig, EngineRequirements};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = EngineConfig::auto();
//! let selector = EngineSelector::new(config).await?;
//!
//! // Select engine based on requirements
//! let requirements = EngineRequirements::default();
//! let engine = selector.select(&requirements).await?;
//!
//! match engine {
//!     SelectedEngine::Local(local) => {
//!         println!("Using local engine");
//!     }
//!     SelectedEngine::Cloud(_) => {
//!         println!("Using cloud engine");
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Configuration
//!
//! Engine behavior is controlled by the [`cb_core::engine::EngineConfig`] type,
//! which supports three modes:
//!
//! - **Local**: Always use local Dagger installation
//! - **Cloud**: Always use Engine Service (requires credentials)
//! - **Auto**: Automatically select based on requirements and availability
//!
//! See the `cb-core` crate documentation for detailed configuration options.

pub mod detect;
pub mod error;
pub mod local;
pub mod selector;

// Re-export main types for convenience
pub use detect::{ContainerRuntimeInfo, DaggerInfo, RuntimeDetector};
pub use error::{EngineError, EngineResult};
pub use local::LocalEngine;
pub use selector::{EngineSelector, SelectedEngine};

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

/// Result of executing a Dagger module function.
///
/// Contains the outputs from the execution, along with metadata about
/// resource usage and timing.
#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    /// Outputs returned by the Dagger function.
    ///
    /// The structure depends on the function being called. Typically this
    /// is a JSON object with named outputs.
    pub outputs: Option<serde_json::Value>,

    /// Duration of the execution.
    pub duration: Duration,

    /// Logs captured during execution.
    pub logs: Vec<String>,

    /// Resource usage statistics (if available).
    pub resource_usage: Option<ResourceUsage>,
}

/// Resource usage statistics from an execution.
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// CPU time used in milliseconds.
    pub cpu_millis: u64,

    /// Peak memory usage in bytes.
    pub memory_bytes: u64,

    /// Network bytes received.
    pub network_rx_bytes: u64,

    /// Network bytes transmitted.
    pub network_tx_bytes: u64,
}

/// Trait for engine implementations that can execute Dagger modules.
///
/// This trait abstracts over local and cloud engine implementations,
/// allowing the runner to use either transparently.
///
/// # Examples
///
/// ```rust,no_run
/// use cb_runner::engine::{EngineExecutor, ExecutionOutput, EngineResult};
/// use async_trait::async_trait;
/// use std::collections::HashMap;
///
/// struct MyEngine;
///
/// #[async_trait]
/// impl EngineExecutor for MyEngine {
///     async fn execute(
///         &self,
///         module: &str,
///         function: Option<&str>,
///         args: Option<&HashMap<String, serde_json::Value>>,
///     ) -> EngineResult<ExecutionOutput> {
///         // Implementation here
///         # unimplemented!()
///     }
///
///     async fn is_healthy(&self) -> bool {
///         true
///     }
///
///     fn engine_type(&self) -> &'static str {
///         "my-engine"
///     }
/// }
/// ```
#[async_trait]
pub trait EngineExecutor: Send + Sync {
    /// Execute a Dagger module function.
    ///
    /// # Arguments
    ///
    /// * `module` - Path to the Dagger module (e.g., "github.com/org/repo")
    /// * `function` - Optional function name to call (default: module's default)
    /// * `args` - Optional arguments to pass to the function
    ///
    /// # Returns
    ///
    /// Returns the execution output on success, or an error if execution fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The module cannot be loaded
    /// - The function does not exist
    /// - The function execution fails
    /// - The connection to the engine is lost
    async fn execute(
        &self,
        module: &str,
        function: Option<&str>,
        args: Option<&HashMap<String, serde_json::Value>>,
    ) -> EngineResult<ExecutionOutput>;

    /// Check if the engine is healthy and ready to accept requests.
    ///
    /// This is used for health checks and to determine availability in
    /// auto mode selection.
    async fn is_healthy(&self) -> bool;

    /// Returns the type of this engine ("local" or "cloud").
    ///
    /// Used for logging and metrics.
    fn engine_type(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_output_default() {
        let output = ExecutionOutput {
            outputs: None,
            duration: Duration::from_secs(1),
            logs: vec![],
            resource_usage: None,
        };

        assert!(output.outputs.is_none());
        assert_eq!(output.duration, Duration::from_secs(1));
        assert!(output.logs.is_empty());
    }

    #[test]
    fn test_resource_usage_default() {
        let usage = ResourceUsage::default();
        assert_eq!(usage.cpu_millis, 0);
        assert_eq!(usage.memory_bytes, 0);
        assert_eq!(usage.network_rx_bytes, 0);
        assert_eq!(usage.network_tx_bytes, 0);
    }

    #[test]
    fn test_execution_output_with_outputs() {
        let output = ExecutionOutput {
            outputs: Some(serde_json::json!({"result": "success"})),
            duration: Duration::from_millis(500),
            logs: vec!["step 1".to_string(), "step 2".to_string()],
            resource_usage: Some(ResourceUsage {
                cpu_millis: 100,
                memory_bytes: 1024 * 1024,
                network_rx_bytes: 1000,
                network_tx_bytes: 500,
            }),
        };

        assert!(output.outputs.is_some());
        assert_eq!(output.logs.len(), 2);
        assert!(output.resource_usage.is_some());

        let usage = output.resource_usage.unwrap();
        assert_eq!(usage.cpu_millis, 100);
        assert_eq!(usage.memory_bytes, 1024 * 1024);
    }
}
