//! # Circuit Breaker Core
//!
//! Core types and definitions for the Circuit Breaker workflow engine.
//!
//! This crate provides:
//! - Petri net primitives (places, transitions, arcs, tokens)
//! - Workflow definition types (matching the JSON Schema contract)
//! - Event types for the NATS event-driven architecture
//! - Common error types and utilities
//!
//! ## Architecture
//!
//! The core crate is designed to be a shared dependency across all other
//! Circuit Breaker crates, providing the foundational types that ensure
//! consistency between the TypeScript SDK and Rust engine.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod engine;
pub mod error;
pub mod events;
pub mod petri;
pub mod workflow;

// Re-exports for convenience
pub use engine::{
    AutoModeConfig, CloudEngineConfig, ContainerRuntime, EngineConfig, EngineMode,
    EngineRequirements, LocalEngineConfig,
};
pub use error::{Error, Result};
pub use events::{Event, EventMetadata, EventType};
pub use petri::{Arc, Marking, Place, Token, Transition};
pub use workflow::{Action, DaggerAction, PolicyGate, Resources, Workflow};

/// Schema version for workflow definitions.
pub const SCHEMA_VERSION: &str = "1.0";

/// Default namespace for workflows.
pub const DEFAULT_NAMESPACE: &str = "default";

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::engine::{EngineConfig, EngineMode, EngineRequirements};
    pub use crate::error::{Error, Result};
    pub use crate::events::{Event, EventMetadata, EventType};
    pub use crate::petri::{Arc, Marking, Place, Token, Transition};
    pub use crate::workflow::{Action, Workflow};
    pub use crate::{DEFAULT_NAMESPACE, SCHEMA_VERSION};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_version() {
        assert_eq!(SCHEMA_VERSION, "1.0");
    }

    #[test]
    fn test_default_namespace() {
        assert_eq!(DEFAULT_NAMESPACE, "default");
    }
}
