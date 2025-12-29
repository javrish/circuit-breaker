//! Workflow types matching the JSON Schema contract.
//!
//! These types are the Rust representation of the workflow definitions
//! that are submitted via the TypeScript SDK. They must match the
//! JSON Schema in `/schemas/workflow.schema.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schema version for workflow definitions.
pub const WORKFLOW_VERSION: &str = "1.0";

/// A complete workflow definition (Petri net).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    /// Schema version (must be "1.0").
    pub version: String,

    /// Workflow name (DNS-compatible: lowercase, hyphens, 2-63 chars).
    pub name: String,

    /// Namespace for multi-tenancy.
    #[serde(default = "default_namespace")]
    pub namespace: String,

    /// Optional metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Places (states) in the Petri net.
    pub places: Vec<Place>,

    /// Transitions (actions) in the Petri net.
    pub transitions: Vec<Transition>,
}

fn default_namespace() -> String {
    "default".to_string()
}

/// Workflow metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Key-value labels for filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Arbitrary metadata annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
}

/// A place (state) in the Petri net.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Unique identifier for the place.
    pub id: String,

    /// Initial token count (marking).
    #[serde(default)]
    pub initial_tokens: u32,

    /// Maximum token capacity (None = unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u32>,

    /// JSON Schema for typed tokens (colored Petri nets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_schema: Option<serde_json::Value>,
}

/// A transition (action) in the Petri net.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    /// Unique identifier for the transition.
    pub id: String,

    /// Input arcs from places.
    pub inputs: Vec<Arc>,

    /// Output arcs to places.
    pub outputs: Vec<Arc>,

    /// CEL expression guard condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,

    /// Action to execute when transition fires.
    pub action: Action,

    /// Resource requirements for execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Resources>,

    /// Execution timeout (e.g., "5m", "1h").
    #[serde(default = "default_timeout")]
    pub timeout: String,

    /// Number of retry attempts on failure.
    #[serde(default)]
    pub retries: u8,

    /// Retry backoff strategy.
    #[serde(default)]
    pub retry_backoff: RetryBackoff,

    /// Priority for scheduling (0-100, higher = more priority).
    #[serde(default = "default_priority")]
    pub priority: u8,
}

fn default_timeout() -> String {
    "5m".to_string()
}

fn default_priority() -> u8 {
    50
}

/// An arc connecting a place and transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Arc {
    /// Reference to a place ID.
    pub place: String,

    /// Number of tokens consumed/produced.
    #[serde(default = "default_weight")]
    pub weight: u32,

    /// CEL expression for token selection (colored nets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

fn default_weight() -> u32 {
    1
}

/// Retry backoff strategy.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RetryBackoff {
    /// Fixed delay between retries.
    Fixed,
    /// Exponential backoff.
    #[default]
    Exponential,
}

/// Action to execute when a transition fires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Action {
    /// Execute a Dagger pipeline.
    Dagger(DaggerAction),
    /// Make an HTTP request.
    Http(HttpAction),
    /// Run an inline script.
    Script(ScriptAction),
    /// No-op (for synchronization points).
    Noop,
}

/// Dagger pipeline action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaggerAction {
    /// Path to Dagger module.
    pub module: String,

    /// Function to call in the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,

    /// Arguments to pass to the function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<HashMap<String, serde_json::Value>>,

    /// Override container image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Enable Dagger caching.
    #[serde(default = "default_cache")]
    pub cache: bool,
}

fn default_cache() -> bool {
    true
}

/// HTTP request action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAction {
    /// HTTP endpoint URL.
    pub url: String,

    /// HTTP method.
    #[serde(default)]
    pub method: HttpMethod,

    /// HTTP headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    /// Request body template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Expected HTTP status codes for success.
    #[serde(default = "default_expected_status")]
    pub expected_status: Vec<u16>,
}

fn default_expected_status() -> Vec<u16> {
    vec![200, 201, 202, 204]
}

