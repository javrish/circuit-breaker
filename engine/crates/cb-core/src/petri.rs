//! Petri net execution primitives.
//!
//! This module provides the core types and algorithms for executing Petri nets:
//! - `Place`, `Transition`, `Arc` - structural components
//! - `Token` - data flowing through the net
//! - `Marking` - current state (token distribution)
//! - Firing rules and enabled transition detection

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::workflow;

/// A token in the Petri net.
///
/// Tokens represent work items or data flowing through the workflow.
/// In colored Petri nets, tokens can carry arbitrary data payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    /// Unique identifier for this token.
    pub id: Uuid,

    /// The place where this token currently resides.
    pub place_id: String,

    /// Optional data payload (for colored Petri nets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,

    /// Timestamp when this token was created.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// ID of the transition that produced this token (or "initial" for initial marking).
    pub produced_by: String,
}

impl Token {
    /// Create a new token in the specified place.
    #[must_use]
    pub fn new(place_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            place_id: place_id.into(),
            data: None,
            created_at: chrono::Utc::now(),
            produced_by: "initial".to_string(),
        }
    }

    /// Create a new token with data payload.
    #[must_use]
    pub fn with_data(place_id: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            place_id: place_id.into(),
            data: Some(data),
            created_at: chrono::Utc::now(),
            produced_by: "initial".to_string(),
        }
    }

    /// Set the producer of this token.
    #[must_use]
    pub fn produced_by(mut self, transition_id: impl Into<String>) -> Self {
        self.produced_by = transition_id.into();
        self
    }
}

/// The marking (state) of a Petri net.
///
/// A marking represents the current distribution of tokens across places.
/// This is the "state" of a workflow execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Marking {
    /// Map of place ID to tokens in that place.
    places: HashMap<String, Vec<Token>>,
}

impl Marking {
    /// Create a new empty marking.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a marking from a workflow's initial marking.
    #[must_use]
    pub fn from_workflow(workflow: &workflow::Workflow) -> Self {
        let mut marking = Self::new();

        for place in &workflow.places {
            if place.initial_tokens > 0 {
                for _ in 0..place.initial_tokens {
                    marking.add_token(Token::new(&place.id));
                }
            }
        }

        marking
    }

    /// Add a token to the marking.
    pub fn add_token(&mut self, token: Token) {
        self.places
            .entry(token.place_id.clone())
            .or_default()
            .push(token);
    }

    /// Remove a token from a place by ID.
    ///
    /// Returns the removed token if found.
    pub fn remove_token(&mut self, place_id: &str, token_id: Uuid) -> Option<Token> {
        if let Some(tokens) = self.places.get_mut(place_id) {
            if let Some(pos) = tokens.iter().position(|t| t.id == token_id) {
                return Some(tokens.remove(pos));
            }
        }
        None
    }

    /// Remove one token from a place (FIFO order).
    ///
    /// Returns the removed token if the place had any tokens.
    pub fn remove_one(&mut self, place_id: &str) -> Option<Token> {
        if let Some(tokens) = self.places.get_mut(place_id) {
            if !tokens.is_empty() {
                return Some(tokens.remove(0));
            }
        }
        None
    }

    /// Remove multiple tokens from a place.
    ///
    /// Returns the removed tokens (may be fewer than requested if not enough tokens).
    pub fn remove_many(&mut self, place_id: &str, count: usize) -> Vec<Token> {
        if let Some(tokens) = self.places.get_mut(place_id) {
            let to_remove = count.min(tokens.len());
            tokens.drain(0..to_remove).collect()
        } else {
            Vec::new()
        }
    }

    /// Get the number of tokens in a place.
    #[must_use]
    pub fn token_count(&self, place_id: &str) -> usize {
        self.places.get(place_id).map_or(0, Vec::len)
    }

    /// Check if a place has at least the specified number of tokens.
    #[must_use]
    pub fn has_tokens(&self, place_id: &str, count: usize) -> bool {
        self.token_count(place_id) >= count
    }

    /// Get all tokens in a place.
    #[must_use]
    pub fn tokens_in(&self, place_id: &str) -> &[Token] {
        self.places.get(place_id).map_or(&[], Vec::as_slice)
    }

    /// Get a simple representation of the marking (place -> count).
    #[must_use]
    pub fn as_counts(&self) -> HashMap<String, usize> {
        self.places
            .iter()
            .map(|(k, v)| (k.clone(), v.len()))
            .collect()
    }

    /// Get all place IDs that have tokens.
    #[must_use]
    pub fn non_empty_places(&self) -> Vec<&str> {
        self.places
            .iter()
            .filter(|(_, tokens)| !tokens.is_empty())
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Check if the marking is empty (no tokens anywhere).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.places.values().all(Vec::is_empty)
    }

