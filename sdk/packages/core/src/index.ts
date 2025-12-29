/**
 * @circuit-breaker/core
 *
 * Core TypeScript SDK for Circuit Breaker workflow orchestration.
 * Provides type-safe workflow definitions using Petri-net semantics.
 *
 * @packageDocumentation
 */

// Schema exports
export {
  WorkflowSchema,
  PlaceSchema,
  TransitionSchema,
  ArcSchema,
  ResourcesSchema,
  DaggerActionSchema,
  HttpActionSchema,
  ScriptActionSchema,
  NoopActionSchema,
  ActionSchema,
  MetadataSchema,
} from "./schema";

// Type exports
export type {
  Workflow,
  Place,
  Transition,
  Arc,
  Resources,
  DaggerAction,
  HttpAction,
  ScriptAction,
  NoopAction,
  Action,
  Metadata,
  TokenSchema,
} from "./schema";

// Builder exports
export {
  workflow,
  WorkflowBuilder,
  TransitionBuilder,
  type TaskAction,
} from "./workflow";

// OpenCode AI agent integration
export {
  opencode,
  OpenCodeTaskBuilder,
  OpenCodeTasks,
  opencodeToDagger,
  OPENCODE_IMAGE,
  OPENCODE_DOCKERFILE,
  type OpenCodeConfig,
  type OpenCodeResult,
  type OpenCodeProvider,
  type OpenCodeModel,
  type OpenCodeAgent,
} from "./opencode";

// Client exports
export { CircuitBreakerClient, type ClientOptions } from "./client";

// Event types
export type {
  WorkflowEvent,
  WorkflowSubmittedEvent,
  WorkflowStartedEvent,
  WorkflowCompletedEvent,
  WorkflowFailedEvent,
  TransitionEvent,
  TransitionFiredEvent,
  TransitionCompletedEvent,
  TransitionFailedEvent,
  TokenEvent,
  TaskEvent,
} from "./events";

// Utility exports
export { validateWorkflow, ValidationError } from "./validate";
export {
  visualize,
  getGraphvizUrl,
  getMermaidUrl,
  type VisualizationOptions,
} from "./visualize";

// Re-export zod for convenience
export { z } from "zod";
