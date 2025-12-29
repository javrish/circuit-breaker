//! HTTP endpoint handlers for webhook processing.
//!
//! This module provides the Axum handlers for:
//! - Receiving and processing webhook events
//! - Managing webhook endpoint configurations
//! - Health and status endpoints
//! - Event replay and debugging

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AuthValidator, Authenticate};
use crate::cloudevents::{CircuitBreakerExtension, CloudEventBuilder, EventNormalizer};
use crate::error::WebhookError;
use crate::{AppState, EventStatus, TriggerMapping, WebhookEndpoint, WebhookEvent};

// ==================== Response Types ====================

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Service version.
    pub version: String,
    /// NATS connection status.
    pub nats_connected: bool,
    /// Number of registered endpoints.
    pub endpoint_count: usize,
}

/// Webhook received response.
#[derive(Debug, Serialize)]
pub struct WebhookReceivedResponse {
    /// Event ID assigned to this webhook.
    pub event_id: String,
    /// Processing status.
    pub status: String,
    /// Message.
    pub message: String,
    /// Matched triggers (if any).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matched_triggers: Vec<String>,
}

/// Endpoint list response.
#[derive(Debug, Serialize)]
pub struct ListEndpointsResponse {
    /// List of endpoints.
    pub endpoints: Vec<EndpointSummary>,
    /// Total count.
    pub total: usize,
}

/// Endpoint summary for listing.
#[derive(Debug, Serialize)]
pub struct EndpointSummary {
    /// Endpoint ID.
    pub id: String,
    /// Endpoint name.
    pub name: String,
    /// Endpoint namespace.
    pub namespace: String,
    /// Endpoint path.
    pub path: String,
    /// Whether endpoint is enabled.
    pub enabled: bool,
    /// Number of triggers.
    pub trigger_count: usize,
    /// Creation timestamp.
    pub created_at: String,
}

/// Event list response.
#[derive(Debug, Serialize)]
pub struct ListEventsResponse {
    /// List of events.
    pub events: Vec<EventSummary>,
    /// Total count.
    pub total: usize,
}

/// Event summary for listing.
#[derive(Debug, Serialize)]
pub struct EventSummary {
    /// Event ID.
    pub id: String,
    /// Endpoint name.
    pub endpoint_name: String,
    /// Source type.
    pub source_type: String,
    /// Event type.
    pub event_type: String,
    /// Processing status.
    pub status: String,
    /// Received timestamp.
    pub received_at: String,
}

/// Query parameters for event listing.
#[derive(Debug, Deserialize, Default)]
pub struct ListEventsQuery {
    /// Filter by endpoint ID.
    pub endpoint_id: Option<String>,
    /// Filter by status.
    pub status: Option<String>,
    /// Maximum number of events to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

// ==================== Health Handlers ====================

/// Health check handler.
pub async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        nats_connected: state.nats.is_some(),
        endpoint_count: state.endpoints.len(),
    })
}

/// Readiness check handler.
pub async fn ready_handler(State(state): State<Arc<AppState>>) -> Response {
    if state.nats.is_some() || state.endpoints.is_empty() {
        (StatusCode::OK, "ready").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready").into_response()
    }
}

/// Liveness check handler.
pub async fn live_handler() -> Response {
    (StatusCode::OK, "alive").into_response()
}

// ==================== Webhook Handlers ====================

/// Main webhook handler - receives webhooks at dynamic paths.
pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookReceivedResponse>, WebhookError> {
    let full_path = format!("/webhooks/{}", path);
    let source_ip = extract_source_ip(&headers);
    process_webhook(state, &full_path, headers, source_ip, body).await
}

/// Extract source IP from headers (X-Forwarded-For, X-Real-IP) or use fallback.
fn extract_source_ip(headers: &HeaderMap) -> IpAddr {
    // Try X-Forwarded-For first (may contain multiple IPs, take the first)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            if let Some(first_ip) = xff_str.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    // Try X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            if let Ok(ip) = ip_str.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }

    // Fallback to localhost
    "127.0.0.1".parse().unwrap()
}

