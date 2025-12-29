//! CloudEvents normalization module.
//!
//! This module provides types and utilities for converting webhook payloads
//! from various sources into the CloudEvents specification format.
//!
//! CloudEvents is a specification for describing event data in a common way.
//! See: https://cloudevents.io/
//!
//! ## Example
//!
//! ```rust,ignore
//! use cb_webhook::cloudevents::{CloudEvent, CloudEventBuilder};
//!
//! let event = CloudEventBuilder::new()
//!     .source("github.com/myorg/myrepo")
//!     .event_type("com.github.push")
//!     .subject("refs/heads/main")
//!     .data(payload)
//!     .build()?;
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::{Result, WebhookError};

/// CloudEvents specification version.
pub const CLOUDEVENTS_SPEC_VERSION: &str = "1.0";

/// CloudEvent representation following the CloudEvents specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudEvent {
    /// CloudEvents specification version.
    pub specversion: String,

    /// Unique identifier for this event.
    pub id: String,

    /// Source of the event (URI-reference).
    pub source: String,

    /// Type of event (reverse-DNS naming convention recommended).
    #[serde(rename = "type")]
    pub event_type: String,

    /// Subject of the event (context about the source).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// Timestamp of when the event occurred.
    pub time: DateTime<Utc>,

    /// Content type of the data field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datacontenttype: Option<String>,

    /// Schema of the data field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataschema: Option<String>,

    /// Event payload data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// Base64-encoded binary data (alternative to data).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,

    /// Circuit Breaker extension data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circuitbreaker: Option<CircuitBreakerExtension>,

    /// Additional extension attributes.
    #[serde(flatten)]
    pub extensions: HashMap<String, Value>,
}

/// Circuit Breaker-specific extension data for CloudEvents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerExtension {
    /// Workflow name to trigger.
    #[serde(rename = "workflowName", skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,

    /// Workflow namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Mapped inputs for the workflow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<HashMap<String, Value>>,

    /// Trigger name that matched.
    #[serde(rename = "triggerName", skip_serializing_if = "Option::is_none")]
    pub trigger_name: Option<String>,

    /// Original endpoint that received the webhook.
    #[serde(rename = "endpointId", skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,

    /// Raw event type from the source.
    #[serde(rename = "rawEventType", skip_serializing_if = "Option::is_none")]
    pub raw_event_type: Option<String>,

    /// Trace ID for distributed tracing.
    #[serde(rename = "traceId", skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl Default for CircuitBreakerExtension {
    fn default() -> Self {
        Self {
            workflow_name: None,
            namespace: None,
            inputs: None,
            trigger_name: None,
            endpoint_id: None,
            raw_event_type: None,
            trace_id: None,
        }
    }
}

impl CloudEvent {
    /// Create a new CloudEvent with required fields.
    pub fn new(source: impl Into<String>, event_type: impl Into<String>) -> Self {
        Self {
            specversion: CLOUDEVENTS_SPEC_VERSION.to_string(),
            id: Uuid::new_v4().to_string(),
            source: source.into(),
            event_type: event_type.into(),
            subject: None,
            time: Utc::now(),
            datacontenttype: Some("application/json".to_string()),
            dataschema: None,
            data: None,
            data_base64: None,
            circuitbreaker: None,
            extensions: HashMap::new(),
        }
    }

    /// Create a CloudEvent from a builder.
    pub fn builder() -> CloudEventBuilder {
        CloudEventBuilder::new()
    }

    /// Validate the CloudEvent structure.
    pub fn validate(&self) -> Result<()> {
        if self.specversion.is_empty() {
            return Err(WebhookError::Validation(
                "specversion is required".to_string(),
            ));
        }

        if self.id.is_empty() {
            return Err(WebhookError::Validation("id is required".to_string()));
        }

        if self.source.is_empty() {
            return Err(WebhookError::Validation("source is required".to_string()));
        }

        if self.event_type.is_empty() {
            return Err(WebhookError::Validation("type is required".to_string()));
        }

        Ok(())
    }

    /// Check if this event has binary data.
    pub fn has_binary_data(&self) -> bool {
        self.data_base64.is_some()
    }

