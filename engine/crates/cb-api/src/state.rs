//! Event-sourced state management for workflow runs.
//!
//! This module provides state reconstruction from NATS JetStream events.
//! Instead of maintaining mutable in-memory state, we derive current state
//! by replaying events from the stream.
//!
//! ## Event Types
//!
//! The following events affect run state:
//! - `token.produced` - Add token to a place
//! - `token.consumed` - Remove token from a place
//! - `token.injected` - Manually inject token with data
//! - `token.updated` - Update token data (gate pattern)
//! - `transition.completed` - Transition finished, tokens moved
//! - `transition.failed` - Transition failed
//! - `workflow.completed` - Run completed
//! - `workflow.failed` - Run failed
//! - `workflow.cancelled` - Run cancelled

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

use cb_core::events::{
    EventType, TokenData, TokenProducedPayload, TokenConsumedPayload,
    TokenInjectedPayload, TokenUpdatedPayload, TransitionCompletedPayload,
    TransitionFailedPayload, WorkflowCompletedPayload, WorkflowFailedPayload,
    WorkflowCancelledPayload, WorkflowStartedPayload,
};
use cb_core::workflow::Workflow;
use cb_nats::NatsClient;

/// Run status derived from events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Token in a place with its data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub token_id: Uuid,
    pub place_id: String,
    pub data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl From<TokenData> for Token {
    fn from(td: TokenData) -> Self {
        Self {
            token_id: td.token_id,
            place_id: td.place_id,
            data: td.data,
            created_at: td.created_at,
        }
    }
}

/// Current state of a workflow run, derived from events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub run_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Tokens currently in each place (by place_id -> list of tokens)
    pub tokens: HashMap<String, Vec<Token>>,
    /// Token count per place (derived from tokens map)
    pub marking: HashMap<String, u32>,
    /// Error info if failed
    pub error: Option<ErrorInfo>,
    /// Last event sequence number processed
    pub last_seq: u64,
}

/// Error information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    pub transition: Option<String>,
}

impl RunState {
    /// Create a new run state from a workflow started event.
    pub fn new(
        run_id: Uuid,
        workflow_id: Uuid,
        workflow_name: String,
        initial_marking: HashMap<String, u32>,
    ) -> Self {
        let now = Utc::now();

        // Create initial tokens for places with initial marking
        let mut tokens: HashMap<String, Vec<Token>> = HashMap::new();
        for (place_id, count) in &initial_marking {
            let place_tokens: Vec<Token> = (0..*count)
                .map(|_| Token {
                    token_id: Uuid::new_v4(),
                    place_id: place_id.clone(),
                    data: None,
                    created_at: now,
                })
                .collect();
            tokens.insert(place_id.clone(), place_tokens);
        }

        Self {
            run_id,
            workflow_id,
            workflow_name,
            status: RunStatus::Running,
            started_at: now,
            completed_at: None,
            tokens,
            marking: initial_marking,
            error: None,
            last_seq: 0,
        }
    }

    /// Apply an event to update the state.
    pub fn apply_event(&mut self, event_type: &str, payload: &serde_json::Value, seq: u64) {
        self.last_seq = seq;

        match event_type {
            "token.produced" => {
                if let Ok(p) = serde_json::from_value::<TokenProducedPayload>(payload.clone()) {
                    self.add_token(p.token.into());
                }
            }
            "token.consumed" => {
                if let Ok(p) = serde_json::from_value::<TokenConsumedPayload>(payload.clone()) {
                    self.remove_token(&p.token.place_id, &p.token.token_id);
                }
            }
            "token.injected" => {
                if let Ok(p) = serde_json::from_value::<TokenInjectedPayload>(payload.clone()) {
                    self.add_token(p.token.into());
                }
            }
            "token.updated" => {
                if let Ok(p) = serde_json::from_value::<TokenUpdatedPayload>(payload.clone()) {
                    self.update_token(&p.place_id, &p.token.token_id, p.token.data);
                }
            }
            "workflow.completed" => {
                if let Ok(_p) = serde_json::from_value::<WorkflowCompletedPayload>(payload.clone()) {
                    self.status = RunStatus::Completed;
                    self.completed_at = Some(Utc::now());
                }
            }
            "workflow.failed" => {
                if let Ok(p) = serde_json::from_value::<WorkflowFailedPayload>(payload.clone()) {
                    self.status = RunStatus::Failed;
                    self.completed_at = Some(Utc::now());
                    self.error = Some(ErrorInfo {
                        code: p.error.code,
                        message: p.error.message,
                        transition: p.failed_transition,
                    });
                }
            }
            "workflow.cancelled" => {
                if let Ok(_p) = serde_json::from_value::<WorkflowCancelledPayload>(payload.clone()) {
                    self.status = RunStatus::Cancelled;
                    self.completed_at = Some(Utc::now());
                }
            }
            _ => {
                debug!(event_type, "Unknown event type, ignoring");
            }
        }
    }

