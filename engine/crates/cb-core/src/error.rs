//! Error types for Circuit Breaker.
//!
//! This module provides a comprehensive error hierarchy for the Circuit Breaker
//! workflow engine, covering workflow validation, execution, and infrastructure errors.

use std::fmt;

/// Result type alias using the Circuit Breaker error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for Circuit Breaker operations.
#[derive(Debug, Clone)]
pub struct Error {
    /// The kind of error that occurred.
    kind: ErrorKind,
    /// Optional error message with additional context.
    message: Option<String>,
    /// Optional source/cause of the error (as string for Clone).
    source: Option<String>,
}

/// Categories of errors that can occur in Circuit Breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    // ============ Validation Errors ============
    /// Invalid workflow schema.
    InvalidSchema,
    /// Duplicate identifier (place or transition).
    DuplicateId,
    /// Invalid reference (arc pointing to non-existent place).
    InvalidReference,
    /// Invalid guard expression (CEL syntax error).
    InvalidGuard,
    /// Invalid resource specification.
    InvalidResource,

    // ============ Workflow Errors ============
    /// Workflow not found.
    WorkflowNotFound,
    /// Workflow already exists.
    WorkflowExists,
    /// Invalid workflow state.
    InvalidWorkflowState,

    // ============ Execution Errors ============
    /// Run not found.
    RunNotFound,
    /// Transition cannot fire (not enabled).
    TransitionNotEnabled,
    /// Transition execution failed.
    TransitionFailed,
    /// Execution timeout.
    Timeout,
    /// Deadlock detected.
    Deadlock,
    /// Run was cancelled.
    Cancelled,

    // ============ Action Errors ============
    /// Dagger pipeline failed.
    DaggerError,
    /// HTTP request failed.
    HttpError,
    /// Script execution failed.
    ScriptError,

    // ============ Infrastructure Errors ============
    /// NATS connection/messaging error.
    NatsError,
    /// Database error.
    DatabaseError,
    /// Kubernetes API error.
    KubernetesError,
    /// Serialization/deserialization error.
    SerializationError,

    // ============ Generic Errors ============
    /// Internal error (unexpected).
    Internal,
    /// Configuration error.
    Configuration,
    /// Authentication/authorization error.
    Unauthorized,
    /// Rate limit exceeded.
    RateLimited,
}

impl Error {
    /// Create a new error with the given kind.
    #[must_use]
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            message: None,
            source: None,
        }
    }

    /// Create a new error with a message.
    #[must_use]
    pub fn with_message(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: Some(message.into()),
            source: None,
        }
    }

    /// Add a message to this error.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Add a source error to this error.
    #[must_use]
    pub fn source(mut self, source: impl fmt::Display) -> Self {
        self.source = Some(source.to_string());
        self
    }

    /// Get the error kind.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Check if this is a retriable error.
    #[must_use]
    pub fn is_retriable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Timeout
                | ErrorKind::NatsError
                | ErrorKind::DatabaseError
                | ErrorKind::KubernetesError
                | ErrorKind::RateLimited
                | ErrorKind::HttpError
        )
    }

    /// Check if this is a validation error.
    #[must_use]
    pub fn is_validation_error(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::InvalidSchema
                | ErrorKind::DuplicateId
                | ErrorKind::InvalidReference
                | ErrorKind::InvalidGuard
                | ErrorKind::InvalidResource
        )
    }

    /// Get the error code as a string (for API responses).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::InvalidSchema => "INVALID_SCHEMA",
            ErrorKind::DuplicateId => "DUPLICATE_ID",
            ErrorKind::InvalidReference => "INVALID_REFERENCE",
            ErrorKind::InvalidGuard => "INVALID_GUARD",
            ErrorKind::InvalidResource => "INVALID_RESOURCE",
            ErrorKind::WorkflowNotFound => "WORKFLOW_NOT_FOUND",
            ErrorKind::WorkflowExists => "WORKFLOW_EXISTS",
            ErrorKind::InvalidWorkflowState => "INVALID_WORKFLOW_STATE",
            ErrorKind::RunNotFound => "RUN_NOT_FOUND",
            ErrorKind::TransitionNotEnabled => "TRANSITION_NOT_ENABLED",
            ErrorKind::TransitionFailed => "TRANSITION_FAILED",
            ErrorKind::Timeout => "TIMEOUT",
            ErrorKind::Deadlock => "DEADLOCK",
            ErrorKind::Cancelled => "CANCELLED",
            ErrorKind::DaggerError => "DAGGER_ERROR",
            ErrorKind::HttpError => "HTTP_ERROR",
            ErrorKind::ScriptError => "SCRIPT_ERROR",
            ErrorKind::NatsError => "NATS_ERROR",
            ErrorKind::DatabaseError => "DATABASE_ERROR",
            ErrorKind::KubernetesError => "KUBERNETES_ERROR",
            ErrorKind::SerializationError => "SERIALIZATION_ERROR",
            ErrorKind::Internal => "INTERNAL_ERROR",
            ErrorKind::Configuration => "CONFIGURATION_ERROR",
            ErrorKind::Unauthorized => "UNAUTHORIZED",
            ErrorKind::RateLimited => "RATE_LIMITED",
        }
    }

    /// Get an HTTP status code appropriate for this error.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        match self.kind {
            // 400 Bad Request - validation errors
            ErrorKind::InvalidSchema
            | ErrorKind::DuplicateId
            | ErrorKind::InvalidReference
            | ErrorKind::InvalidGuard
            | ErrorKind::InvalidResource
            | ErrorKind::InvalidWorkflowState => 400,

            // 401 Unauthorized
            ErrorKind::Unauthorized => 401,

            // 404 Not Found
            ErrorKind::WorkflowNotFound | ErrorKind::RunNotFound => 404,

            // 409 Conflict
            ErrorKind::WorkflowExists | ErrorKind::TransitionNotEnabled => 409,

            // 408 Request Timeout
            ErrorKind::Timeout => 408,

            // 429 Too Many Requests
            ErrorKind::RateLimited => 429,

            // 500 Internal Server Error
            ErrorKind::Internal
            | ErrorKind::NatsError
            | ErrorKind::DatabaseError
            | ErrorKind::KubernetesError
            | ErrorKind::SerializationError
            | ErrorKind::Configuration => 500,

            // 502 Bad Gateway - upstream service failures
            ErrorKind::DaggerError | ErrorKind::HttpError | ErrorKind::ScriptError => 502,

            // 409 Conflict for execution issues
            ErrorKind::TransitionFailed | ErrorKind::Deadlock | ErrorKind::Cancelled => 409,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}]", self.code())?;

        if let Some(ref msg) = self.message {
            write!(f, " {msg}")?;
        } else {
            write!(f, " {}", self.kind)?;
        }

        if let Some(ref src) = self.source {
            write!(f, ": {src}")?;
        }

        Ok(())
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = match self {
            Self::InvalidSchema => "invalid workflow schema",
            Self::DuplicateId => "duplicate identifier",
            Self::InvalidReference => "invalid reference",
            Self::InvalidGuard => "invalid guard expression",
            Self::InvalidResource => "invalid resource specification",
            Self::WorkflowNotFound => "workflow not found",
            Self::WorkflowExists => "workflow already exists",
            Self::InvalidWorkflowState => "invalid workflow state",
            Self::RunNotFound => "run not found",
            Self::TransitionNotEnabled => "transition not enabled",
            Self::TransitionFailed => "transition execution failed",
            Self::Timeout => "execution timeout",
            Self::Deadlock => "deadlock detected",
            Self::Cancelled => "execution cancelled",
            Self::DaggerError => "Dagger pipeline error",
            Self::HttpError => "HTTP request error",
            Self::ScriptError => "script execution error",
            Self::NatsError => "NATS messaging error",
            Self::DatabaseError => "database error",
            Self::KubernetesError => "Kubernetes API error",
            Self::SerializationError => "serialization error",
            Self::Internal => "internal error",
            Self::Configuration => "configuration error",
            Self::Unauthorized => "unauthorized",
            Self::RateLimited => "rate limit exceeded",
        };
        write!(f, "{desc}")
    }
}

