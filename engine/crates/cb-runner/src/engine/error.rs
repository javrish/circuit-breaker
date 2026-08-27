//! # Engine Error Types
//!
//! This module defines the error types for engine operations in Circuit Breaker.
//! These errors cover all failure modes that can occur during engine detection,
//! initialization, and execution.
//!
//! ## Error Categories
//!
//! Errors are organized into categories based on their source:
//!
//! - **Detection errors**: Failures when detecting Dagger or container runtimes
//! - **Configuration errors**: Invalid or missing configuration
//! - **Connection errors**: Network or IPC failures
//! - **Execution errors**: Failures during Dagger module execution
//! - **Timeout errors**: Operations that exceed time limits
//!
//! ## Error Handling Strategy
//!
//! The error types are designed to:
//!
//! 1. Provide actionable error messages with troubleshooting guidance
//! 2. Distinguish between recoverable and unrecoverable errors
//! 3. Include context for debugging (paths, versions, durations)
//! 4. Support conversion from underlying library errors
//!
//! ## Usage
//!
//! ```rust
//! use cb_runner::engine::error::{EngineError, EngineResult};
//!
//! fn check_dagger() -> EngineResult<String> {
//!     // Simulate a missing Dagger installation
//!     Err(EngineError::DaggerNotFound {
//!         searched_paths: vec!["/usr/local/bin".into(), "/usr/bin".into()],
//!     })
//! }
//!
//! match check_dagger() {
//!     Ok(version) => println!("Dagger version: {}", version),
//!     Err(EngineError::DaggerNotFound { searched_paths }) => {
//!         eprintln!("Dagger not found. Searched: {:?}", searched_paths);
//!         eprintln!("Install Dagger: https://docs.dagger.io/install");
//!     }
//!     Err(e) => eprintln!("Other error: {}", e),
//! }
//! ```

use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

/// Result type alias for engine operations.
///
/// This is the standard result type used throughout the engine module.
pub type EngineResult<T> = Result<T, EngineError>;

/// Errors that can occur during engine operations.
///
/// This enum covers all failure modes for local and cloud engine operations,
/// including detection, configuration, connection, and execution errors.
#[derive(Debug, Error)]
pub enum EngineError {
    // ========================================================================
    // Detection Errors
    // ========================================================================
    /// Dagger CLI was not found on the system.
    ///
    /// This error occurs when the local engine cannot find a Dagger
    /// installation. The user should install Dagger or configure
    /// the path explicitly.
    #[error("Dagger CLI not found. Searched paths: {searched_paths:?}. Install from https://docs.dagger.io/install")]
    DaggerNotFound {
        /// Paths that were searched for the Dagger binary.
        searched_paths: Vec<PathBuf>,
    },

