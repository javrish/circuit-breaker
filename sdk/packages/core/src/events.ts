/**
 * Event types for Circuit Breaker NATS event-driven architecture.
 * These types match the JSON Schema contract in /schemas/events.schema.json
 *
 * @module
 */

// ============ Common Types ============

/**
 * Metadata attached to every event.
 */
export interface EventMetadata {
  /** Unique identifier for this event */
  eventId: string;
  /** ISO 8601 timestamp when event was created */
  timestamp: string;
  /** Event schema version */
  version: '1.0';
  /** ID to correlate related events */
  correlationId?: string;
  /** ID of the event that caused this event */
  causationId?: string;
  /** Service that emitted this event */
  source?: string;
}

/**
 * Reference to a workflow definition.
 */
export interface WorkflowRef {
  workflowId: string;
  workflowName: string;
  namespace?: string;
}

/**
 * Reference to a workflow run (execution instance).
 */
export interface RunRef {
  runId: string;
  workflowId: string;
  attempt?: number;
}

/**
 * Reference to a transition execution.
 */
export interface TransitionRef {
  transitionId: string;
  runId: string;
  /** Unique ID for this specific execution of the transition */
  executionId?: string;
}

/**
 * Token data for colored Petri nets.
 */
export interface TokenData {
  tokenId: string;
  placeId: string;
  /** Arbitrary token payload */
  data?: Record<string, unknown>;
  createdAt: string;
}

/**
 * Error information.
 */
export interface ErrorInfo {
  /** Error code for programmatic handling */
  code: string;
  /** Human-readable error message */
  message: string;
  /** Additional error context */
  details?: Record<string, unknown>;
  /** Stack trace if available */
  stackTrace?: string;
  /** Whether this error is retryable */
  retryable?: boolean;
}

/**
 * Resource usage metrics.
 */
export interface ResourceUsage {
  /** CPU time in milliseconds */
  cpuMillis?: number;
  /** Peak memory usage in bytes */
  memoryBytes?: number;
  /** Wall clock duration in milliseconds */
  durationMs?: number;
}

// ============ Event Type Constants ============

export const EventTypes = {
  // Workflow lifecycle
  WORKFLOW_SUBMITTED: 'workflow.submitted',
  WORKFLOW_STARTED: 'workflow.started',
  WORKFLOW_COMPLETED: 'workflow.completed',
  WORKFLOW_FAILED: 'workflow.failed',
  WORKFLOW_CANCELLED: 'workflow.cancelled',

  // Transition lifecycle
  TRANSITION_ENABLED: 'transition.enabled',
  TRANSITION_FIRED: 'transition.fired',
  TRANSITION_COMPLETED: 'transition.completed',
  TRANSITION_FAILED: 'transition.failed',
  TRANSITION_RETRYING: 'transition.retrying',

  // Token events
  TOKEN_PRODUCED: 'token.produced',
  TOKEN_CONSUMED: 'token.consumed',

  // Task events
  TASK_DISPATCHED: 'task.dispatched',
  TASK_COMPLETED: 'task.completed',
  TASK_FAILED: 'task.failed',
} as const;

export type EventType = (typeof EventTypes)[keyof typeof EventTypes];

// ============ Base Event Interface ============

interface BaseEvent<T extends string, P> {
  type: T;
  metadata: EventMetadata;
  payload: P;
}

// ============ Workflow Events ============

export interface WorkflowSubmittedPayload {
  workflow: Record<string, unknown>; // Full workflow definition
  submittedBy?: string;
  labels?: Record<string, string>;
}

export interface WorkflowStartedPayload {
  workflowRef: WorkflowRef;
  runRef: RunRef;
  /** Map of place IDs to token counts */
  initialMarking: Record<string, number>;
  /** Input parameters for this run */
  inputs?: Record<string, unknown>;
  triggeredBy?: 'manual' | 'schedule' | 'webhook' | 'event' | 'api';
}

export interface WorkflowCompletedPayload {
  runRef: RunRef;
  finalMarking: Record<string, number>;
  outputs?: Record<string, unknown>;
  resourceUsage?: ResourceUsage;
}

