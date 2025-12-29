# Circuit Breaker

A cloud-native workflow orchestration platform combining Petri-net formal modeling with Dagger.io pipelines, powered by NATS event-driven architecture and Karpenter autoscaling.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      Developer Experience (Bun + TypeScript)             │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │  @circuit-breaker/sdk          @circuit-breaker/cli                │ │
│  │  - Workflow DSL                - cb submit workflow.ts             │ │
│  │  - Type-safe builders          - cb run / status / logs            │ │
│  │  - Zod schemas                 - cb inject / describe              │ │
│  └────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                          JSON IR (compiled workflow)
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              NATS JetStream                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌───────────────┐  │
│  │  workflows  │  │  runs       │  │  tokens     │  │  transitions  │  │
│  │  .submit    │  │  .status    │  │  .injected  │  │  .enabled     │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └───────────────┘  │
│                                                                         │
│  KV Buckets:  workflow_state | token_state | run_history               │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Rust Engine Services                             │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
│  │  cb-api         │  │  cb-runner      │  │  cb-controller          │ │
│  │  - REST API     │  │  - Dagger exec  │  │  - K8s operator         │ │
│  │  - Event sub    │  │  - Docker/OCI   │  │  - Autoscaling          │ │
│  │  - Token inject │  │  - OpenCode AI  │  │  - Pod lifecycle        │ │
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
- **Token Schemas**: JSON Schema definitions for typed/colored tokens

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
- **Token Injection**: Manual token injection via API/CLI with full event trail
- **No external database required**: State lives in NATS KV/Object Store

### Dagger Integration

Each transition can execute a Dagger pipeline:

- Container-native CI/CD
- Cacheable, reproducible builds
- Language-agnostic (TypeScript, Python, Go modules)
- Local and remote execution

### OpenCode AI Integration

Built-in support for AI-powered tasks via OpenCode:

- Code review and analysis
- Bug fixing and refactoring
- Test generation
- Documentation

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
    ├── hello-world/            # Basic examples
    ├── ci-pipeline/            # CI/CD workflow
    ├── ai-code-review/         # AI-powered code review
    └── open-code/              # Simple OpenCode example
```

## Quick Start

### Prerequisites

- [Bun](https://bun.sh) >= 1.0
- [Rust](https://rustup.rs) >= 1.75
- [Docker](https://docker.com)
- [NATS Server](https://nats.io) with JetStream

### 1. Install Dependencies

```bash
# Install SDK dependencies
cd sdk && bun install

# Build Rust engine
cd engine && cargo build
```

### 2. Start NATS

```bash
docker run -d --name nats -p 4222:4222 -p 8222:8222 nats:latest -js
```

### 3. Start the API Server

```bash
cd engine && cargo run --bin cb-api
```

You should see:
```
INFO cb_api: Starting Circuit Breaker API server host=0.0.0.0 port=8080
INFO cb_api: Connected to NATS at nats://localhost:4222
INFO cb_api: RUNS stream ready
INFO cb_api: WORKFLOWS stream ready
INFO cb_api: API server listening on 0.0.0.0:8080
```

### 4. Start the Runner

In a separate terminal:

```bash
# Set API keys for AI providers (optional, for OpenCode)
export ANTHROPIC_API_KEY="your-key-here"

cd engine && cargo run --bin cb-runner
```

You should see:
```
INFO cb_runner: Starting Circuit Breaker Runner
INFO cb_runner: Connected to NATS
INFO cb_runner: Runner ready - waiting for tasks...
```

### 5. Run a Workflow

```bash
# Run a simple workflow
./cb run examples/open-code/workflow.ts --watch

# Or run step by step:
./cb submit examples/open-code/workflow.ts   # Submit workflow
./cb run <workflow-id>                        # Start a run
./cb status <run-id>                          # Check status
./cb logs <run-id>                            # View logs
```

## Define a Workflow

### Basic Workflow (TypeScript)

```typescript
import { workflow } from '@circuit-breaker/core';

export default workflow('ci-pipeline')
  .namespace('examples')
  .description('CI/CD pipeline with build, test, deploy')
  
  // Define places (states)
  .place('source', { initialTokens: 1 })
  .place('built')
  .place('tested')
  .place('deployed')

  // Define transitions (actions)
  .transition('build')
    .from('source')
    .to('built')
    .dagger('./ci', 'build')
    .timeout('10m')
    .retries(2)
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

### Workflow with Token Schemas

```typescript
import { workflow } from '@circuit-breaker/core';

export default workflow('data-pipeline')
  .place('input', { 
    initialTokens: 1,
    tokenSchema: {
      type: 'object',
      properties: {
        repository: { type: 'string', format: 'uri' },
        branch: { type: 'string' },
        commit: { type: 'string' }
      },
      required: ['repository', 'branch']
    }
  })
  .place('processed')
  .place('output')
  
  // Transitions...
  .build();
```

### AI-Powered Workflow with OpenCode