    /// Get the total number of tokens across all places.
    #[must_use]
    pub fn total_tokens(&self) -> usize {
        self.places.values().map(Vec::len).sum()
    }
}

/// Re-export workflow types as Petri net primitives.
pub type Place = workflow::Place;
pub type Transition = workflow::Transition;
pub type Arc = workflow::Arc;

/// Result of checking if a transition is enabled.
#[derive(Debug, Clone)]
pub struct EnabledTransition {
    /// The transition that is enabled.
    pub transition_id: String,

    /// Tokens that would be consumed if this transition fires.
    pub input_tokens: Vec<Token>,

    /// Whether the guard condition (if any) evaluated to true.
    pub guard_satisfied: bool,
}

/// Check if a transition is enabled given the current marking.
///
/// A transition is enabled if:
/// 1. All input places have at least the required number of tokens (arc weights)
/// 2. All output places have capacity for new tokens (if capacity is limited)
/// 3. The guard condition (if any) evaluates to true
///
/// Note: Guard evaluation requires a CEL interpreter and context,
/// so this function only checks structural enablement by default.
#[must_use]
pub fn is_enabled(
    transition: &Transition,
    marking: &Marking,
    workflow: &workflow::Workflow,
) -> bool {
    // Check input arcs - sufficient tokens in each input place
    for input_arc in &transition.inputs {
        if !marking.has_tokens(&input_arc.place, input_arc.weight as usize) {
            return false;
        }
    }

    // Check output arcs - capacity constraints
    for output_arc in &transition.outputs {
        if let Some(place) = workflow.get_place(&output_arc.place) {
            if let Some(capacity) = place.capacity {
                let current = marking.token_count(&output_arc.place);
                let after = current + output_arc.weight as usize;
                if after > capacity as usize {
                    return false;
                }
            }
        }
    }

    // Note: Guard condition is checked separately as it requires CEL evaluation
    true
}

/// Find all enabled transitions in a workflow given the current marking.
#[must_use]
pub fn find_enabled_transitions<'a>(
    workflow: &'a workflow::Workflow,
    marking: &Marking,
) -> Vec<&'a Transition> {
    workflow
        .transitions
        .iter()
        .filter(|t| is_enabled(t, marking, workflow))
        .collect()
}

/// Result of firing a transition.
#[derive(Debug, Clone)]
pub struct FiringResult {
    /// Tokens consumed from input places.
    pub consumed_tokens: Vec<Token>,

    /// Tokens produced to output places.
    pub produced_tokens: Vec<Token>,

    /// The new marking after firing.
    pub new_marking: Marking,
}

/// Fire a transition, consuming input tokens and producing output tokens.
///
/// This function assumes the transition is enabled. It:
/// 1. Removes tokens from input places according to arc weights
/// 2. Creates new tokens in output places according to arc weights
/// 3. Returns the consumed and produced tokens along with the new marking
///
/// # Panics
///
/// Panics if the transition is not enabled (not enough tokens in input places).
pub fn fire_transition(
    transition: &Transition,
    marking: &mut Marking,
    _workflow: &workflow::Workflow,
) -> FiringResult {
    let mut consumed_tokens = Vec::new();
    let mut produced_tokens = Vec::new();

    // Consume tokens from input places
    for input_arc in &transition.inputs {
        let tokens = marking.remove_many(&input_arc.place, input_arc.weight as usize);
        if tokens.len() < input_arc.weight as usize {
            panic!(
                "Transition '{}' is not enabled: insufficient tokens in place '{}'",
                transition.id, input_arc.place
            );
        }
        consumed_tokens.extend(tokens);
    }

    // Produce tokens to output places
    for output_arc in &transition.outputs {
        for _ in 0..output_arc.weight {
            let token = Token::new(&output_arc.place).produced_by(&transition.id);
            marking.add_token(token.clone());
            produced_tokens.push(token);
        }
    }

    FiringResult {
        consumed_tokens,
        produced_tokens,
        new_marking: marking.clone(),
    }
}

/// Compute the set of reachable places from places with initial tokens.
#[must_use]
pub fn reachable_places(workflow: &workflow::Workflow) -> HashSet<String> {
    let mut reachable = HashSet::new();
    let mut queue: Vec<String> = Vec::new();

    // Start from places with initial tokens
    for place in &workflow.places {
        if place.initial_tokens > 0 {
            reachable.insert(place.id.clone());
            queue.push(place.id.clone());
        }
    }

    // BFS to find reachable places
    while let Some(place_id) = queue.pop() {
        // Find transitions that consume from this place
        for transition in &workflow.transitions {
            if transition.inputs.iter().any(|arc| arc.place == place_id) {
                // All output places of this transition are reachable
                for output_arc in &transition.outputs {
                    if !reachable.contains(&output_arc.place) {
                        reachable.insert(output_arc.place.clone());
                        queue.push(output_arc.place.clone());
                    }
                }
            }
        }
    }

    reachable
}