    /// Add a token to a place.
    fn add_token(&mut self, token: Token) {
        let place_id = token.place_id.clone();
        self.tokens
            .entry(place_id.clone())
            .or_insert_with(Vec::new)
            .push(token);
        *self.marking.entry(place_id).or_insert(0) += 1;
    }

    /// Remove a token from a place.
    fn remove_token(&mut self, place_id: &str, token_id: &Uuid) {
        if let Some(tokens) = self.tokens.get_mut(place_id) {
            tokens.retain(|t| &t.token_id != token_id);
            let count = self.marking.entry(place_id.to_string()).or_insert(0);
            if *count > 0 {
                *count -= 1;
            }
        }
    }

    /// Update token data (for gate pattern).
    fn update_token(&mut self, place_id: &str, token_id: &Uuid, new_data: Option<serde_json::Value>) {
        if let Some(tokens) = self.tokens.get_mut(place_id) {
            if let Some(token) = tokens.iter_mut().find(|t| &t.token_id == token_id) {
                token.data = new_data;
            }
        }
    }

    /// Get aggregated token data for a place (merge all token data).
    pub fn get_place_data(&self, place_id: &str) -> Option<serde_json::Value> {
        self.tokens.get(place_id).and_then(|tokens| {
            let mut merged = serde_json::Map::new();
            for token in tokens {
                if let Some(data) = &token.data {
                    if let Some(obj) = data.as_object() {
                        for (k, v) in obj {
                            merged.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            if merged.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(merged))
            }
        })
    }

    /// Get all token data as a map (place_id -> merged data).
    pub fn get_all_token_data(&self) -> HashMap<String, serde_json::Value> {
        let mut result = HashMap::new();
        for place_id in self.tokens.keys() {
            if let Some(data) = self.get_place_data(place_id) {
                result.insert(place_id.clone(), data);
            }
        }
        result
    }
}

/// State store that reconstructs state from NATS events.
pub struct EventSourcedStateStore {
    nats: Arc<NatsClient>,
}

impl EventSourcedStateStore {
    /// Create a new event-sourced state store.
    pub fn new(nats: Arc<NatsClient>) -> Self {
        Self { nats }
    }

    /// Reconstruct run state by replaying events from NATS.
    pub async fn get_run_state(&self, run_id: Uuid) -> Result<Option<RunState>, String> {
        let js = self.nats.jetstream();

        // Get the stream
        let stream = js.get_stream("CB_RUNS").await
            .map_err(|e| format!("Failed to get stream: {}", e))?;

        // Create a consumer that filters to this run's events
        let filter_subject = format!("cb.runs.{}.>", run_id);

        let consumer_config = async_nats::jetstream::consumer::pull::Config {
            filter_subject: filter_subject.clone(),
            deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::All,
            ack_policy: async_nats::jetstream::consumer::AckPolicy::None,
            ..Default::default()
        };

        let consumer = stream
            .create_consumer(consumer_config)
            .await
            .map_err(|e| format!("Failed to create consumer: {}", e))?;

        // Fetch all messages for this run
        let mut messages = consumer.messages().await
            .map_err(|e| format!("Failed to get messages: {}", e))?;

        let mut state: Option<RunState> = None;

        // Process messages with a timeout
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(msg_result) = messages.next().await {
                match msg_result {
                    Ok(msg) => {
                        let subject = msg.subject.as_str();
                        let seq = msg.info().map(|i| i.stream_sequence).unwrap_or(0);

                        // Parse the event
                        if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                            // Extract event type from subject or payload
                            let event_type = extract_event_type(subject);

                            // Initialize state on first workflow.started event
                            if event_type == "workflow.started" {
                                if let Ok(payload) = serde_json::from_value::<WorkflowStartedPayload>(event.clone()) {
                                    state = Some(RunState::new(
                                        run_id,
                                        payload.workflow_ref.workflow_id,
                                        payload.workflow_ref.workflow_name,
                                        payload.initial_marking,
                                    ));
                                }
                            } else if let Some(ref mut s) = state {
                                // Apply event to existing state
                                if let Some(payload) = event.get("payload") {
                                    s.apply_event(&event_type, payload, seq);
                                } else {
                                    s.apply_event(&event_type, &event, seq);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Error reading message");
                        break;
                    }
                }
            }
        });

        let _ = timeout.await;

        // Clean up the ephemeral consumer
        // (In production, we'd use a named consumer with better lifecycle management)

        Ok(state)
    }

    /// Publish an event to NATS.
    pub async fn publish_event(
        &self,
        subject: &str,
        event: &impl Serialize,
    ) -> Result<(), String> {
        self.nats
            .publish_jetstream(subject, event)
            .await
            .map_err(|e| format!("Failed to publish event: {}", e))?;
        Ok(())
    }
}

/// Extract event type from NATS subject.
/// Subject format: cb.runs.{run_id}.{event_category}.{event_type}
/// Example: cb.runs.xxx.tokens.produced -> token.produced
fn extract_event_type(subject: &str) -> String {
    let parts: Vec<&str> = subject.split('.').collect();
    if parts.len() >= 5 {
        // cb.runs.{run_id}.transitions.{tid}.completed -> transition.completed
        // cb.runs.{run_id}.tokens.{place}.produced -> token.produced
        let category = parts.get(3).unwrap_or(&"");
        let event = parts.last().unwrap_or(&"");

        match *category {
            "transitions" => format!("transition.{}", event),
            "tokens" => format!("token.{}", event),
            "status" => "workflow.status".to_string(),
            _ => format!("{}.{}", category, event),
        }
    } else if parts.len() >= 4 {
        // cb.runs.{run_id}.completed -> workflow.completed
        let event = parts.last().unwrap_or(&"");
        format!("workflow.{}", event)
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_state_new() {
        let initial = HashMap::from([("start".to_string(), 1u32)]);
        let state = RunState::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "test-workflow".to_string(),
            initial,
        );

        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.marking.get("start"), Some(&1));
        assert_eq!(state.tokens.get("start").map(|v| v.len()), Some(1));
    }

    #[test]
    fn test_add_remove_token() {
        let mut state = RunState::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "test".to_string(),
            HashMap::new(),
        );

        let token = Token {
            token_id: Uuid::new_v4(),
            place_id: "place-1".to_string(),
            data: Some(serde_json::json!({"key": "value"})),
            created_at: Utc::now(),
        };
        let token_id = token.token_id;

        state.add_token(token);
        assert_eq!(state.marking.get("place-1"), Some(&1));

        state.remove_token("place-1", &token_id);
        assert_eq!(state.marking.get("place-1"), Some(&0));
    }

    #[test]
    fn test_get_place_data() {
        let mut state = RunState::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "test".to_string(),
            HashMap::new(),
        );

        state.add_token(Token {
            token_id: Uuid::new_v4(),
            place_id: "place-1".to_string(),
            data: Some(serde_json::json!({"score": 95})),
            created_at: Utc::now(),
        });

        let data = state.get_place_data("place-1");
        assert!(data.is_some());
        assert_eq!(data.unwrap().get("score").and_then(|v| v.as_i64()), Some(95));
    }

    #[test]
    fn test_extract_event_type() {
        assert_eq!(
            extract_event_type("cb.runs.xxx.transitions.tid.completed"),
            "transition.completed"
        );
        assert_eq!(
            extract_event_type("cb.runs.xxx.tokens.place.produced"),
            "token.produced"
        );
        assert_eq!(
            extract_event_type("cb.runs.xxx.completed"),
            "workflow.completed"
        );
    }
}