```typescript
import { workflow, opencode } from '@circuit-breaker/core';

export default workflow('ai-review')
  .place('start', { initialTokens: 1 })
  .place('done')

  .transition('analyze')
    .from('start')
    .to('done')
    .opencode(
      opencode('Analyze this codebase and summarize the architecture')
        .plan()  // Read-only mode
        .model('anthropic', 'claude-sonnet-4-5-20250929')
        .timeout(300)
    )
    .done()

  .build();
```

## CLI Reference

### Workflow Management

```bash
# Validate a workflow
./cb validate examples/ci-pipeline/workflow.ts

# Submit a workflow definition
./cb submit examples/ci-pipeline/workflow.ts

# Run a workflow (submit + start)
./cb run examples/ci-pipeline/workflow.ts --watch

# Visualize workflow structure
./cb visualize examples/ci-pipeline/workflow.ts --open
```

### Run Management

```bash
# Check run status
./cb status <run-id>
./cb status <run-id> --watch

# View run logs
./cb logs <run-id>
./cb logs <run-id> --follow
./cb logs <run-id> --json

# Describe run (places, tokens, schemas)
./cb describe <run-id>

# Cancel a run
./cb cancel <run-id>
```

### Token Injection

Inject tokens into specific places for testing or manual intervention:

```bash
# Inject a simple token
./cb inject <run-id> <place-id>

# Inject token with data
./cb inject <run-id> start --data '{"repo": "https://github.com/org/repo"}'

# Inject with reason (for audit trail)
./cb inject <run-id> start --data '{"repo": "..."}' --reason "Manual retry"

# View expected token schema for a place
./cb inject <run-id> <place-id> --show-schema
```

### System Commands

```bash
# Check API health
./cb health
```

## NATS Event Subjects

Circuit Breaker publishes events to NATS JetStream for observability and integration:

| Subject Pattern | Description |
|----------------|-------------|
| `cb.workflows.{ns}.submitted` | Workflow submitted |
| `cb.runs.{run_id}.status` | Run status changes |
| `cb.runs.{run_id}.tokens.{place}.injected` | Token injected |
| `cb.runs.{run_id}.transitions.{id}.enabled` | Transition enabled |
| `cb.runs.{run_id}.transitions.{id}.fired` | Transition started |
| `cb.runs.{run_id}.transitions.{id}.completed` | Transition completed |
| `cb.runs.{run_id}.transitions.{id}.failed` | Transition failed |

Subscribe to events:
```bash
# All run events
nats sub "cb.runs.>"

# Token injections
nats sub "cb.runs.*.tokens.*.injected"

# Transition completions
nats sub "cb.runs.*.transitions.*.completed"
```

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/health` | Health check |
| `POST` | `/api/v1/workflows` | Submit workflow |
| `GET` | `/api/v1/workflows` | List workflows |
| `GET` | `/api/v1/workflows/{id}` | Get workflow |
| `DELETE` | `/api/v1/workflows/{id}` | Delete workflow |
| `POST` | `/api/v1/workflows/{id}/runs` | Start run |
| `GET` | `/api/v1/runs` | List runs |
| `GET` | `/api/v1/runs/{id}` | Get run status |
| `GET` | `/api/v1/runs/{id}/logs` | Get run logs |
| `GET` | `/api/v1/runs/{id}/places` | Get places & schemas |
| `POST` | `/api/v1/runs/{id}/inject` | Inject token |
| `POST` | `/api/v1/runs/{id}/cancel` | Cancel run |

## Examples

### Hello World
```bash
./cb run examples/hello-world/workflow.ts --watch
```

### CI Pipeline
```bash
./cb run examples/ci-pipeline/workflow.ts --watch
```

### OpenCode AI
```bash
# Requires ANTHROPIC_API_KEY
export ANTHROPIC_API_KEY="your-key"
./cb run examples/open-code/workflow.ts --watch
./cb logs <run-id>  # View AI output
```

### Manual Token Injection
```bash
# Submit workflow
./cb submit examples/open-code/workflow.ts
# Returns: Workflow ID: abc123

# Create a run without auto-start
curl -X POST http://localhost:8080/api/v1/workflows/abc123/runs

# Inject token to trigger specific transition
./cb inject <run-id> start --reason "Manual test"

# Watch execution
./cb status <run-id> --watch
./cb logs <run-id>
```

## Development

### Building

```bash
# Build everything
cd sdk && bun install && bun run build
cd engine && cargo build

# Run tests
cd sdk && bun test
cd engine && cargo test
```

### Running Locally

Terminal 1 - NATS:
```bash
docker run -d --name nats -p 4222:4222 -p 8222:8222 nats:latest -js
```

Terminal 2 - API:
```bash
cd engine && cargo run --bin cb-api
```

Terminal 3 - Runner:
```bash
export ANTHROPIC_API_KEY="..."  # For OpenCode
cd engine && cargo run --bin cb-runner
```

Terminal 4 - CLI:
```bash
./cb run examples/open-code/workflow.ts --watch
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `NATS_URL` | NATS server URL | `nats://localhost:4222` |
| `CB_API_PORT` | API server port | `8080` |
| `CB_RUNNER_POOL` | Runner pool name | `default` |
| `ANTHROPIC_API_KEY` | Anthropic API key | - |
| `OPENAI_API_KEY` | OpenAI API key | - |

## License

MIT