export interface WorkflowFailedPayload {
  runRef: RunRef;
  error: ErrorInfo;
  failedTransition?: string;
  marking?: Record<string, number>;
}

export interface WorkflowCancelledPayload {
  runRef: RunRef;
  reason?: string;
  cancelledBy?: string;
  marking?: Record<string, number>;
}

export type WorkflowSubmittedEvent = BaseEvent<'workflow.submitted', WorkflowSubmittedPayload>;
export type WorkflowStartedEvent = BaseEvent<'workflow.started', WorkflowStartedPayload>;
export type WorkflowCompletedEvent = BaseEvent<'workflow.completed', WorkflowCompletedPayload>;
export type WorkflowFailedEvent = BaseEvent<'workflow.failed', WorkflowFailedPayload>;
export type WorkflowCancelledEvent = BaseEvent<'workflow.cancelled', WorkflowCancelledPayload>;

export type WorkflowEvent =
  | WorkflowSubmittedEvent
  | WorkflowStartedEvent
  | WorkflowCompletedEvent
  | WorkflowFailedEvent
  | WorkflowCancelledEvent;

// ============ Transition Events ============

export interface TransitionEnabledPayload {
  transitionRef: TransitionRef;
  inputTokens: TokenData[];
  /** Result of guard expression evaluation */
  guardResult?: boolean;
}

export interface TransitionFiredPayload {
  transitionRef: TransitionRef;
  consumedTokens: TokenData[];
  /** ID of the dispatched task */
  taskId?: string;
  /** ID of the runner assigned to this task */
  runnerId?: string;
}

export interface TransitionCompletedPayload {
  transitionRef: TransitionRef;
  producedTokens: TokenData[];
  outputs?: Record<string, unknown>;
  resourceUsage?: ResourceUsage;
}

export interface TransitionFailedPayload {
  transitionRef: TransitionRef;
  error: ErrorInfo;
  attempt?: number;
  maxRetries?: number;
  willRetry?: boolean;
}

export interface TransitionRetryingPayload {
  transitionRef: TransitionRef;
  attempt: number;
  maxRetries?: number;
  nextAttemptAt: string;
  backoffMs?: number;
  lastError?: ErrorInfo;
}

export type TransitionEnabledEvent = BaseEvent<'transition.enabled', TransitionEnabledPayload>;
export type TransitionFiredEvent = BaseEvent<'transition.fired', TransitionFiredPayload>;
export type TransitionCompletedEvent = BaseEvent<'transition.completed', TransitionCompletedPayload>;
export type TransitionFailedEvent = BaseEvent<'transition.failed', TransitionFailedPayload>;
export type TransitionRetryingEvent = BaseEvent<'transition.retrying', TransitionRetryingPayload>;

export type TransitionEvent =
  | TransitionEnabledEvent
  | TransitionFiredEvent
  | TransitionCompletedEvent
  | TransitionFailedEvent
  | TransitionRetryingEvent;

// ============ Token Events ============

export interface TokenProducedPayload {
  runRef: RunRef;
  token: TokenData;
  /** Transition ID that produced this token, or 'initial' for initial marking */
  producedBy: string;
}

export interface TokenConsumedPayload {
  runRef: RunRef;
  token: TokenData;
  /** Transition ID that consumed this token */
  consumedBy: string;
}

export type TokenProducedEvent = BaseEvent<'token.produced', TokenProducedPayload>;
export type TokenConsumedEvent = BaseEvent<'token.consumed', TokenConsumedPayload>;

export type TokenEvent = TokenProducedEvent | TokenConsumedEvent;

// ============ Task Events ============

export interface TaskDispatchedPayload {
  taskId: string;
  transitionRef: TransitionRef;
  action: Record<string, unknown>;
  resources?: {
    cpu?: string;
    memory?: string;
    gpu?: string;
  };
  timeout?: string;
  runnerPool?: string;
}

export interface TaskCompletedPayload {
  taskId: string;
  transitionRef: TransitionRef;
  runnerId?: string;
  outputs?: Record<string, unknown>;
  artifacts?: Array<{
    name: string;
    path: string;
    size?: number;
    checksum?: string;
  }>;
  resourceUsage?: ResourceUsage;
}