/// HTTP methods.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    #[default]
    Post,
    Put,
    Patch,
    Delete,
}

/// Script execution action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAction {
    /// JavaScript runtime.
    #[serde(default)]
    pub runtime: ScriptRuntime,

    /// Inline script code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Path to script file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Supported script runtimes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScriptRuntime {
    #[default]
    Bun,
    Deno,
    Node,
}

/// Resource requirements for transition execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Resources {
    /// CPU request (e.g., "100m", "2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,

    /// Memory request (e.g., "256Mi", "4Gi").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,

    /// GPU request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,

    /// Ephemeral storage request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_storage: Option<String>,

    /// Kubernetes node selector labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_selector: Option<HashMap<String, String>>,

    /// Kubernetes tolerations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerations: Option<Vec<Toleration>>,
}

/// Kubernetes toleration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Toleration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<TolerationOperator>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<TolerationEffect>,
}

/// Toleration operator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TolerationOperator {
    Exists,
    Equal,
}

/// Toleration effect.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TolerationEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

impl Workflow {
    /// Create a new workflow with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: WORKFLOW_VERSION.to_string(),
            name: name.into(),
            namespace: default_namespace(),
            metadata: None,
            places: Vec::new(),
            transitions: Vec::new(),
        }
    }

    /// Get the initial marking (map of place IDs to token counts).
    #[must_use]
    pub fn initial_marking(&self) -> HashMap<String, u32> {
        self.places
            .iter()
            .filter(|p| p.initial_tokens > 0)
            .map(|p| (p.id.clone(), p.initial_tokens))
            .collect()
    }

    /// Get a place by ID.
    #[must_use]
    pub fn get_place(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }

    /// Get a transition by ID.
    #[must_use]
    pub fn get_transition(&self, id: &str) -> Option<&Transition> {
        self.transitions.iter().find(|t| t.id == id)
    }

    /// Get all place IDs.
    #[must_use]
    pub fn place_ids(&self) -> Vec<&str> {
        self.places.iter().map(|p| p.id.as_str()).collect()
    }

    /// Get all transition IDs.
    #[must_use]
    pub fn transition_ids(&self) -> Vec<&str> {
        self.transitions.iter().map(|t| t.id.as_str()).collect()
    }

    /// Get transitions that consume from a given place.
    #[must_use]
    pub fn transitions_from(&self, place_id: &str) -> Vec<&Transition> {
        self.transitions
            .iter()
            .filter(|t| t.inputs.iter().any(|arc| arc.place == place_id))
            .collect()
    }

    /// Get transitions that produce to a given place.
    #[must_use]
    pub fn transitions_to(&self, place_id: &str) -> Vec<&Transition> {
        self.transitions
            .iter()
            .filter(|t| t.outputs.iter().any(|arc| arc.place == place_id))
            .collect()
    }
}

impl Transition {
    /// Check if this transition has a guard condition.
    #[must_use]
    pub fn has_guard(&self) -> bool {
        self.guard.is_some()
    }

