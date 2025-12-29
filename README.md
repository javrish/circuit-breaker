# Circuit Breaker

A cloud-native workflow orchestration platform combining Petri-net formal modeling with Dagger.io pipelines, powered by NATS event-driven architecture and Karpenter autoscaling.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      Developer Experience (Bun + TypeScript)             │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │  @circuit-breaker/sdk          @circuit-breaker/cli                │ │
│  │  - Workflow DSL                - cb submit workflow.ts             │ │
│  │  - Type-safe builders          - cb status <workflow-id>           │ │
│  │  - Zod schemas                 - cb logs <run-id>                  │ │
│  └────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                          JSON IR (compiled workflow)
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              NATS JetStream                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌───────────────┐  │
│  │  workflows  │  │  events     │  │  tasks      │  │  results      │  │
│  │  .submit    │  │  .fired     │  │  .dispatch  │  │  .complete    │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └───────────────┘  │
│                                                                         │
│  KV Buckets:  workflow_state | token_state | run_history               │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Rust Engine Services                             │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
│  │  petri-engine   │  │  scheduler      │  │  api-server             │ │
│  │  - Token mgmt   │  │  - Task queue   │  │  - REST/gRPC            │ │
│  │  - Firing rules │  │  - Dispatch     │  │  - WebSocket (live)     │ │
│  │  - Event source │  │  - Timeouts     │  │  - Workflow CRUD        │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    Kubernetes + Karpenter                                │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Dagger Runner Pods (ephemeral, scale-to-zero)                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

## Key Concepts

### Petri Net Workflows

Circuit Breaker uses [Petri nets](https://en.wikipedia.org/wiki/Petri_net) as the formal model for workflows:

- **Places**: States or conditions in your workflow (e.g., "source ready", "build complete")
- **Transitions**: Actions that execute when enabled (e.g., "build", "test", "deploy")
- **Tokens**: Markers that flow through the net, representing work items or data
- **Arcs**: Connections between places and transitions defining the flow

This formal model enables:
- Deadlock detection at compile time
- Concurrent execution with proper synchronization
- Visual representation of complex workflows
- Mathematical analysis of workflow properties

### Event-Driven Architecture

Inspired by Temporal/Cadence but lightweight:

- **Event Sourcing**: Workflow state derived from event log
- **Replay Recovery**: Rebuild state from events on restart
- **NATS JetStream**: Durable, exactly-once message delivery
- **No external database required**: State lives in NATS KV/Object Store

### Dagger Integration

Each transition can execute a Dagger pipeline:

- Container-native CI/CD
- Cacheable, reproducible builds
- Language-agnostic (TypeScript, Python, Go modules)
- Local and remote execution

## Project Structure

```
circuit-breaker/
├── engine/                     # Rust workspace
│   └── crates/
│       ├── cb-core/            # Shared types, events
│       ├── cb-engine/          # Petri net execution
│       ├── cb-nats/            # NATS client
│       ├── cb-scheduler/       # Task dispatch
│       ├── cb-controller/      # Kubernetes operator
│       ├── cb-api/             # HTTP/gRPC API
│       └── cb-runner/          # Dagger executor
│
├── sdk/                        # Bun/TypeScript packages
│   └── packages/
│       ├── core/               # @circuit-breaker/core
│       ├── cli/                # @circuit-breaker/cli
│       └── dagger/             # @circuit-breaker/dagger
│
├── schemas/                    # JSON Schemas (contract)
├── k8s/                        # Kubernetes manifests
└── examples/                   # Example workflows
```

## Quick Start

### Define a Workflow (TypeScript)

```typescript
import { workflow } from '@circuit-breaker/core';

export default workflow('ci-pipeline')
  .place('source', { initialTokens: 1 })
  .place('built')
  .place('tested')
  .place('deployed')

  .transition('build')
    .from('source')
    .to('built')
    .dagger('./ci', 'build')
    .done()

  .transition('test')
    .from('built')
    .to('tested')
    .dagger('./ci', 'test')
    .done()

  .transition('deploy')
    .from('tested')
    .to('deployed')
    .guard('ctx.branch == "main"')
    .dagger('./ci', 'deploy')
    .done()

  .build();
```

### Submit and Run

```bash
# Submit workflow
bun run cb submit ./workflow.ts

# Watch execution
bun run cb run ./workflow.ts --watch

# Check status
bun run cb status <run-id>
```

## Development

### Prerequisites

- [Bun](https://bun.sh) >= 1.0
- [Rust](https://rustup.rs) >= 1.75
- [NATS Server](https://nats.io) with JetStream
- [Docker](https://docker.com) (for Dagger)
- [kubectl](https://kubernetes.io/docs/tasks/tools/) (for K8s deployment)

### Setup

```bash
# Install SDK dependencies
cd sdk && bun install

# Build Rust engine
cd engine && cargo build

# Start local NATS
docker run -d --name nats -p 4222:4222 -p 8222:8222 nats:latest -js

# Run the engine
cargo run --bin cb-api
```

## License

MIT