export interface TaskFailedPayload {
  taskId: string;
  transitionRef: TransitionRef;
  runnerId?: string;
  error: ErrorInfo;
  logs?: string;
}

export type TaskDispatchedEvent = BaseEvent<'task.dispatched', TaskDispatchedPayload>;
export type TaskCompletedEvent = BaseEvent<'task.completed', TaskCompletedPayload>;
export type TaskFailedEvent = BaseEvent<'task.failed', TaskFailedPayload>;

export type TaskEvent = TaskDispatchedEvent | TaskCompletedEvent | TaskFailedEvent;

// ============ Union Type for All Events ============

export type CircuitBreakerEvent =
  | WorkflowEvent
  | TransitionEvent
  | TokenEvent
  | TaskEvent;

// ============ Event Factory Functions ============

/**
 * Create event metadata with defaults.
 */
export function createEventMetadata(
  overrides: Partial<EventMetadata> = {}
): EventMetadata {
  return {
    eventId: crypto.randomUUID(),
    timestamp: new Date().toISOString(),
    version: '1.0',
    ...overrides,
  };
}

/**
 * Create a workflow submitted event.
 */
export function workflowSubmitted(
  payload: WorkflowSubmittedPayload,
  metadata?: Partial<EventMetadata>
): WorkflowSubmittedEvent {
  return {
    type: 'workflow.submitted',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

/**
 * Create a workflow started event.
 */
export function workflowStarted(
  payload: WorkflowStartedPayload,
  metadata?: Partial<EventMetadata>
): WorkflowStartedEvent {
  return {
    type: 'workflow.started',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

/**
 * Create a workflow completed event.
 */
export function workflowCompleted(
  payload: WorkflowCompletedPayload,
  metadata?: Partial<EventMetadata>
): WorkflowCompletedEvent {
  return {
    type: 'workflow.completed',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

/**
 * Create a workflow failed event.
 */
export function workflowFailed(
  payload: WorkflowFailedPayload,
  metadata?: Partial<EventMetadata>
): WorkflowFailedEvent {
  return {
    type: 'workflow.failed',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

/**
 * Create a transition fired event.
 */
export function transitionFired(
  payload: TransitionFiredPayload,
  metadata?: Partial<EventMetadata>
): TransitionFiredEvent {
  return {
    type: 'transition.fired',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

/**
 * Create a transition completed event.
 */
export function transitionCompleted(
  payload: TransitionCompletedPayload,
  metadata?: Partial<EventMetadata>
): TransitionCompletedEvent {
  return {
    type: 'transition.completed',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

/**
 * Create a task dispatched event.
 */
export function taskDispatched(
  payload: TaskDispatchedPayload,
  metadata?: Partial<EventMetadata>
): TaskDispatchedEvent {
  return {
    type: 'task.dispatched',
    metadata: createEventMetadata(metadata),
    payload,
  };
}

// ============ Type Guards ============

export function isWorkflowEvent(event: CircuitBreakerEvent): event is WorkflowEvent {
  return event.type.startsWith('workflow.');
}

export function isTransitionEvent(event: CircuitBreakerEvent): event is TransitionEvent {
  return event.type.startsWith('transition.');
}

export function isTokenEvent(event: CircuitBreakerEvent): event is TokenEvent {
  return event.type.startsWith('token.');
}

export function isTaskEvent(event: CircuitBreakerEvent): event is TaskEvent {
  return event.type.startsWith('task.');
}

// ============ NATS Subject Helpers ============

/**
 * NATS subject patterns for subscribing to events.
 */
export const NatsSubjects = {
  /** All workflow events */
  workflows: 'workflow.>',
  /** Specific workflow events */
  workflow: (workflowId: string) => `workflow.${workflowId}.>`,
  /** All run events */
  runs: 'run.>',
  /** Specific run events */
  run: (runId: string) => `run.${runId}.>`,
  /** All task dispatch events (for runners) */
  taskDispatch: 'task.dispatch.>',
  /** Task dispatch for specific runner pool */
  taskDispatchPool: (pool: string) => `task.dispatch.${pool}`,
  /** Task results */
  taskResults: 'task.result.>',
} as const;