/// Process a webhook request.
async fn process_webhook(
    state: Arc<AppState>,
    path: &str,
    headers: HeaderMap,
    source_ip: IpAddr,
    body: Bytes,
) -> Result<Json<WebhookReceivedResponse>, WebhookError> {
    let event_id = Uuid::new_v4();
    let received_at = Utc::now();

    // Find the endpoint for this path
    let endpoint = state
        .get_endpoint_by_path(path)
        .ok_or_else(|| WebhookError::EndpointNotFound(path.to_string()))?;

    // Check if endpoint is enabled
    if !endpoint.enabled {
        return Err(WebhookError::EndpointDisabled(endpoint.name.clone()));
    }

    // Check payload size
    if body.len() > endpoint.max_payload_bytes {
        return Err(WebhookError::PayloadTooLarge {
            size: body.len(),
            limit: endpoint.max_payload_bytes,
        });
    }

    // Convert headers to HashMap
    let headers_map: HashMap<String, String> = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_lowercase(), v.to_string()))
        })
        .collect();

    // Parse payload
    let payload: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
        WebhookError::InvalidPayload(format!("Failed to parse JSON payload: {}", e))
    })?;

    // Get event type from headers (provider-specific)
    let event_type = detect_event_type(&headers_map, &payload);

    // Create webhook event record
    let mut webhook_event = WebhookEvent {
        id: event_id,
        endpoint_id: endpoint.id.clone(),
        endpoint_name: endpoint.name.clone(),
        source_type: detect_source_type(&headers_map),
        event_type: event_type.clone(),
        payload: payload.clone(),
        headers: headers_map.clone(),
        source_ip: source_ip.to_string(),
        received_at,
        auth_valid: false,
        status: EventStatus::Received,
    };

    // Validate authentication if configured
    if let Some(ref auth_config) = endpoint.auth {
        let validator = AuthValidator::new(auth_config.clone());
        let auth_result = validator
            .validate(&headers_map, &body, Some(source_ip))
            .await?;

        if !auth_result.valid {
            webhook_event.status = EventStatus::Failed;
            state.store_event(webhook_event);
            return Err(WebhookError::AuthenticationFailed(
                auth_result
                    .failure_reason
                    .unwrap_or_else(|| "Authentication failed".to_string()),
            ));
        }
        webhook_event.auth_valid = true;
    } else {
        webhook_event.auth_valid = true;
    }

    // Update status
    webhook_event.status = EventStatus::Validating;
    state.update_event_status(event_id, EventStatus::Validating);

    // Check IP allowlist
    if !endpoint.ip_allowlist.is_empty() {
        let ip_str = source_ip.to_string();
        let allowed = endpoint.ip_allowlist.iter().any(|allowed| {
            allowed == "*" || allowed == &ip_str || allowed.contains('/')
            // TODO: CIDR matching
        });
        if !allowed {
            webhook_event.status = EventStatus::Failed;
            state.store_event(webhook_event);
            return Err(WebhookError::IpNotAllowed(ip_str));
        }
    }

    // Match triggers
    webhook_event.status = EventStatus::Normalizing;
    let matched_triggers = match_triggers(&endpoint.triggers, &event_type, &payload);

    if matched_triggers.is_empty() {
        webhook_event.status = EventStatus::Filtered;
        state.store_event(webhook_event);

        return Ok(Json(WebhookReceivedResponse {
            event_id: event_id.to_string(),
            status: "filtered".to_string(),
            message: "No matching triggers found".to_string(),
            matched_triggers: vec![],
        }));
    }

    // Normalize to CloudEvent format
    let normalizer = EventNormalizer::new();
    let cloud_event = normalizer.normalize_auto(&endpoint.name, payload.clone(), &headers_map)?;

    // Publish to NATS for each matched trigger
    let mut published_triggers = Vec::new();

    if let Some(ref nats) = state.nats {
        for trigger in &matched_triggers {
            // Build inputs from mapping
            let inputs = map_inputs(&trigger.inputs, &payload);

            // Create CloudEvent with Circuit Breaker extension
            let cb_extension = CircuitBreakerExtension {
                workflow_name: Some(trigger.workflow.clone()),
                namespace: trigger.workflow_namespace.clone(),
                inputs: Some(inputs),
                trigger_name: Some(format!("{}/{}", endpoint.name, trigger.event)),
                endpoint_id: Some(endpoint.id.clone()),
                raw_event_type: Some(event_type.clone()),
                trace_id: Some(event_id.to_string()),
            };

            let enriched_event = CloudEventBuilder::new()
                .id(format!("{}-{}", event_id, trigger.workflow))
                .source(cloud_event.source.clone())
                .event_type(cloud_event.event_type.clone())
                .subject(cloud_event.subject.clone().unwrap_or_default())
                .data(payload.clone())
                .circuitbreaker(cb_extension)
                .build()?;

            // Publish to NATS
            let subject = cb_nats::subjects::webhook_received(&endpoint.name);
            let event_json = serde_json::to_vec(&enriched_event)?;

            if let Err(e) = nats.publish(subject.clone(), event_json.into()).await {
                tracing::error!(
                    error = %e,
                    subject = %subject,
                    "Failed to publish webhook event to NATS"
                );
            } else {
                published_triggers.push(trigger.workflow.clone());
                tracing::info!(
                    event_id = %event_id,
                    workflow = %trigger.workflow,
                    subject = %subject,
                    "Published webhook event to NATS"
                );
            }
        }
    }

    webhook_event.status = if published_triggers.is_empty() {
        EventStatus::Normalized
    } else {
        EventStatus::Published
    };

    state.store_event(webhook_event);

    Ok(Json(WebhookReceivedResponse {
        event_id: event_id.to_string(),
        status: "accepted".to_string(),
        message: format!(
            "Event processed, {} triggers matched",
            matched_triggers.len()
        ),
        matched_triggers: published_triggers,
    }))
}