    /// Check if this is a noop transition.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        matches!(self.action, Action::Noop)
    }

    /// Get the set of input place IDs.
    #[must_use]
    pub fn input_places(&self) -> Vec<&str> {
        self.inputs.iter().map(|arc| arc.place.as_str()).collect()
    }

    /// Get the set of output place IDs.
    #[must_use]
    pub fn output_places(&self) -> Vec<&str> {
        self.outputs.iter().map(|arc| arc.place.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_serialization() {
        let workflow = Workflow {
            version: "1.0".to_string(),
            name: "test-workflow".to_string(),
            namespace: "default".to_string(),
            metadata: Some(Metadata {
                description: Some("Test workflow".to_string()),
                labels: Some(HashMap::from([("env".to_string(), "test".to_string())])),
                annotations: None,
            }),
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
                inputs: vec![Arc {
                    place: "start".to_string(),
                    weight: 1,
                    expression: None,
                }],
                outputs: vec![Arc {
                    place: "end".to_string(),
                    weight: 1,
                    expression: None,
                }],
                guard: None,
                action: Action::Noop,
                resources: None,
                timeout: "5m".to_string(),
                retries: 0,
                retry_backoff: RetryBackoff::Exponential,
                priority: 50,
            }],
        };

        let json = serde_json::to_string_pretty(&workflow).unwrap();
        let parsed: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(workflow, parsed);
    }

    #[test]
    fn test_dagger_action() {
        let action = Action::Dagger(DaggerAction {
            module: "./ci".to_string(),
            function: Some("build".to_string()),
            args: Some(HashMap::from([(
                "target".to_string(),
                serde_json::json!("release"),
            )])),
            image: None,
            cache: true,
        });

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"dagger\""));
    }

    #[test]
    fn test_initial_marking() {
        let workflow = Workflow {
            version: "1.0".to_string(),
            name: "test".to_string(),
            namespace: "default".to_string(),
            metadata: None,
            places: vec![
                Place {
                    id: "a".to_string(),
                    initial_tokens: 2,
                    capacity: None,
                    token_schema: None,
                },
                Place {
                    id: "b".to_string(),
                    initial_tokens: 0,
                    capacity: None,
                    token_schema: None,
                },
                Place {
                    id: "c".to_string(),
                    initial_tokens: 1,
                    capacity: None,
                    token_schema: None,
                },
            ],
            transitions: vec![],
        };

        let marking = workflow.initial_marking();
        assert_eq!(marking.get("a"), Some(&2));
        assert_eq!(marking.get("b"), None);
        assert_eq!(marking.get("c"), Some(&1));
    }

    #[test]
    fn test_parse_hello_world_json() {
        let json = r#"{
  "version": "1.0",
  "name": "hello-world",
  "namespace": "examples",
  "metadata": {
    "description": "A simple hello world workflow",
    "labels": {
      "example": "true"
    }
  },
  "places": [
    { "id": "start", "initialTokens": 1, "capacity": null },
    { "id": "hello-done", "initialTokens": 0, "capacity": null },
    { "id": "complete", "initialTokens": 0, "capacity": null }
  ],
  "transitions": [
    {
      "id": "say-hello",
      "inputs": [{ "place": "start", "weight": 1 }],
      "outputs": [{ "place": "hello-done", "weight": 1 }],
      "action": { "type": "script", "runtime": "bun", "code": "console.log('Hello');" },
      "timeout": "1m",
      "retries": 0,
      "retryBackoff": "exponential",
      "priority": 50
    },
    {
      "id": "say-world",
      "inputs": [{ "place": "hello-done", "weight": 1 }],
      "outputs": [{ "place": "complete", "weight": 1 }],
      "action": { "type": "script", "runtime": "bun", "code": "console.log('World!');" },
      "timeout": "1m",
      "retries": 0,
      "retryBackoff": "exponential",
      "priority": 50
    }
  ]
}"#;

        let workflow: Workflow =
            serde_json::from_str(json).expect("Failed to parse hello-world JSON");

        assert_eq!(workflow.name, "hello-world");
        assert_eq!(workflow.namespace, "examples");
        assert_eq!(workflow.places.len(), 3);
        assert_eq!(workflow.transitions.len(), 2);

        // Check initial marking
        let marking = workflow.initial_marking();
        assert_eq!(marking.get("start"), Some(&1));
        assert!(marking.get("hello-done").is_none());
        assert!(marking.get("complete").is_none());

        // Check transitions
        assert_eq!(workflow.transitions[0].id, "say-hello");
        assert_eq!(workflow.transitions[1].id, "say-world");

        // Check action types
        assert!(matches!(workflow.transitions[0].action, Action::Script(_)));
        assert!(matches!(workflow.transitions[1].action, Action::Script(_)));

        // Verify it can be serialized back
        let serialized = serde_json::to_string(&workflow).unwrap();
        let reparsed: Workflow = serde_json::from_str(&serialized).unwrap();
        assert_eq!(workflow, reparsed);
    }
}
