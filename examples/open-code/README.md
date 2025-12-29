# OpenCode Example: Tell Me About Yourself

A simple workflow demonstrating OpenCode AI agent integration with Circuit Breaker.

## Overview

This example defines a minimal Petri-net workflow that executes an OpenCode AI task. When submitted to the Circuit Breaker engine, the workflow goes through the following execution flow:

```
┌─────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────┐
│   CLI   │────▶│  API Server │────▶│    NATS     │────▶│ Runner  │
│ cb run  │     │   cb-api    │     │  JetStream  │     │cb-runner│
└─────────┘     └─────────────┘     └─────────────┘     └────┬────┘
                                                              │
                                                              ▼
                                                        ┌─────────┐
                                                        │ Dagger  │
                                                        │Container│
                                                        │OpenCode │
                                                        └─────────┘
```

## Workflow Structure

```
[start] ──▶ (analyze) ──▶ [done]
   •
```

- **Places**: `start` (with initial token), `done`
- **Transitions**: `analyze` (OpenCode AI task)

## Prerequisites

1. **Circuit Breaker Engine Running**
   ```bash
   # Start NATS with JetStream
   docker run -d --name nats -p 4222:4222 nats:latest -js

   # Start the engine
   cd engine && cargo run --bin cb-api
   ```

2. **API Key Configuration**
   
   The API key must be available to the Dagger container running OpenCode. Set it on the Runner host:
   ```bash
   # Set on the Runner host (where cb-runner executes)
   export ANTHROPIC_API_KEY="your-key-here"
   ```
   
   The Runner passes this environment variable to the container.

## Running the Example

### Submit and Run

```bash
# Submit the workflow to the engine
cb submit examples/open-code/workflow.ts

# Or run directly with live output
cb run examples/open-code/workflow.ts --watch
```

### Check Status

```bash
# View workflow status
cb status <run-id>

# View execution logs
cb logs <run-id>
```

## How It Works

1. **Submit**: The CLI compiles the TypeScript workflow to JSON IR and POSTs it to the API server
2. **Controller**: Receives the workflow, creates initial marking (token in `start` place)
3. **Scheduler**: Evaluates firing rules, sees `analyze` transition is enabled
4. **Runner**: Claims the transition, executes via Dagger:
   - Pulls OpenCode container image
   - Mounts workspace
   - Runs OpenCode with the prompt in `plan` mode
   - Returns output
5. **Complete**: Token moves to `done`, workflow completes

## Workflow Definition

```typescript
import { workflow, opencode } from "@circuit-breaker/core";

export default workflow("opencode-hello")
  .namespace("examples")
  .place("start", { initialTokens: 1 })
  .place("done")

  .transition("analyze")
    .from("start")
    .to("done")
    .opencode(
      opencode("Tell me about this project...")
        .plan()
        .model("anthropic", "claude-sonnet-4-20250514")
        .timeout(300)
      // ANTHROPIC_API_KEY is passed from Runner's environment
    )
    .timeout("10m")
    .resources({ cpu: "500m", memory: "1Gi" })
    .done()

  .build();
```

## Configuration Options

### OpenCode Task Builder

| Method | Description |
|--------|-------------|
| `.plan()` | Read-only mode (no file modifications) |
| `.build()` | Full access mode (can modify files) |
| `.model(provider, model)` | Set AI provider and model |
| `.files(...paths)` | Attach specific files to analyze |
| `.timeout(seconds)` | Set execution timeout |
| `.format("json")` | Get structured JSON output |

### Transition Options

| Method | Description |
|--------|-------------|
| `.timeout(duration)` | Maximum execution time (e.g., "10m") |
| `.resources({ cpu, memory })` | Resource requests for the pod |
| `.retries(count)` | Number of retry attempts on failure |

## Troubleshooting

### Workflow stuck in pending

Check that the engine components are running:
```bash
cb status <run-id> --verbose
```

### Transition failed

View the runner logs:
```bash
cb logs <run-id> --transition analyze
```

### API key not found

Ensure the Runner has the API key in its environment:
```bash
# On the Runner host
export ANTHROPIC_API_KEY="your-key-here"

# Or in Kubernetes, configure as a secret in the Runner deployment
```

## Next Steps

- [CI Pipeline Example](../ci-pipeline/) - Full CI/CD workflow
- [Architecture Docs](../../docs/architecture/) - Deep dive into the engine