/// Detect event type from headers (provider-specific).
fn detect_event_type(headers: &HashMap<String, String>, payload: &serde_json::Value) -> String {
    // GitHub
    if let Some(event) = headers.get("x-github-event") {
        return event.clone();
    }

    // GitLab
    if let Some(event) = headers.get("x-gitlab-event") {
        return event.clone();
    }

    // Bitbucket
    if let Some(event) = headers.get("x-event-key") {
        return event.clone();
    }

    // Stripe
    if let Some(event_type) = payload.get("type").and_then(|t| t.as_str()) {
        if headers.contains_key("stripe-signature") {
            return event_type.to_string();
        }
    }

    // Generic: try to extract from payload
    if let Some(event) = payload.get("event").and_then(|e| e.as_str()) {
        return event.to_string();
    }

    if let Some(action) = payload.get("action").and_then(|a| a.as_str()) {
        return action.to_string();
    }

    "unknown".to_string()
}

/// Detect source type from headers.
fn detect_source_type(headers: &HashMap<String, String>) -> String {
    if headers.contains_key("x-github-event") || headers.contains_key("x-github-delivery") {
        return "github".to_string();
    }

    if headers.contains_key("x-gitlab-event") || headers.contains_key("x-gitlab-token") {
        return "gitlab".to_string();
    }

    if headers.contains_key("x-event-key") {
        return "bitbucket".to_string();
    }

    if headers.contains_key("stripe-signature") {
        return "stripe".to_string();
    }

    if headers
        .get("user-agent")
        .map_or(false, |ua| ua.contains("Docker-Hub"))
    {
        return "dockerhub".to_string();
    }

    "generic".to_string()
}

