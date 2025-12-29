# Hello World Examples

Simple introductory workflows demonstrating Circuit Breaker and Petri net concepts.

## Examples

### 1. Basic Sequential (`workflow.ts`)

The simplest possible workflow - two steps executed in sequence.

```
[start] ──▶ (say-hello) ──▶ [hello-done] ──▶ (say-world) ──▶ [complete]
   •
```

**Concepts demonstrated:**
- Places (states)
- Transitions (actions)
- Initial marking (starting token)
- Sequential execution

**Run it:**
```bash
cb run ./workflow.ts --watch
```

---

### 2. Parallel Execution (`parallel.ts`)

Demonstrates the fork/join pattern for parallel execution.

```
                        ┌──▶ [task-a-done] ──┐
                        │                    │
[start] ──▶ (fork) ─────┼──▶ [task-b-done] ──┼──▶ (join) ──▶ [complete]
   •                    │                    │
                        └──▶ [task-c-done] ──┘
```

**Concepts demonstrated:**
- **AND-split (fork)**: One transition producing multiple tokens
- **AND-join**: One transition requiring multiple input tokens
- Synchronization barrier

**Run it:**
```bash
cb run ./parallel.ts --watch
```

---

### 3. Conditional Branching (`conditional.ts`)

Demonstrates XOR-split pattern with guard conditions.

```
                   [guard: score >= 70]
                        ┌──▶ (pass) ──▶ [passed]
                        │
[start] ──▶ (evaluate) ─┤
   •                    │
                        └──▶ (fail) ──▶ [failed]
                   [guard: score < 70]
```

**Concepts demonstrated:**
- **XOR-split**: Mutually exclusive branches
- **Guards**: CEL expressions controlling transition firing
- Conditional execution based on data

**Run it:**
```bash
cb run ./conditional.ts --watch
```

---

## Petri Net Primer

### Places (circles)
- Represent **states** or **conditions**
- Can hold zero or more **tokens**
- Drawn as circles: `[place-name]`

### Transitions (rectangles)
- Represent **actions** or **events**
- Fire when all input places have required tokens
- Drawn as: `(transition-name)`

### Tokens (dots)
- Represent **work items** or **data**
- Flow through the net as transitions fire
- Drawn as: `•`

### Firing Rules
1. A transition is **enabled** when all input places have sufficient tokens
2. When a transition **fires**:
   - Tokens are **consumed** from input places
   - Tokens are **produced** to output places
3. The workflow completes when tokens reach terminal places

---

## Running Examples

### Prerequisites

```bash
# Install the SDK
cd ../../sdk && bun install

# Start NATS (required for the engine)
docker run -d --name nats -p 4222:4222 nats:latest -js

# Start the API server
cd ../../engine && cargo run --bin cb-api
```

### Submit and Run

```bash
# Validate a workflow
cb validate ./workflow.ts

# Submit without running
cb submit ./workflow.ts

# Submit and run immediately
cb run ./workflow.ts

# Run with watch mode (see real-time updates)
cb run ./workflow.ts --watch

# Visualize the Petri net
cb visualize ./workflow.ts --format mermaid
cb visualize ./workflow.ts --format dot --open
```

### Programmatic Usage

```typescript
import { helloWorld } from './workflow';
import { CircuitBreakerClient, validateWorkflow } from '@circuit-breaker/core';

// Validate locally
const validation = validateWorkflow(helloWorld);
if (!validation.valid) {
  console.error('Validation errors:', validation.errors);
  process.exit(1);
}

// Submit to the engine
const client = new CircuitBreakerClient();
const { workflowId } = await client.submitWorkflow(helloWorld);
console.log('Submitted:', workflowId);

// Run the workflow
const { runId } = await client.runWorkflow(workflowId);
console.log('Started run:', runId);

// Watch for completion
for await (const status of client.watchRun(runId)) {
  console.log('Status:', status.status);
  if (status.status === 'completed') {
    console.log('Done!');
    break;
  }
}
```

---

## Next Steps

After understanding these basics, explore:

1. **[CI Pipeline Example](../ci-pipeline/)** - Real-world CI/CD workflow
2. **Colored Petri Nets** - Tokens carrying data
3. **Dagger Integration** - Container-native actions
4. **Resource Requirements** - CPU/memory/GPU allocation
5. **Retry Policies** - Handling failures gracefully