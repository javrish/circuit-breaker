//! Error types for the Circuit Breaker Webhook Server.
//!
//! This module defines the error types used throughout the webhook server,
//! including authentication failures, validation errors, and NATS connectivity issues.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result type alias for webhook operations.
pub type Result<T> = std::result::Result<T, WebhookError>;

/// Webhook server errors.
#[derive(Debug, Error)]
pub enum WebhookError {
    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Invalid signature.
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    /// Missing authentication header.
    #[error("Missing authentication header: {0}")]
    MissingAuthHeader(String),

    /// Endpoint not found.
    #[error("Endpoint not found: {0}")]
    EndpointNotFound(String),

    /// Endpoint disabled.
    #[error("Endpoint is disabled: {0}")]
    EndpointDisabled(String),

    /// Payload too large.
    #[error("Payload too large: {size} bytes exceeds limit of {limit} bytes")]
    PayloadTooLarge {
        /// Actual payload size in bytes.
        size: usize,
        /// Maximum allowed payload size in bytes.
        limit: usize,
    },

    /// Invalid payload.
    #[error("Invalid payload: {0}")]
    InvalidPayload(String),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Rate limit exceeded.
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// IP not allowed.
    #[error("IP address not allowed: {0}")]
    IpNotAllowed(String),

    /// Filter evaluation error.
    #[error("Filter evaluation failed: {0}")]
    FilterError(String),

    /// Input mapping error.
    #[error("Input mapping failed: {0}")]
    InputMappingError(String),

    /// NATS connection error.
    #[error("NATS error: {0}")]
    Nats(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Event not found.
    #[error("Event not found: {0}")]
    EventNotFound(String),

    /// Timeout error.
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),
}

impl WebhookError {
    /// Get the HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::AuthenticationFailed(_) => StatusCode::UNAUTHORIZED,
            Self::InvalidSignature(_) => StatusCode::UNAUTHORIZED,
            Self::MissingAuthHeader(_) => StatusCode::UNAUTHORIZED,
            Self::EndpointNotFound(_) => StatusCode::NOT_FOUND,
            Self::EndpointDisabled(_) => StatusCode::FORBIDDEN,
            Self::PayloadTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidPayload(_) => StatusCode::BAD_REQUEST,
            Self::Json(_) => StatusCode::BAD_REQUEST,
            Self::RateLimitExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::IpNotAllowed(_) => StatusCode::FORBIDDEN,
            Self::FilterError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::InputMappingError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Nats(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::EventNotFound(_) => StatusCode::NOT_FOUND,
            Self::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
        }
    }

    /// Get the error code for this error.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::AuthenticationFailed(_) => "AUTHENTICATION_FAILED",
            Self::InvalidSignature(_) => "INVALID_SIGNATURE",
            Self::MissingAuthHeader(_) => "MISSING_AUTH_HEADER",
            Self::EndpointNotFound(_) => "ENDPOINT_NOT_FOUND",
            Self::EndpointDisabled(_) => "ENDPOINT_DISABLED",
            Self::PayloadTooLarge { .. } => "PAYLOAD_TOO_LARGE",
            Self::InvalidPayload(_) => "INVALID_PAYLOAD",
            Self::Json(_) => "JSON_ERROR",
            Self::RateLimitExceeded(_) => "RATE_LIMIT_EXCEEDED",
            Self::IpNotAllowed(_) => "IP_NOT_ALLOWED",
            Self::FilterError(_) => "FILTER_ERROR",
            Self::InputMappingError(_) => "INPUT_MAPPING_ERROR",
            Self::Nats(_) => "NATS_ERROR",
            Self::Config(_) => "CONFIG_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::EventNotFound(_) => "EVENT_NOT_FOUND",
            Self::Timeout(_) => "TIMEOUT",
            Self::Validation(_) => "VALIDATION_ERROR",
        }
    }
}

/// Error response body.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl IntoResponse for WebhookError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorResponse {
            code: self.error_code().to_string(),
            message: self.to_string(),
            details: None,
        };

        let json = serde_json::to_string(&body).unwrap_or_else(|_| {
            r#"{"code":"INTERNAL_ERROR","message":"Failed to serialize error"}"#.to_string()
        });

        (status, [("content-type", "application/json")], json).into_response()
    }
}

impl From<anyhow::Error> for WebhookError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_codes() {
        assert_eq!(
            WebhookError::AuthenticationFailed("test".to_string()).status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            WebhookError::EndpointNotFound("test".to_string()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            WebhookError::RateLimitExceeded("test".to_string()).status_code(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            WebhookError::PayloadTooLarge {
                size: 100,
                limit: 50
            }
            .status_code(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            WebhookError::InvalidSignature("test".to_string()).error_code(),
            "INVALID_SIGNATURE"
        );
        assert_eq!(
            WebhookError::Nats("test".to_string()).error_code(),
            "NATS_ERROR"
        );
    }

    #[test]
    fn test_error_response_serialization() {
        let response = ErrorResponse {
            code: "TEST_ERROR".to_string(),
            message: "Test message".to_string(),
            details: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("TEST_ERROR"));
        assert!(json.contains("Test message"));
        assert!(!json.contains("details"));
    }

    #[test]
    fn test_error_display() {
        let err = WebhookError::PayloadTooLarge {
            size: 2_000_000,
            limit: 1_000_000,
        };
        let msg = err.to_string();
        assert!(msg.contains("2000000"));
        assert!(msg.contains("1000000"));
    }
}