/// Match triggers against the event.
fn match_triggers(
    triggers: &[TriggerMapping],
    event_type: &str,
    payload: &serde_json::Value,
) -> Vec<TriggerMapping> {
    triggers
        .iter()
        .filter(|trigger| {
            // Match event type (supports wildcards)
            let event_matches = trigger.event == "*"
                || trigger.event == event_type
                || event_type.starts_with(&trigger.event);

            if !event_matches {
                return false;
            }

            // Apply filter if present
            if let Some(ref filter) = trigger.filter {
                // Simple filter evaluation (CEL would be used in production)
                return evaluate_simple_filter(filter, payload);
            }

            true
        })
        .cloned()
        .collect()
}

/// Simple filter evaluation (placeholder for CEL).
fn evaluate_simple_filter(filter: &str, payload: &serde_json::Value) -> bool {
    // Very basic filter evaluation
    // In production, this would use CEL (cel-interpreter)

    // Handle "field == value" patterns
    if filter.contains("==") {
        let parts: Vec<&str> = filter.split("==").collect();
        if parts.len() == 2 {
            let field = parts[0].trim();
            let expected = parts[1].trim().trim_matches('"').trim_matches('\'');

            // Navigate to the field using dot notation
            let value = get_json_path(payload, field);

            return value
                .and_then(|v| v.as_str())
                .map_or(false, |v| v == expected);
        }
    }

    // Handle "field.startsWith(value)" patterns
    if filter.contains(".startsWith(") {
        if let Some(start) = filter.find(".startsWith(") {
            let field = filter[..start].trim();
            let pattern_start = start + 12;
            if let Some(end) = filter[pattern_start..].find(')') {
                let expected = filter[pattern_start..pattern_start + end]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');

                let value = get_json_path(payload, field);

                return value
                    .and_then(|v| v.as_str())
                    .map_or(false, |v| v.starts_with(expected));
            }
        }
    }

    // If we can't parse the filter, assume it matches
    true
}

/// Get a value from JSON using dot notation path.
fn get_json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;

    for part in path.split('.') {
        let part = part.trim();

        // Handle array indexing
        if part.contains('[') {
            if let Some(bracket_pos) = part.find('[') {
                let field = &part[..bracket_pos];
                if !field.is_empty() {
                    current = current.get(field)?;
                }

                let index_str = &part[bracket_pos + 1..part.len() - 1];
                if let Ok(index) = index_str.parse::<usize>() {
                    current = current.get(index)?;
                }
            }
        } else {
            current = current.get(part)?;
        }
    }

    Some(current)
}

/// Map inputs from the payload using template expressions.
fn map_inputs(
    input_mappings: &HashMap<String, String>,
    payload: &serde_json::Value,
) -> HashMap<String, serde_json::Value> {
    input_mappings
        .iter()
        .filter_map(|(key, template)| {
            let value = evaluate_template(template, payload)?;
            Some((key.clone(), value))
        })
        .collect()
}

/// Evaluate a template expression against the payload.
fn evaluate_template(template: &str, payload: &serde_json::Value) -> Option<serde_json::Value> {
    // Handle {{ .path.to.field }} syntax
    let trimmed = template.trim();

    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        let path = trimmed[2..trimmed.len() - 2].trim();

        // Handle .field.subfield syntax
        let path = path.strip_prefix('.').unwrap_or(path);

        // Handle pipe operations like | trimPrefix
        let (path, _pipe_ops) = if let Some(pipe_pos) = path.find('|') {
            (path[..pipe_pos].trim(), Some(&path[pipe_pos + 1..]))
        } else {
            (path, None)
        };

        let value = get_json_path(payload, path)?;

        // TODO: Apply pipe operations

        return Some(value.clone());
    }

    // Handle direct path references (without {{ }})
    if trimmed.starts_with('.') {
        let path = &trimmed[1..];
        return get_json_path(payload, path).cloned();
    }

    // Handle literal string values
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        let literal = &trimmed[1..trimmed.len() - 1];
        return Some(serde_json::Value::String(literal.to_string()));
    }

    // Return as literal string
    Some(serde_json::Value::String(template.to_string()))
}