impl std::error::Error for Error {}

// ============ Convenience constructors ============

impl Error {
    /// Create a workflow not found error.
    #[must_use]
    pub fn workflow_not_found(id: impl Into<String>) -> Self {
        Self::with_message(
            ErrorKind::WorkflowNotFound,
            format!("workflow '{}' not found", id.into()),
        )
    }

    /// Create a run not found error.
    #[must_use]
    pub fn run_not_found(id: impl Into<String>) -> Self {
        Self::with_message(
            ErrorKind::RunNotFound,
            format!("run '{}' not found", id.into()),
        )
    }

    /// Create an invalid schema error.
    #[must_use]
    pub fn invalid_schema(details: impl Into<String>) -> Self {
        Self::with_message(ErrorKind::InvalidSchema, details)
    }

    /// Create a transition not enabled error.
    #[must_use]
    pub fn transition_not_enabled(transition_id: impl Into<String>) -> Self {
        Self::with_message(
            ErrorKind::TransitionNotEnabled,
            format!("transition '{}' is not enabled", transition_id.into()),
        )
    }

    /// Create a timeout error.
    #[must_use]
    pub fn timeout(details: impl Into<String>) -> Self {
        Self::with_message(ErrorKind::Timeout, details)
    }

    /// Create an internal error.
    #[must_use]
    pub fn internal(details: impl Into<String>) -> Self {
        Self::with_message(ErrorKind::Internal, details)
    }

    /// Create a serialization error.
    #[must_use]
    pub fn serialization(err: impl fmt::Display) -> Self {
        Self::new(ErrorKind::SerializationError).source(err)
    }
}

// ============ From implementations ============

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::serialization(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::new(ErrorKind::Internal).source(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::workflow_not_found("my-workflow");
        assert!(err.to_string().contains("WORKFLOW_NOT_FOUND"));
        assert!(err.to_string().contains("my-workflow"));
    }

    #[test]
    fn test_error_kind() {
        let err = Error::new(ErrorKind::Timeout);
        assert_eq!(err.kind(), ErrorKind::Timeout);
        assert!(err.is_retriable());
    }

    #[test]
    fn test_validation_error() {
        let err = Error::invalid_schema("missing required field 'name'");
        assert!(err.is_validation_error());
        assert_eq!(err.http_status(), 400);
    }

    #[test]
    fn test_error_chaining() {
        let err = Error::new(ErrorKind::DaggerError)
            .message("pipeline failed")
            .source("container exited with code 1");

        let display = err.to_string();
        assert!(display.contains("DAGGER_ERROR"));
        assert!(display.contains("pipeline failed"));
        assert!(display.contains("container exited"));
    }
}