    /// Dagger version is incompatible with requirements.
    ///
    /// This error occurs when the installed Dagger version does not
    /// meet the minimum version requirement specified in the configuration.
    #[error(
        "Dagger version {installed} is incompatible. Required: {required}. \
         Upgrade with: dagger upgrade"
    )]
    DaggerVersionIncompatible {
        /// The version that is installed.
        installed: String,
        /// The minimum version required.
        required: String,
    },

    /// No container runtime found (Docker, Podman, etc.).
    ///
    /// Dagger requires a container runtime to execute pipelines. This
    /// error occurs when no supported runtime is detected.
    #[error(
        "No container runtime found. Dagger requires Docker, Podman, or another \
         compatible runtime. Install Docker: https://docs.docker.com/get-docker/"
    )]
    NoContainerRuntime,

    /// Container runtime is not running.
    ///
    /// The runtime was found but is not currently running. The user
    /// should start the runtime daemon.
    #[error(
        "Container runtime '{runtime}' is installed but not running. \
         Start it with: {start_command}"
    )]
    ContainerRuntimeNotRunning {
        /// The runtime that was detected.
        runtime: String,
        /// Command to start the runtime.
        start_command: String,
    },

    /// Failed to detect runtime information.
    ///
    /// An error occurred while trying to detect the Dagger or container
    /// runtime installation.
    #[error("Failed to detect {component}: {message}")]
    DetectionFailed {
        /// What component we were trying to detect.
        component: String,
        /// The error message.
        message: String,
    },

    // ========================================================================
    // Configuration Errors
    // ========================================================================
    /// Invalid engine configuration.
    ///
    /// The provided configuration is invalid or inconsistent.
    #[error("Invalid engine configuration: {message}")]
    InvalidConfiguration {
        /// Description of the configuration error.
        message: String,
    },

    /// Missing required configuration for the selected mode.
    ///
    /// Some configuration is required but not provided. For example,
    /// cloud mode requires API credentials.
    #[error("Missing required configuration for {mode} mode: {missing}")]
    MissingConfiguration {
        /// The engine mode.
        mode: String,
        /// The missing configuration item.
        missing: String,
    },

    /// Cloud credentials are invalid or expired.
    #[error("Invalid cloud credentials: {message}")]
    InvalidCredentials {
        /// Description of the credential error.
        message: String,
    },

    // ========================================================================
    // Connection Errors
    // ========================================================================
    /// Failed to connect to the Dagger engine.
    ///
    /// This error occurs when the connection to the Dagger engine
    /// (local or remote) cannot be established.
    #[error("Failed to connect to Dagger engine: {message}")]
    ConnectionFailed {
        /// The error message.
        message: String,
        /// The underlying error, if available.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Connection to the engine was lost during execution.
    #[error("Connection to Dagger engine lost: {message}")]
    ConnectionLost {
        /// The error message.
        message: String,
    },

    /// Failed to connect to the Engine Service (cloud mode).
    #[error("Failed to connect to Engine Service at {url}: {message}")]
    EngineServiceConnectionFailed {
        /// The Engine Service URL.
        url: String,
        /// The error message.
        message: String,
    },

    // ========================================================================
    // Execution Errors
    // ========================================================================
    /// Failed to load a Dagger module.
    #[error("Failed to load module '{module}': {message}")]
    ModuleLoadFailed {
        /// The module path.
        module: String,
        /// The error message.
        message: String,
    },

    /// The specified function does not exist in the module.
    #[error("Function '{function}' not found in module '{module}'")]
    FunctionNotFound {
        /// The module path.
        module: String,
        /// The function name.
        function: String,
    },

    /// Function execution failed.
    #[error("Execution of '{module}::{function}' failed: {message}")]
    ExecutionFailed {
        /// The module path.
        module: String,
        /// The function name.
        function: String,
        /// The error message.
        message: String,
        /// Exit code if available.
        exit_code: Option<i32>,
    },

    /// Execution was cancelled.
    #[error("Execution cancelled: {reason}")]
    ExecutionCancelled {
        /// The reason for cancellation.
        reason: String,
    },

    // ========================================================================
    // Timeout Errors
    // ========================================================================
    /// Operation timed out.
    #[error("{operation} timed out after {duration:?}")]
    Timeout {
        /// The operation that timed out.
        operation: String,
        /// How long we waited.
        duration: Duration,
    },

    /// Connection timeout.
    #[error("Connection timed out after {duration:?}")]
    ConnectionTimeout {
        /// How long we waited.
        duration: Duration,
    },

    // ========================================================================
    // Resource Errors
    // ========================================================================
    /// Insufficient resources to execute.
    #[error("Insufficient resources: {message}")]
    InsufficientResources {
        /// Description of the resource constraint.
        message: String,
    },

    /// Resource quota exceeded.
    #[error("Resource quota exceeded: {quota_type} (limit: {limit}, current: {current})")]
    QuotaExceeded {
        /// The type of quota.
        quota_type: String,
        /// The quota limit.
        limit: u64,
        /// The current usage.
        current: u64,
    },

    // ========================================================================
    // Engine Selection Errors
    // ========================================================================
    /// No engine available for the requested mode.
    #[error(
        "No engine available. Local: {local_reason:?}, Cloud: {cloud_reason:?}. \
         Check configuration and ensure either local Dagger or cloud credentials are configured."
    )]
    NoEngineAvailable {
        /// Why local engine is not available.
        local_reason: Option<String>,
        /// Why cloud engine is not available.
        cloud_reason: Option<String>,
    },

    /// Engine mode is not supported in this configuration.
    #[error("Engine mode '{mode}' is not supported: {reason}")]
    ModeNotSupported {
        /// The requested mode.
        mode: String,
        /// The reason it's not supported.
        reason: String,
    },

    // ========================================================================
    // Internal Errors
    // ========================================================================
    /// An internal error occurred.
    ///
    /// This indicates a bug or unexpected condition in the engine code.
    #[error("Internal engine error: {message}")]
    Internal {
        /// The error message.
        message: String,
    },

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization/deserialization error occurred.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl EngineError {
    // ========================================================================
    // Constructor helpers
    // ========================================================================

    /// Creates a new `DaggerNotFound` error.
    pub fn dagger_not_found(searched_paths: Vec<PathBuf>) -> Self {
        Self::DaggerNotFound { searched_paths }
    }

    /// Creates a new `ConnectionFailed` error.
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::ConnectionFailed {
            message: message.into(),
            source: None,
        }
    }

    /// Creates a new `ConnectionFailed` error with a source.
    pub fn connection_failed_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::ConnectionFailed {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates a new `ModuleLoadFailed` error.
    pub fn module_load_failed(module: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ModuleLoadFailed {
            module: module.into(),
            message: message.into(),
        }
    }

    /// Creates a new `ExecutionFailed` error.
    pub fn execution_failed(
        module: impl Into<String>,
        function: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::ExecutionFailed {
            module: module.into(),
            function: function.into(),
            message: message.into(),
            exit_code: None,
        }
    }

    /// Creates a new `ExecutionFailed` error with an exit code.
    pub fn execution_failed_with_exit_code(
        module: impl Into<String>,
        function: impl Into<String>,
        message: impl Into<String>,
        exit_code: i32,
    ) -> Self {
        Self::ExecutionFailed {
            module: module.into(),
            function: function.into(),
            message: message.into(),
            exit_code: Some(exit_code),
        }
    }

    /// Creates a new `Timeout` error.
    pub fn timeout(operation: impl Into<String>, duration: Duration) -> Self {
        Self::Timeout {
            operation: operation.into(),
            duration,
        }
    }

    /// Creates a new `InvalidConfiguration` error.
    pub fn invalid_configuration(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration {
            message: message.into(),
        }
    }

    /// Creates a new `Internal` error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    // ========================================================================
    // Error classification helpers
    // ========================================================================

    /// Returns `true` if this error is recoverable and the operation can be retried.
    ///
    /// Recoverable errors are typically transient failures like timeouts or
    /// connection issues that may succeed on retry.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed { .. }
                | Self::ConnectionLost { .. }
                | Self::EngineServiceConnectionFailed { .. }
                | Self::Timeout { .. }
                | Self::ConnectionTimeout { .. }
                | Self::ContainerRuntimeNotRunning { .. }
        )
    }

    /// Returns `true` if this error is a configuration error.
    ///
    /// Configuration errors require user intervention to fix and should not
    /// be retried automatically.
    pub fn is_configuration_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidConfiguration { .. }
                | Self::MissingConfiguration { .. }
                | Self::InvalidCredentials { .. }
                | Self::ModeNotSupported { .. }
        )
    }

    /// Returns `true` if this error is related to missing dependencies.
    ///
    /// These errors indicate that required software (Dagger, Docker, etc.)
    /// is not installed.
    pub fn is_missing_dependency(&self) -> bool {
        matches!(
            self,
            Self::DaggerNotFound { .. }
                | Self::DaggerVersionIncompatible { .. }
                | Self::NoContainerRuntime
        )
    }

    /// Returns `true` if this error is a timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::ConnectionTimeout { .. })
    }

    /// Returns a user-friendly troubleshooting message for this error.
    pub fn troubleshooting_hint(&self) -> Option<&'static str> {
        match self {
            Self::DaggerNotFound { .. } => Some(
                "Install Dagger from https://docs.dagger.io/install or \
                 set the dagger_path in your configuration.",
            ),
            Self::NoContainerRuntime => Some(
                "Install Docker from https://docs.docker.com/get-docker/ or \
                 Podman from https://podman.io/getting-started/installation",
            ),
            Self::ContainerRuntimeNotRunning { .. } => Some(
                "Start your container runtime. For Docker: 'sudo systemctl start docker' \
                 or open Docker Desktop.",
            ),
            Self::InvalidCredentials { .. } => Some(
                "Check your API key and organization ID. You can find these in \
                 the Circuit Breaker dashboard.",
            ),
            Self::ConnectionTimeout { .. } | Self::Timeout { .. } => Some(
                "The operation timed out. Check your network connection and \
                 consider increasing the timeout in your configuration.",
            ),
            Self::QuotaExceeded { .. } => Some(
                "You have exceeded your resource quota. Upgrade your plan or \
                 wait for quota to reset.",
            ),
            Self::NoEngineAvailable { .. } => Some(
                "Ensure either local Dagger is installed or cloud credentials \
                 are configured in your runner configuration.",
            ),
            _ => None,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dagger_not_found_error() {
        let err = EngineError::dagger_not_found(vec![
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ]);

        let msg = err.to_string();
        assert!(msg.contains("Dagger CLI not found"));
        assert!(msg.contains("/usr/local/bin"));
        assert!(msg.contains("https://docs.dagger.io/install"));
    }

    #[test]
    fn test_connection_failed_error() {
        let err = EngineError::connection_failed("network unreachable");
        assert!(err.to_string().contains("network unreachable"));
    }

    #[test]
    fn test_connection_failed_with_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = EngineError::connection_failed_with_source("connection refused", io_err);

        assert!(err.to_string().contains("connection refused"));
        // Source is preserved
        match &err {
            EngineError::ConnectionFailed { source, .. } => {
                assert!(source.is_some());
            }
            _ => panic!("Wrong error variant"),
        }
    }

    #[test]
    fn test_module_load_failed_error() {
        let err = EngineError::module_load_failed("github.com/org/repo", "not found");
        let msg = err.to_string();
        assert!(msg.contains("github.com/org/repo"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_execution_failed_error() {
        let err = EngineError::execution_failed("my-module", "build", "compilation error");
        let msg = err.to_string();
        assert!(msg.contains("my-module"));
        assert!(msg.contains("build"));
        assert!(msg.contains("compilation error"));
    }

    #[test]
    fn test_execution_failed_with_exit_code() {
        let err =
            EngineError::execution_failed_with_exit_code("my-module", "test", "test failed", 1);
        match &err {
            EngineError::ExecutionFailed { exit_code, .. } => {
                assert_eq!(*exit_code, Some(1));
            }
            _ => panic!("Wrong error variant"),
        }
    }

    #[test]
    fn test_timeout_error() {
        let err = EngineError::timeout("module load", Duration::from_secs(30));
        let msg = err.to_string();
        assert!(msg.contains("module load"));
        assert!(msg.contains("30"));
    }

    #[test]
    fn test_is_recoverable() {
        assert!(EngineError::connection_failed("test").is_recoverable());
        assert!(EngineError::ConnectionLost {
            message: "test".into()
        }
        .is_recoverable());
        assert!(EngineError::timeout("test", Duration::from_secs(1)).is_recoverable());
        assert!(EngineError::ConnectionTimeout {
            duration: Duration::from_secs(1)
        }
        .is_recoverable());
        assert!(EngineError::ContainerRuntimeNotRunning {
            runtime: "docker".into(),
            start_command: "systemctl start docker".into(),
        }
        .is_recoverable());

        // Non-recoverable errors
        assert!(!EngineError::dagger_not_found(vec![]).is_recoverable());
        assert!(!EngineError::invalid_configuration("test").is_recoverable());
        assert!(!EngineError::NoContainerRuntime.is_recoverable());
    }

    #[test]
    fn test_is_configuration_error() {
        assert!(EngineError::invalid_configuration("test").is_configuration_error());
        assert!(EngineError::MissingConfiguration {
            mode: "cloud".into(),
            missing: "api_key".into(),
        }
        .is_configuration_error());
        assert!(EngineError::InvalidCredentials {
            message: "expired".into()
        }
        .is_configuration_error());

        // Non-configuration errors
        assert!(!EngineError::connection_failed("test").is_configuration_error());
        assert!(!EngineError::NoContainerRuntime.is_configuration_error());
    }

    #[test]
    fn test_is_missing_dependency() {
        assert!(EngineError::dagger_not_found(vec![]).is_missing_dependency());
        assert!(EngineError::NoContainerRuntime.is_missing_dependency());
        assert!(EngineError::DaggerVersionIncompatible {
            installed: "0.1.0".into(),
            required: "0.18.0".into(),
        }
        .is_missing_dependency());

        // Non-dependency errors
        assert!(!EngineError::connection_failed("test").is_missing_dependency());
    }

    #[test]
    fn test_is_timeout() {
        assert!(EngineError::timeout("test", Duration::from_secs(1)).is_timeout());
        assert!(EngineError::ConnectionTimeout {
            duration: Duration::from_secs(1)
        }
        .is_timeout());

        // Non-timeout errors
        assert!(!EngineError::connection_failed("test").is_timeout());
    }

    #[test]
    fn test_troubleshooting_hint() {
        assert!(EngineError::dagger_not_found(vec![])
            .troubleshooting_hint()
            .is_some());
        assert!(EngineError::NoContainerRuntime
            .troubleshooting_hint()
            .is_some());
        assert!(EngineError::ContainerRuntimeNotRunning {
            runtime: "docker".into(),
            start_command: "start docker".into(),
        }
        .troubleshooting_hint()
        .is_some());

        // Some errors don't have hints
        assert!(EngineError::internal("bug").troubleshooting_hint().is_none());
    }

    #[test]
    fn test_no_engine_available_error() {
        let err = EngineError::NoEngineAvailable {
            local_reason: Some("Dagger not found".into()),
            cloud_reason: Some("No credentials".into()),
        };

        let msg = err.to_string();
        assert!(msg.contains("No engine available"));
        assert!(msg.contains("Dagger not found"));
        assert!(msg.contains("No credentials"));
    }

    #[test]
    fn test_quota_exceeded_error() {
        let err = EngineError::QuotaExceeded {
            quota_type: "concurrent_engines".into(),
            limit: 10,
            current: 15,
        };

        let msg = err.to_string();
        assert!(msg.contains("concurrent_engines"));
        assert!(msg.contains("10"));
        assert!(msg.contains("15"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let engine_err: EngineError = io_err.into();

        assert!(matches!(engine_err, EngineError::Io(_)));
        assert!(engine_err.to_string().contains("file not found"));
    }

    #[test]
    fn test_serialization_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let engine_err: EngineError = json_err.into();

        assert!(matches!(engine_err, EngineError::Serialization(_)));
    }

    #[test]
    fn test_version_incompatible_error() {
        let err = EngineError::DaggerVersionIncompatible {
            installed: "0.10.0".into(),
            required: "0.18.0".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("0.10.0"));
        assert!(msg.contains("0.18.0"));
        assert!(msg.contains("dagger upgrade"));
    }

    #[test]
    fn test_engine_service_connection_failed() {
        let err = EngineError::EngineServiceConnectionFailed {
            url: "https://engines.circuitbreaker.io".into(),
            message: "connection refused".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("engines.circuitbreaker.io"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_function_not_found_error() {
        let err = EngineError::FunctionNotFound {
            module: "github.com/org/ci".into(),
            function: "nonexistent".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("github.com/org/ci"));
        assert!(msg.contains("nonexistent"));
    }

    #[test]
    fn test_execution_cancelled_error() {
        let err = EngineError::ExecutionCancelled {
            reason: "user requested".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("cancelled"));
        assert!(msg.contains("user requested"));
    }

    #[test]
    fn test_insufficient_resources_error() {
        let err = EngineError::InsufficientResources {
            message: "not enough memory".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("Insufficient resources"));
        assert!(msg.contains("not enough memory"));
    }

    #[test]
    fn test_mode_not_supported_error() {
        let err = EngineError::ModeNotSupported {
            mode: "cloud".into(),
            reason: "no credentials configured".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("cloud"));
        assert!(msg.contains("no credentials configured"));
    }

    #[test]
    fn test_detection_failed_error() {
        let err = EngineError::DetectionFailed {
            component: "Docker".into(),
            message: "permission denied".into(),
        };

        let msg = err.to_string();
        assert!(msg.contains("Docker"));
        assert!(msg.contains("permission denied"));
    }
}