// ==================== Endpoint Management Handlers ====================

/// List all registered endpoints.
pub async fn list_endpoints_handler(
    State(state): State<Arc<AppState>>,
) -> Json<ListEndpointsResponse> {
    let endpoints: Vec<EndpointSummary> = state
        .endpoints
        .iter()
        .map(|e| EndpointSummary {
            id: e.id.clone(),
            name: e.name.clone(),
            namespace: e.namespace.clone(),
            path: e.path.clone(),
            enabled: e.enabled,
            trigger_count: e.triggers.len(),
            created_at: e.created_at.to_rfc3339(),
        })
        .collect();

    let total = endpoints.len();

    Json(ListEndpointsResponse { endpoints, total })
}

/// Get a specific endpoint by ID.
pub async fn get_endpoint_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<WebhookEndpoint>, WebhookError> {
    state
        .get_endpoint(&id)
        .map(Json)
        .ok_or_else(|| WebhookError::EndpointNotFound(id))
}

/// Create a new endpoint.
pub async fn create_endpoint_handler(
    State(state): State<Arc<AppState>>,
    Json(endpoint): Json<WebhookEndpoint>,
) -> Result<(StatusCode, Json<WebhookEndpoint>), WebhookError> {
    // Check if endpoint already exists
    if state.get_endpoint(&endpoint.id).is_some() {
        return Err(WebhookError::Validation(format!(
            "Endpoint {} already exists",
            endpoint.id
        )));
    }

    // Check if path is already in use
    if state.get_endpoint_by_path(&endpoint.path).is_some() {
        return Err(WebhookError::Validation(format!(
            "Path {} is already in use",
            endpoint.path
        )));
    }

    state.register_endpoint(endpoint.clone());

    tracing::info!(
        endpoint_id = %endpoint.id,
        path = %endpoint.path,
        "Registered new webhook endpoint"
    );

    Ok((StatusCode::CREATED, Json(endpoint)))
}

/// Delete an endpoint.
pub async fn delete_endpoint_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, WebhookError> {
    state
        .unregister_endpoint(&id)
        .map(|_| StatusCode::NO_CONTENT)
        .ok_or_else(|| WebhookError::EndpointNotFound(id))
}

/// Update an endpoint.
pub async fn update_endpoint_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut endpoint): Json<WebhookEndpoint>,
) -> Result<Json<WebhookEndpoint>, WebhookError> {
    // Ensure the ID matches
    if endpoint.id != id {
        endpoint.id = id.clone();
    }

    // Check if endpoint exists
    if state.get_endpoint(&id).is_none() {
        return Err(WebhookError::EndpointNotFound(id));
    }

    // Update timestamp
    endpoint.updated_at = Utc::now();

    // Re-register (this handles path updates)
    state.unregister_endpoint(&id);
    state.register_endpoint(endpoint.clone());

    Ok(Json(endpoint))
}

// ==================== Event Handlers ====================

/// List recent events.
pub async fn list_events_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListEventsQuery>,
) -> Json<ListEventsResponse> {
    let mut events: Vec<EventSummary> = state
        .recent_events
        .iter()
        .filter(|e| {
            // Apply filters
            if let Some(ref endpoint_id) = query.endpoint_id {
                if &e.endpoint_id != endpoint_id {
                    return false;
                }
            }
            if let Some(ref status) = query.status {
                let status_str = format!("{:?}", e.status).to_lowercase();
                if !status_str.contains(&status.to_lowercase()) {
                    return false;
                }
            }
            true
        })
        .map(|e| EventSummary {
            id: e.id.to_string(),
            endpoint_name: e.endpoint_name.clone(),
            source_type: e.source_type.clone(),
            event_type: e.event_type.clone(),
            status: format!("{:?}", e.status).to_lowercase(),
            received_at: e.received_at.to_rfc3339(),
        })
        .collect();

    // Sort by received_at descending
    events.sort_by(|a, b| b.received_at.cmp(&a.received_at));

    let total = events.len();

    // Apply pagination
    let events: Vec<EventSummary> = events
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .collect();

    Json(ListEventsResponse { events, total })
}