/// Find terminal places (places with no outgoing transitions).
#[must_use]
pub fn terminal_places(workflow: &workflow::Workflow) -> Vec<&str> {
    let places_with_outgoing: HashSet<_> = workflow
        .transitions
        .iter()
        .flat_map(|t| t.inputs.iter().map(|arc| arc.place.as_str()))
        .collect();

    workflow
        .places
        .iter()
        .filter(|p| !places_with_outgoing.contains(p.id.as_str()))
        .map(|p| p.id.as_str())
        .collect()
}

/// Find source places (places with no incoming transitions).
#[must_use]
pub fn source_places(workflow: &workflow::Workflow) -> Vec<&str> {
    let places_with_incoming: HashSet<_> = workflow
        .transitions
        .iter()
        .flat_map(|t| t.outputs.iter().map(|arc| arc.place.as_str()))
        .collect();

    workflow
        .places
        .iter()
        .filter(|p| !places_with_incoming.contains(p.id.as_str()))
        .map(|p| p.id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{Action, Workflow};

    fn create_simple_workflow() -> Workflow {
        Workflow {
            version: "1.0".to_string(),
            name: "test".to_string(),
            namespace: "default".to_string(),
            metadata: None,
            places: vec![
                workflow::Place {
                    id: "p1".to_string(),
                    initial_tokens: 1,
                    capacity: None,
                    token_schema: None,
                    annotations: Default::default(),
                },
                workflow::Place {
                    id: "p2".to_string(),
                    initial_tokens: 0,
                    capacity: None,
                    token_schema: None,
                    annotations: Default::default(),
                },
            ],
            transitions: vec![workflow::Transition {
                id: "t1".to_string(),
                inputs: vec![workflow::Arc {
                    place: "p1".to_string(),
                    weight: 1,
                    expression: None,
                }],
                outputs: vec![workflow::Arc {
                    place: "p2".to_string(),
                    weight: 1,
                    expression: None,
                }],
                guard: None,
                action: Action::Noop,
                policy: None,
                resources: None,
                timeout: "5m".to_string(),
                retries: 0,
                retry_backoff: workflow::RetryBackoff::Exponential,
                priority: 50,
            }],
        }
    }

    #[test]
    fn test_marking_from_workflow() {
        let workflow = create_simple_workflow();
        let marking = Marking::from_workflow(&workflow);

        assert_eq!(marking.token_count("p1"), 1);
        assert_eq!(marking.token_count("p2"), 0);
    }

    #[test]
    fn test_token_operations() {
        let mut marking = Marking::new();

        let token = Token::new("p1");
        let token_id = token.id;
        marking.add_token(token);

        assert_eq!(marking.token_count("p1"), 1);
        assert!(marking.has_tokens("p1", 1));

        let removed = marking.remove_token("p1", token_id);
        assert!(removed.is_some());
        assert_eq!(marking.token_count("p1"), 0);
    }

    #[test]
    fn test_is_enabled() {
        let workflow = create_simple_workflow();
        let marking = Marking::from_workflow(&workflow);

        let transition = &workflow.transitions[0];
        assert!(is_enabled(transition, &marking, &workflow));

        // Empty marking should disable the transition
        let empty_marking = Marking::new();
        assert!(!is_enabled(transition, &empty_marking, &workflow));
    }

    #[test]
    fn test_fire_transition() {
        let workflow = create_simple_workflow();
        let mut marking = Marking::from_workflow(&workflow);

        let transition = &workflow.transitions[0];
        let result = fire_transition(transition, &mut marking, &workflow);

        assert_eq!(result.consumed_tokens.len(), 1);
        assert_eq!(result.produced_tokens.len(), 1);
        assert_eq!(marking.token_count("p1"), 0);
        assert_eq!(marking.token_count("p2"), 1);
    }

    #[test]
    fn test_find_enabled_transitions() {
        let workflow = create_simple_workflow();
        let marking = Marking::from_workflow(&workflow);

        let enabled = find_enabled_transitions(&workflow, &marking);
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "t1");
    }

    #[test]
    fn test_reachable_places() {
        let workflow = create_simple_workflow();
        let reachable = reachable_places(&workflow);

        assert!(reachable.contains("p1"));
        assert!(reachable.contains("p2"));
    }

    #[test]
    fn test_terminal_and_source_places() {
        let workflow = create_simple_workflow();

        let terminals = terminal_places(&workflow);
        assert_eq!(terminals, vec!["p2"]);

        let sources = source_places(&workflow);
        assert_eq!(sources, vec!["p1"]);
    }
}
