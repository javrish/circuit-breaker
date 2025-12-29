# Circuit Breaker: Event-Driven Architecture

This document describes the event-driven architecture of Circuit Breaker, detailing how workflows flow through the system from submission to completion.

## Overview

Circuit Breaker uses an **event-sourced architecture** where:
- All state changes are captured as immutable events
- Components communicate exclusively through events via NATS JetStream
- State is derived by replaying events
- Each component has a single responsibility

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Circuit Breaker Architecture                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   ┌─────────┐     ┌─────────────┐     ┌──────────────────────────────────┐  │
│   │   CLI   │────▶│  API Server │────▶│         NATS JetStream           │  │
│   │   SDK   │     │   (cb-api)  │     │     (Event Bus / Store)          │  │
│   └─────────┘     └─────────────┘     └──────────────────────────────────┘  │
│                                                    │                         │
│                          ┌─────────────────────────┼─────────────────────┐   │
│                          │                         │                     │   │
│                          ▼                         ▼                     ▼   │
│                   ┌─────────────┐          ┌─────────────┐        ┌─────────┐│
│                   │ Controller  │          │  Scheduler  │        │ Runner  ││
│                   │             │◀────────▶│             │◀──────▶│(Dagger) ││
│                   └─────────────┘          └─────────────┘        └─────────┘│
│                          │                         │                     │   │
│                          └─────────────────────────┼─────────────────────┘   │
│                                                    │                         │
│                                                    ▼                         │
│                                        ┌──────────────────────┐              │
│                                        │   Kubernetes Pods    │              │
│                                        │   (Task Execution)   │              │
│                                        └──────────────────────┘              │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Components

### 1. API Server (`cb-api`)
- **Role**: Entry point for workflow submissions and queries
- **Responsibilities**:
  - Validate workflow definitions against JSON Schema
  - Store workflow definitions
  - Publish `WorkflowSubmitted` events
  - Expose REST API for workflow/run management
  - WebSocket endpoint for real-time event streaming

### 2. Controller (`cb-controller`)
- **Role**: Orchestrates workflow execution lifecycle
- **Responsibilities**:
  - Initialize Petri net marking when workflow starts
  - Update marking as transitions complete
  - Detect workflow completion (final marking reached)
  - Handle failures and retries
  - Publish workflow lifecycle events

### 3. Scheduler (`cb-scheduler`)
- **Role**: Determines which transitions can fire
- **Responsibilities**:
  - Evaluate Petri net firing rules
  - Check guard conditions (CEL expressions)
  - Determine enabled transitions based on current marking
  - Respect resource constraints and priorities
  - Publish `TransitionEnabled` events

### 4. Runner (`cb-runner`)
- **Role**: Executes individual tasks
- **Responsibilities**:
  - Subscribe to `TransitionEnabled` events
  - Execute tasks via Dagger pipelines
  - Report task progress and results
  - Publish `TransitionCompleted`/`TransitionFailed` events

## Event Types

### Workflow Events

| Event | Description | Triggered By |
|-------|-------------|--------------|
| `WorkflowSubmitted` | New workflow definition submitted | API Server |
| `WorkflowStarted` | Workflow run initiated, initial marking set | Controller |
| `WorkflowCompleted` | Final marking reached, workflow successful | Controller |
| `WorkflowFailed` | Unrecoverable error, workflow terminated | Controller |
| `WorkflowCancelled` | User requested cancellation | Controller |

### Transition Events

| Event | Description | Triggered By |
|-------|-------------|--------------|
| `TransitionEnabled` | Transition has sufficient tokens and guard passes | Scheduler |
| `TransitionFired` | Transition execution started, input tokens consumed | Runner |
| `TransitionCompleted` | Task finished successfully, output tokens produced | Runner |
| `TransitionFailed` | Task failed (may retry) | Runner |

### Marking Events

| Event | Description | Triggered By |
|-------|-------------|--------------|
| `TokensConsumed` | Tokens removed from input places | Controller |
| `TokensProduced` | Tokens added to output places | Controller |
| `MarkingUpdated` | Current marking snapshot | Controller |

## Event Flow

### Happy Path: Successful Workflow Execution