    /// Get the data as a specific type.
    pub fn data_as<T: for<'de> Deserialize<'de>>(&self) -> Result<Option<T>> {
        match &self.data {
            Some(data) => {
                let value: T = serde_json::from_value(data.clone())?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Add an extension attribute.
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }

    /// Set the Circuit Breaker extension.
    pub fn with_circuitbreaker(mut self, ext: CircuitBreakerExtension) -> Self {
        self.circuitbreaker = Some(ext);
        self
    }
}

/// Builder for constructing CloudEvents.
#[derive(Debug, Default)]
pub struct CloudEventBuilder {
    id: Option<String>,
    source: Option<String>,
    event_type: Option<String>,
    subject: Option<String>,
    time: Option<DateTime<Utc>>,
    datacontenttype: Option<String>,
    dataschema: Option<String>,
    data: Option<Value>,
    data_base64: Option<String>,
    circuitbreaker: Option<CircuitBreakerExtension>,
    extensions: HashMap<String, Value>,
}

impl CloudEventBuilder {
    /// Create a new CloudEvent builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the event ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the event source.
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set the event type.
    pub fn event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    /// Set the event subject.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Set the event time.
    pub fn time(mut self, time: DateTime<Utc>) -> Self {
        self.time = Some(time);
        self
    }

    /// Set the data content type.
    pub fn datacontenttype(mut self, content_type: impl Into<String>) -> Self {
        self.datacontenttype = Some(content_type.into());
        self
    }

    /// Set the data schema.
    pub fn dataschema(mut self, schema: impl Into<String>) -> Self {
        self.dataschema = Some(schema.into());
        self
    }

    /// Set the event data.
    pub fn data(mut self, data: impl Into<Value>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Set the event data from a serializable value.
    pub fn data_from<T: Serialize>(mut self, data: &T) -> Result<Self> {
        self.data = Some(serde_json::to_value(data)?);
        Ok(self)
    }

    /// Set the base64-encoded binary data.
    pub fn data_base64(mut self, data: impl Into<String>) -> Self {
        self.data_base64 = Some(data.into());
        self
    }

    /// Set the Circuit Breaker extension.
    pub fn circuitbreaker(mut self, ext: CircuitBreakerExtension) -> Self {
        self.circuitbreaker = Some(ext);
        self
    }

    /// Set the workflow to trigger.
    pub fn workflow(mut self, name: impl Into<String>, namespace: impl Into<String>) -> Self {
        let ext = self.circuitbreaker.get_or_insert_with(Default::default);
        ext.workflow_name = Some(name.into());
        ext.namespace = Some(namespace.into());
        self
    }

    /// Set the mapped inputs.
    pub fn inputs(mut self, inputs: HashMap<String, Value>) -> Self {
        let ext = self.circuitbreaker.get_or_insert_with(Default::default);
        ext.inputs = Some(inputs);
        self
    }

    /// Add an extension attribute.
    pub fn extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }

    /// Build the CloudEvent.
    pub fn build(self) -> Result<CloudEvent> {
        let source = self
            .source
            .ok_or_else(|| WebhookError::Validation("source is required".to_string()))?;

        let event_type = self
            .event_type
            .ok_or_else(|| WebhookError::Validation("type is required".to_string()))?;

        let event = CloudEvent {
            specversion: CLOUDEVENTS_SPEC_VERSION.to_string(),
            id: self.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            source,
            event_type,
            subject: self.subject,
            time: self.time.unwrap_or_else(Utc::now),
            datacontenttype: self
                .datacontenttype
                .or(Some("application/json".to_string())),
            dataschema: self.dataschema,
            data: self.data,
            data_base64: self.data_base64,
            circuitbreaker: self.circuitbreaker,
            extensions: self.extensions,
        };

        event.validate()?;
        Ok(event)
    }
}

/// Normalizer for converting webhook payloads to CloudEvents.
pub struct EventNormalizer {
    /// Default source prefix for unknown sources.
    pub default_source_prefix: String,
}

impl Default for EventNormalizer {
    fn default() -> Self {
        Self {
            default_source_prefix: "webhook".to_string(),
        }
    }
}

impl EventNormalizer {
    /// Create a new event normalizer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Normalize a GitHub webhook event.
    pub fn normalize_github(
        &self,
        event_type: &str,
        payload: Value,
        headers: &HashMap<String, String>,
    ) -> Result<CloudEvent> {
        let delivery_id = headers
            .get("x-github-delivery")
            .cloned()
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let repo = payload
            .get("repository")
            .and_then(|r| r.get("full_name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        let source = format!("github.com/{}", repo);
        let ce_type = format!("com.github.{}", event_type.replace('_', "."));

        let subject = match event_type {
            "push" => payload
                .get("ref")
                .and_then(|r| r.as_str())
                .map(String::from),
            "pull_request" => payload
                .get("number")
                .and_then(|n| n.as_u64())
                .map(|n| format!("pull/{}", n)),
            "issues" => payload
                .get("issue")
                .and_then(|i| i.get("number"))
                .and_then(|n| n.as_u64())
                .map(|n| format!("issues/{}", n)),
            _ => None,
        };

        let mut builder = CloudEventBuilder::new()
            .id(delivery_id)
            .source(source)
            .event_type(ce_type)
            .data(payload);

        if let Some(subj) = subject {
            builder = builder.subject(subj);
        }

        builder.build()
    }

    /// Normalize a GitLab webhook event.
    pub fn normalize_gitlab(
        &self,
        event_type: &str,
        payload: Value,
        _headers: &HashMap<String, String>,
    ) -> Result<CloudEvent> {
        let project = payload
            .get("project")
            .and_then(|p| p.get("path_with_namespace"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        let source = format!("gitlab.com/{}", project);
        let ce_type = format!("com.gitlab.{}", event_type.replace(' ', ".").to_lowercase());

        let subject = match event_type {
            "Push Hook" => payload
                .get("ref")
                .and_then(|r| r.as_str())
                .map(String::from),
            "Merge Request Hook" => payload
                .get("object_attributes")
                .and_then(|o| o.get("iid"))
                .and_then(|n| n.as_u64())
                .map(|n| format!("merge_requests/{}", n)),
            _ => None,
        };

        let mut builder = CloudEventBuilder::new()
            .source(source)
            .event_type(ce_type)
            .data(payload);

        if let Some(subj) = subject {
            builder = builder.subject(subj);
        }

        builder.build()
    }

    /// Normalize a Docker Hub webhook event.
    pub fn normalize_dockerhub(
        &self,
        payload: Value,
        _headers: &HashMap<String, String>,
    ) -> Result<CloudEvent> {
        let repo = payload
            .get("repository")
            .and_then(|r| r.get("repo_name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        let source = format!("hub.docker.com/{}", repo);
        let ce_type = "com.docker.hub.push".to_string();

        let tag = payload
            .get("push_data")
            .and_then(|p| p.get("tag"))
            .and_then(|t| t.as_str())
            .map(String::from);

        let mut builder = CloudEventBuilder::new()
            .source(source)
            .event_type(ce_type)
            .data(payload);

        if let Some(t) = tag {
            builder = builder.subject(t);
        }

        builder.build()
    }

    /// Normalize a generic webhook event.
    pub fn normalize_generic(
        &self,
        endpoint_name: &str,
        event_type: Option<&str>,
        payload: Value,
        _headers: &HashMap<String, String>,
    ) -> Result<CloudEvent> {
        let source = format!("{}.{}", self.default_source_prefix, endpoint_name);
        let ce_type = event_type
            .map(|t| format!("com.webhook.{}.{}", endpoint_name, t))
            .unwrap_or_else(|| format!("com.webhook.{}.event", endpoint_name));

        CloudEventBuilder::new()
            .source(source)
            .event_type(ce_type)
            .data(payload)
            .build()
    }

    /// Auto-detect and normalize a webhook event based on headers.
    pub fn normalize_auto(
        &self,
        endpoint_name: &str,
        payload: Value,
        headers: &HashMap<String, String>,
    ) -> Result<CloudEvent> {
        // Try to detect the source from headers
        if headers.contains_key("x-github-event") || headers.contains_key("x-github-delivery") {
            let event_type = headers
                .get("x-github-event")
                .map(String::as_str)
                .unwrap_or("unknown");
            return self.normalize_github(event_type, payload, headers);
        }

        if headers.contains_key("x-gitlab-event") || headers.contains_key("x-gitlab-token") {
            let event_type = headers
                .get("x-gitlab-event")
                .map(String::as_str)
                .unwrap_or("unknown");
            return self.normalize_gitlab(event_type, payload, headers);
        }

        // Check for Docker Hub signature in payload
        if payload.get("push_data").is_some() && payload.get("repository").is_some() {
            if payload
                .get("repository")
                .and_then(|r| r.get("repo_name"))
                .is_some()
            {
                return self.normalize_dockerhub(payload, headers);
            }
        }

        // Fall back to generic normalization
        self.normalize_generic(endpoint_name, None, payload, headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloudevent_new() {
        let event = CloudEvent::new("test-source", "test.event.type");
        assert_eq!(event.specversion, "1.0");
        assert_eq!(event.source, "test-source");
        assert_eq!(event.event_type, "test.event.type");
        assert!(!event.id.is_empty());
    }

    #[test]
    fn test_cloudevent_builder() {
        let event = CloudEventBuilder::new()
            .source("test-source")
            .event_type("test.event")
            .subject("test-subject")
            .data(serde_json::json!({"key": "value"}))
            .build()
            .unwrap();

        assert_eq!(event.source, "test-source");
        assert_eq!(event.event_type, "test.event");
        assert_eq!(event.subject, Some("test-subject".to_string()));
        assert!(event.data.is_some());
    }

    #[test]
    fn test_cloudevent_builder_missing_source() {
        let result = CloudEventBuilder::new().event_type("test.event").build();

        assert!(result.is_err());
    }

    #[test]
    fn test_cloudevent_builder_missing_type() {
        let result = CloudEventBuilder::new().source("test-source").build();

        assert!(result.is_err());
    }

    #[test]
    fn test_cloudevent_validation() {
        let mut event = CloudEvent::new("source", "type");
        assert!(event.validate().is_ok());

        event.source = String::new();
        assert!(event.validate().is_err());
    }

    #[test]
    fn test_cloudevent_with_extension() {
        let event = CloudEvent::new("source", "type").with_extension("custom", "value");

        assert_eq!(
            event.extensions.get("custom"),
            Some(&Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_circuitbreaker_extension() {
        let ext = CircuitBreakerExtension {
            workflow_name: Some("my-workflow".to_string()),
            namespace: Some("production".to_string()),
            inputs: Some(HashMap::from([(
                "key".to_string(),
                Value::String("value".to_string()),
            )])),
            ..Default::default()
        };

        let event = CloudEvent::new("source", "type").with_circuitbreaker(ext);

        assert!(event.circuitbreaker.is_some());
        let cb = event.circuitbreaker.unwrap();
        assert_eq!(cb.workflow_name, Some("my-workflow".to_string()));
    }

    #[test]
    fn test_normalize_github() {
        let normalizer = EventNormalizer::new();

        let payload = serde_json::json!({
            "ref": "refs/heads/main",
            "repository": {
                "full_name": "myorg/myrepo"
            },
            "head_commit": {
                "id": "abc123"
            }
        });

        let mut headers = HashMap::new();
        headers.insert("x-github-delivery".to_string(), "delivery-123".to_string());

        let event = normalizer
            .normalize_github("push", payload, &headers)
            .unwrap();

        assert_eq!(event.id, "delivery-123");
        assert_eq!(event.source, "github.com/myorg/myrepo");
        assert_eq!(event.event_type, "com.github.push");
        assert_eq!(event.subject, Some("refs/heads/main".to_string()));
    }

    #[test]
    fn test_normalize_dockerhub() {
        let normalizer = EventNormalizer::new();

        let payload = serde_json::json!({
            "push_data": {
                "tag": "v1.0.0"
            },
            "repository": {
                "repo_name": "myorg/myimage"
            }
        });

        let event = normalizer
            .normalize_dockerhub(payload, &HashMap::new())
            .unwrap();

        assert_eq!(event.source, "hub.docker.com/myorg/myimage");
        assert_eq!(event.event_type, "com.docker.hub.push");
        assert_eq!(event.subject, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_normalize_auto_github() {
        let normalizer = EventNormalizer::new();

        let payload = serde_json::json!({
            "ref": "refs/heads/main",
            "repository": {
                "full_name": "myorg/myrepo"
            }
        });

        let mut headers = HashMap::new();
        headers.insert("x-github-event".to_string(), "push".to_string());
        headers.insert("x-github-delivery".to_string(), "abc123".to_string());

        let event = normalizer
            .normalize_auto("github-webhook", payload, &headers)
            .unwrap();

        assert!(event.source.contains("github.com"));
        assert_eq!(event.event_type, "com.github.push");
    }

    #[test]
    fn test_normalize_generic() {
        let normalizer = EventNormalizer::new();

        let payload = serde_json::json!({
            "message": "test"
        });

        let event = normalizer
            .normalize_generic(
                "custom-endpoint",
                Some("custom.event"),
                payload,
                &HashMap::new(),
            )
            .unwrap();

        assert_eq!(event.source, "webhook.custom-endpoint");
        assert_eq!(event.event_type, "com.webhook.custom-endpoint.custom.event");
    }

    #[test]
    fn test_cloudevent_serialization() {
        let event = CloudEventBuilder::new()
            .source("test-source")
            .event_type("test.event")
            .data(serde_json::json!({"key": "value"}))
            .build()
            .unwrap();

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("specversion"));
        assert!(json.contains("1.0"));
        assert!(json.contains("test-source"));

        let parsed: CloudEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.source, event.source);
        assert_eq!(parsed.event_type, event.event_type);
    }

    #[test]
    fn test_data_as() {
        #[derive(Deserialize, PartialEq, Debug)]
        struct TestData {
            message: String,
        }

        let event = CloudEventBuilder::new()
            .source("test")
            .event_type("test")
            .data(serde_json::json!({"message": "hello"}))
            .build()
            .unwrap();

        let data: Option<TestData> = event.data_as().unwrap();
        assert!(data.is_some());
        assert_eq!(data.unwrap().message, "hello");
    }
}