/// Get a specific event.
pub async fn get_event_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<WebhookEvent>, WebhookError> {
    let uuid = Uuid::parse_str(&id)
        .map_err(|_| WebhookError::EventNotFound(format!("Invalid event ID: {}", id)))?;

    state
        .recent_events
        .get(&uuid)
        .map(|e| Json(e.clone()))
        .ok_or_else(|| WebhookError::EventNotFound(id))
}

/// Replay an event (re-process it).
pub async fn replay_event_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<WebhookReceivedResponse>, WebhookError> {
    let uuid = Uuid::parse_str(&id)
        .map_err(|_| WebhookError::EventNotFound(format!("Invalid event ID: {}", id)))?;

    let event = state
        .recent_events
        .get(&uuid)
        .ok_or_else(|| WebhookError::EventNotFound(id.clone()))?;

    // Get the endpoint
    let endpoint = state
        .get_endpoint(&event.endpoint_id)
        .ok_or_else(|| WebhookError::EndpointNotFound(event.endpoint_id.clone()))?;

    // Match triggers again
    let matched_triggers = match_triggers(&endpoint.triggers, &event.event_type, &event.payload);

    if matched_triggers.is_empty() {
        return Ok(Json(WebhookReceivedResponse {
            event_id: id,
            status: "filtered".to_string(),
            message: "No matching triggers found on replay".to_string(),
            matched_triggers: vec![],
        }));
    }

    // Normalize and publish
    let normalizer = EventNormalizer::new();
    let cloud_event =
        normalizer.normalize_auto(&endpoint.name, event.payload.clone(), &event.headers)?;

    let mut published_triggers = Vec::new();

    if let Some(ref nats) = state.nats {
        for trigger in &matched_triggers {
            let inputs = map_inputs(&trigger.inputs, &event.payload);

            let cb_extension = CircuitBreakerExtension {
                workflow_name: Some(trigger.workflow.clone()),
                namespace: trigger.workflow_namespace.clone(),
                inputs: Some(inputs),
                trigger_name: Some(format!("{}/{}", endpoint.name, trigger.event)),
                endpoint_id: Some(endpoint.id.clone()),
                raw_event_type: Some(event.event_type.clone()),
                trace_id: Some(format!("{}-replay", event.id)),
            };

            let enriched_event = CloudEventBuilder::new()
                .id(format!("{}-replay-{}", event.id, trigger.workflow))
                .source(cloud_event.source.clone())
                .event_type(cloud_event.event_type.clone())
                .data(event.payload.clone())
                .circuitbreaker(cb_extension)
                .build()?;

            let subject = cb_nats::subjects::webhook_received(&endpoint.name);
            let event_json = serde_json::to_vec(&enriched_event)?;

            if let Err(e) = nats.publish(subject, event_json.into()).await {
                tracing::error!(error = %e, "Failed to replay event to NATS");
            } else {
                published_triggers.push(trigger.workflow.clone());
            }
        }
    }

    Ok(Json(WebhookReceivedResponse {
        event_id: format!("{}-replay", id),
        status: "replayed".to_string(),
        message: format!(
            "Event replayed, {} triggers matched",
            matched_triggers.len()
        ),
        matched_triggers: published_triggers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_event_type_github() {
        let mut headers = HashMap::new();
        headers.insert("x-github-event".to_string(), "push".to_string());

        let payload = serde_json::json!({});
        assert_eq!(detect_event_type(&headers, &payload), "push");
    }

    #[test]
    fn test_detect_event_type_gitlab() {
        let mut headers = HashMap::new();
        headers.insert("x-gitlab-event".to_string(), "Push Hook".to_string());

        let payload = serde_json::json!({});
        assert_eq!(detect_event_type(&headers, &payload), "Push Hook");
    }

    #[test]
    fn test_detect_source_type() {
        let mut headers = HashMap::new();
        headers.insert("x-github-event".to_string(), "push".to_string());
        assert_eq!(detect_source_type(&headers), "github");

        let mut headers = HashMap::new();
        headers.insert("x-gitlab-event".to_string(), "Push Hook".to_string());
        assert_eq!(detect_source_type(&headers), "gitlab");

        let mut headers = HashMap::new();
        headers.insert("stripe-signature".to_string(), "sig".to_string());
        assert_eq!(detect_source_type(&headers), "stripe");
    }

    #[test]
    fn test_get_json_path() {
        let payload = serde_json::json!({
            "repository": {
                "full_name": "myorg/myrepo"
            },
            "commits": [
                {"id": "abc123"},
                {"id": "def456"}
            ]
        });

        assert_eq!(
            get_json_path(&payload, "repository.full_name")
                .and_then(|v| v.as_str())
                .unwrap(),
            "myorg/myrepo"
        );

        assert_eq!(
            get_json_path(&payload, "commits[0].id")
                .and_then(|v| v.as_str())
                .unwrap(),
            "abc123"
        );
    }

    #[test]
    fn test_evaluate_template() {
        let payload = serde_json::json!({
            "repository": {
                "full_name": "myorg/myrepo"
            },
            "ref": "refs/heads/main"
        });

        let result = evaluate_template("{{ .repository.full_name }}", &payload);
        assert_eq!(
            result,
            Some(serde_json::Value::String("myorg/myrepo".to_string()))
        );

        let result = evaluate_template(".ref", &payload);
        assert_eq!(
            result,
            Some(serde_json::Value::String("refs/heads/main".to_string()))
        );
    }

    #[test]
    fn test_match_triggers() {
        let triggers = vec![
            TriggerMapping {
                event: "push".to_string(),
                filter: None,
                workflow: "build".to_string(),
                workflow_namespace: Some("default".to_string()),
                inputs: HashMap::new(),
            },
            TriggerMapping {
                event: "pull_request".to_string(),
                filter: Some("action == 'opened'".to_string()),
                workflow: "pr-check".to_string(),
                workflow_namespace: Some("default".to_string()),
                inputs: HashMap::new(),
            },
        ];

        let payload = serde_json::json!({"action": "opened"});

        // Should match push trigger
        let matches = match_triggers(&triggers, "push", &payload);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].workflow, "build");

        // Should match pull_request trigger with filter
        let matches = match_triggers(&triggers, "pull_request", &payload);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].workflow, "pr-check");

        // Should not match with different action
        let payload = serde_json::json!({"action": "closed"});
        let matches = match_triggers(&triggers, "pull_request", &payload);
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_evaluate_simple_filter() {
        let payload = serde_json::json!({
            "ref": "refs/heads/main",
            "action": "opened"
        });

        assert!(evaluate_simple_filter("ref == 'refs/heads/main'", &payload));
        assert!(!evaluate_simple_filter(
            "ref == 'refs/heads/develop'",
            &payload
        ));
        assert!(evaluate_simple_filter(
            "ref.startsWith('refs/heads/')",
            &payload
        ));
    }

    #[test]
    fn test_map_inputs() {
        let payload = serde_json::json!({
            "repository": {
                "full_name": "myorg/myrepo"
            },
            "head_commit": {
                "id": "abc123"
            }
        });

        let mut mappings = HashMap::new();
        mappings.insert(
            "repo".to_string(),
            "{{ .repository.full_name }}".to_string(),
        );
        mappings.insert("commit".to_string(), "{{ .head_commit.id }}".to_string());

        let inputs = map_inputs(&mappings, &payload);

        assert_eq!(
            inputs.get("repo"),
            Some(&serde_json::Value::String("myorg/myrepo".to_string()))
        );
        assert_eq!(
            inputs.get("commit"),
            Some(&serde_json::Value::String("abc123".to_string()))
        );
    }
}