```
┌──────┐    ┌─────────┐    ┌──────┐    ┌────────────┐    ┌───────────┐    ┌────────┐
│ User │    │   API   │    │ NATS │    │ Controller │    │ Scheduler │    │ Runner │
└──┬───┘    └────┬────┘    └──┬───┘    └─────┬──────┘    └─────┬─────┘    └───┬────┘
   │             │            │              │                 │              │
   │ POST /workflows          │              │                 │              │
   │────────────▶│            │              │                 │              │
   │             │            │              │                 │              │
   │             │ WorkflowSubmitted         │                 │              │
   │             │───────────▶│              │                 │              │
   │             │            │              │                 │              │
   │  201 Created│            │ WorkflowSubmitted              │              │
   │◀────────────│            │─────────────▶│                 │              │
   │             │            │              │                 │              │
   │             │            │              │ Initialize      │              │
   │             │            │              │ Marking         │              │
   │             │            │              │                 │              │
   │             │            │ WorkflowStarted                │              │
   │             │            │◀─────────────│                 │              │
   │             │            │              │                 │              │
   │             │            │ WorkflowStarted                │              │
   │             │            │────────────────────────────────▶              │
   │             │            │              │                 │              │
   │             │            │              │                 │ Evaluate     │
   │             │            │              │                 │ Enabled      │
   │             │            │              │                 │              │
   │             │            │ TransitionEnabled              │              │
   │             │            │◀───────────────────────────────│              │
   │             │            │              │                 │              │
   │             │            │ TransitionEnabled                             │
   │             │            │───────────────────────────────────────────────▶
   │             │            │              │                 │              │
   │             │            │              │                 │              │ Execute
   │             │            │              │                 │              │ (Dagger)
   │             │            │              │                 │              │
   │             │            │ TransitionFired                               │
   │             │            │◀──────────────────────────────────────────────│
   │             │            │              │                 │              │
   │             │            │              │                 │              │ ...task
   │             │            │              │                 │              │ runs...
   │             │            │              │                 │              │
   │             │            │ TransitionCompleted                           │
   │             │            │◀──────────────────────────────────────────────│
   │             │            │              │                 │              │
   │             │            │ TransitionCompleted            │              │
   │             │            │─────────────▶│                 │              │
   │             │            │              │                 │              │
   │             │            │              │ Update          │              │
   │             │            │              │ Marking         │              │
   │             │            │              │                 │              │
   │             │            │ MarkingUpdated                 │              │
   │             │            │◀─────────────│                 │              │
   │             │            │              │                 │              │
   │             │            │ MarkingUpdated                 │              │
   │             │            │────────────────────────────────▶              │
   │             │            │              │                 │              │
   │             │            │              │                 │ Evaluate     │
   │             │            │              │                 │ (repeat)     │
   │             │            │              │                 │              │
   │             │            │    ... cycle continues until final marking ...│
   │             │            │              │                 │              │
   │             │            │              │ Final marking   │              │
   │             │            │              │ detected        │              │
   │             │            │              │                 │              │
   │             │            │ WorkflowCompleted              │              │
   │             │            │◀─────────────│                 │              │
   │             │            │              │                 │              │
```

### Petri Net Execution Example

Consider a simple sequential workflow:

```
[start] ──▶ (task-1) ──▶ [ready] ──▶ (task-2) ──▶ [done]
   •
```

**Initial Marking**: `{start: 1, ready: 0, done: 0}`

| Step | Event | Marking After |
|------|-------|---------------|
| 1 | `WorkflowStarted` | `{start: 1, ready: 0, done: 0}` |
| 2 | `TransitionEnabled(task-1)` | (no change) |
| 3 | `TransitionFired(task-1)` | `{start: 0, ready: 0, done: 0}` |
| 4 | `TransitionCompleted(task-1)` | `{start: 0, ready: 1, done: 0}` |
| 5 | `TransitionEnabled(task-2)` | (no change) |
| 6 | `TransitionFired(task-2)` | `{start: 0, ready: 0, done: 0}` |
| 7 | `TransitionCompleted(task-2)` | `{start: 0, ready: 0, done: 1}` |
| 8 | `WorkflowCompleted` | (final) |

## NATS Subject Hierarchy

Events are published to hierarchical subjects for flexible subscription patterns:

```
cb.
├── workflows.
│   ├── {namespace}.
│   │   ├── submitted                    # New workflow definitions
│   │   └── {workflow_id}.
│   │       ├── started                  # Workflow runs started
│   │       ├── completed                # Workflow runs completed
│   │       ├── failed                   # Workflow runs failed
│   │       └── cancelled                # Workflow runs cancelled
│   │
├── runs.
│   └── {run_id}.
│       ├── status                       # Run status updates
│       ├── marking                      # Marking snapshots
│       └── transitions.
│           └── {transition_id}.
│               ├── enabled              # Transition can fire
│               ├── fired                # Transition started
│               ├── completed            # Transition succeeded
│               └── failed               # Transition failed
│
└── system.
    ├── runners.heartbeat                # Runner health checks
    ├── scheduler.assignments            # Task assignments
    └── metrics                          # System metrics
```

### Subscription Patterns

```bash
# Controller: All workflow events in a namespace
cb.workflows.production.>

# Scheduler: All marking updates
cb.runs.*.marking

# Runner: All enabled transitions (to pick up work)
cb.runs.*.transitions.*.enabled

# Dashboard: All events for a specific run
cb.runs.abc123.*
cb.runs.abc123.transitions.>
```

## Event Schemas

### WorkflowSubmitted

```json
{
  "eventId": "evt_abc123",
  "eventType": "WorkflowSubmitted",
  "timestamp": "2024-01-15T10:30:00Z",
  "data": {
    "workflowId": "wf_xyz789",
    "name": "build-and-deploy",
    "namespace": "production",
    "version": 1,
    "definition": { /* full workflow JSON */ }
  }
}
```

### TransitionEnabled

```json
{
  "eventId": "evt_def456",
  "eventType": "TransitionEnabled",
  "timestamp": "2024-01-15T10:30:05Z",
  "data": {
    "runId": "run_123",
    "workflowId": "wf_xyz789",
    "transitionId": "build",
    "inputPlaces": ["source-ready"],
    "currentMarking": {
      "source-ready": 1,
      "build-complete": 0
    },
    "priority": 10,
    "resourceRequirements": {
      "cpu": "2",
      "memory": "4Gi"
    }
  }
}
```

### TransitionCompleted

```json
{
  "eventId": "evt_ghi789",
  "eventType": "TransitionCompleted",
  "timestamp": "2024-01-15T10:35:00Z",
  "data": {
    "runId": "run_123",
    "workflowId": "wf_xyz789",
    "transitionId": "build",
    "duration": "4m55s",
    "outputs": {
      "imageDigest": "sha256:abc...",
      "buildLog": "s3://logs/build-123.txt"
    },
    "tokensProduced": {
      "build-complete": 1
    }
  }
}
```

## JetStream Configuration

### Streams

```yaml
# Workflow events stream
- name: WORKFLOWS
  subjects:
    - "cb.workflows.>"
  retention: limits
  max_age: 30d
  max_bytes: 10GB
  storage: file
  replicas: 3

# Run events stream  
- name: RUNS
  subjects:
    - "cb.runs.>"
  retention: limits
  max_age: 7d
  max_bytes: 50GB
  storage: file
  replicas: 3

# System events stream
- name: SYSTEM
  subjects:
    - "cb.system.>"
  retention: limits
  max_age: 1d
  max_bytes: 1GB
  storage: memory
  replicas: 1
```

### Consumers

```yaml
# Controller consumer - durable, processes all workflow events
- name: controller
  stream: WORKFLOWS
  durable: controller
  deliver_policy: all
  ack_policy: explicit
  max_deliver: 5

# Scheduler consumer - durable, processes marking updates
- name: scheduler
  stream: RUNS
  durable: scheduler
  filter_subject: "cb.runs.*.marking"
  deliver_policy: last_per_subject
  ack_policy: explicit

# Runner consumer - queue group for work distribution
- name: runner
  stream: RUNS
  durable: runner-group
  filter_subject: "cb.runs.*.transitions.*.enabled"
  deliver_policy: all
  ack_policy: explicit
  max_ack_pending: 10
```

## Failure Handling

### Transition Retry

```
TransitionEnabled ──▶ TransitionFired ──▶ TransitionFailed
                                                │
                                                ▼
                                          Retry Logic
                                                │
                              ┌─────────────────┼─────────────────┐
                              ▼                 ▼                 ▼
                        Retry (< max)    Max Retries       Non-Retryable
                              │           Exceeded              │
                              ▼                 │                ▼
                      TransitionEnabled         ▼          WorkflowFailed
                                          WorkflowFailed
```

### Dead Letter Queue

Failed events that exceed retry limits are moved to a dead letter subject:

```
cb.dlq.runs.{run_id}.transitions.{transition_id}
```

## Scaling Considerations

### Runner Scaling with Karpenter

```yaml
# Karpenter NodePool for runners
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: circuit-breaker-runners
spec:
  template:
    spec:
      requirements:
        - key: "workload-type"
          operator: In
          values: ["circuit-breaker-runner"]
      taints:
        - key: "circuit-breaker.io/runner"
          effect: NoSchedule
  limits:
    cpu: 1000
    memory: 2000Gi
  disruption:
    consolidationPolicy: WhenEmpty
```

### Event-Driven Autoscaling

The number of pending `TransitionEnabled` events drives runner scaling:

```
Pending Events    Runners
0-10              1
11-50             3
51-100            5
100+              10 (max)
```

## Observability

### Distributed Tracing

Each event carries trace context for end-to-end tracing:

```json
{
  "traceContext": {
    "traceId": "abc123def456",
    "spanId": "span789",
    "parentSpanId": "span456"
  }
}
```

### Metrics

Key metrics exported to Prometheus:

- `cb_workflows_submitted_total` - Counter of submitted workflows
- `cb_workflows_completed_total` - Counter of completed workflows
- `cb_transitions_enabled_total` - Counter of enabled transitions
- `cb_transitions_duration_seconds` - Histogram of transition durations
- `cb_marking_tokens_total` - Gauge of tokens per place
- `cb_nats_lag_messages` - Consumer lag in messages

## Benefits of Event-Driven Architecture

1. **Loose Coupling**: Components don't know about each other, only events
2. **Scalability**: Add more runners without changing other components
3. **Resilience**: Events are persisted; components can restart and resume
4. **Auditability**: Complete event history for debugging and compliance
5. **Flexibility**: Easy to add new consumers (dashboards, analytics, etc.)
6. **Replay**: Can rebuild state by replaying events from any point