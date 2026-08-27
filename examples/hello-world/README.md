# Hello World Examples

Simple introductory workflows demonstrating Circuit Breaker and Petri net concepts.

## Quick Start with the TUI

The easiest way to run these examples is with the interactive TUI:

```bash
# Start all services (NATS, API, Controller, Runner) and launch TUI
cb dev

# Then use slash commands to run workflows
```

---

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

**Run it in the TUI:**
```
❯ /run examples/hello-world/workflow.ts

[10:23:20] Loaded workflow: hello-world
[10:23:20] Validation passed
[10:23:21] Submitted: wf-abc123
[10:23:21] Started run: run-xyz789
[10:23:22] == run-xyz789 == [say-hello] : Hello
[10:23:22] Transition completed: say-hello
[10:23:23] == run-xyz789 == [say-world] : World!
[10:23:23] Transition completed: say-world
[10:23:23] Run status: completed
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

**Run it in the TUI:**
```
❯ /run examples/hello-world/parallel.ts

[10:25:00] Loaded workflow: parallel-example
[10:25:01] Started run: run-abc456
[10:25:02] == run-abc456 == [fork] : Forking into parallel tasks...
[10:25:02] Transition completed: fork
[10:25:03] Transition enabled: join
[10:25:03] == run-abc456 == [join] : All parallel tasks complete! Joining...
[10:25:03] Transition completed: join
[10:25:03] Run status: completed
```

---

### 3. Conditional Branching (`conditional.ts`)

Demonstrates XOR-split pattern with guard conditions.

```
                   [guard: ctx.score >= 70]
                        ┌──▶ (pass) ──▶ [passed]
                        │
[start] ──▶ (evaluate) ─┤
   •                    │
                        └──▶ (fail) ──▶ [failed]
                   [guard: ctx.score < 70]
```

**Concepts demonstrated:**
- **XOR-split**: Mutually exclusive branches
- **Guards**: CEL expressions controlling transition firing
- **Gate pattern**: Tokens wait until guards are satisfied
- Conditional execution based on data

**Run it in the TUI:**
```
❯ /run examples/hello-world/conditional.ts

[10:30:00] Loaded workflow: conditional-example
[10:30:01] Started run: run-def789
[10:30:02] == run-def789 == [evaluate] : Evaluation score: 85
[10:30:02] Transition completed: evaluate
[10:30:02] Transition enabled: pass (guard: ctx.score >= 70)
[10:30:03] == run-def789 == [pass] : ✓ Passed! Score: 85
[10:30:03] Transition completed: pass
[10:30:03] Run status: completed
```

**Using `/resume` to retry with different data:**

If the score was < 70, the "pass" transition won't fire because its guard fails.
The token waits in the `evaluated` place. You can use `/resume` to update the token
data and re-evaluate the guard:

```
# Workflow is waiting at 'evaluated' place - guard failed with low score
# Update the token data to satisfy the guard:

❯ /resume evaluated '{"score": 95}'

[10:35:00] Resumed: updated token in evaluated
[10:35:00] Guards passed! Enabled: pass
[10:35:01] == run-def789 == [pass] : ✓ Passed! Score: 95
```

This is the key differentiator from traditional CI systems - you can **resume from any checkpoint** with modified data instead of restarting the entire pipeline.

---

## TUI Command Reference

| Command | Description | Example |
|---------|-------------|---------|
| `/run <file>` | Run a workflow file | `/run workflow.ts` |
| `/status` | Show current run status | `/status` |
| `/describe` | Show workflow structure with token positions | `/describe` |
| `/resume <place> <data>` | Resume by updating token data to satisfy a failed guard | `/resume evaluated '{"score": 95}'` |
| `/inject <place> [data]` | Inject a new token into a place | `/inject start '{"input": "value"}'` |
| `/logs` | View logs for current run | `/logs` |
| `/list` | List recent runs | `/list` |
| `/cancel` | Cancel current run | `/cancel` |
| `/clear` | Clear the log output | `/clear` |
| `/help` | Show all commands | `/help` |
| `/quit` | Exit the TUI | `/quit` |

---

## Petri Net Primer

### Places (circles)
- Represent **states** or **conditions**
- Can hold zero or more **tokens**
- Drawn as circles: `[place-name]`

### Transitions (rectangles)
- Represent **actions** or **events**
- Fire when all input places have required tokens AND guards pass
- Drawn as: `(transition-name)`

### Tokens (dots)
- Represent **work items** or **data**
- Flow through the net as transitions fire
- Can carry data (colored tokens): `{ score: 85 }`
- Drawn as: `•`

### Guards
- CEL expressions that control when a transition can fire
- Access token data via `ctx`: `ctx.score >= 70`
- Transitions wait (gate) until guard is satisfied

### Firing Rules
1. A transition is **enabled** when:
   - All input places have sufficient tokens
   - Guard expression evaluates to `true` (if present)
2. When a transition **fires**:
   - Tokens are **consumed** from input places
   - Tokens are **produced** to output places
   - Output data becomes the new token data
3. The workflow completes when tokens reach terminal places

---

## Running Examples Step by Step

### Prerequisites

Make sure you have the CLI installed:
```bash
cd ../../sdk && bun install && bun link
```

### Using `cb dev` (Recommended)

The `cb dev` command starts everything you need:

```bash
# From anywhere in the circuit-breaker project
cb dev
```

This starts:
1. **NATS** - Event messaging (if not already running)
2. **cb-api** - REST API server
3. **cb-runner** - Task executor
4. **TUI** - Interactive terminal interface

Once the TUI is running:
```
❯ /run examples/hello-world/workflow.ts
❯ /status
❯ /describe
```

### Manual Setup (Alternative)

If you prefer to run components separately:

```bash
# Terminal 1: Start NATS
docker run -d --name nats -p 4222:4222 nats:latest -js

# Terminal 2: Start the API server
cd engine && cargo run --bin cb-api

# Terminal 3: Start the runner
cd engine && cargo run --bin cb-runner

# Terminal 4: Launch TUI
cb
```

---

## Example Workflows Explained

### workflow.ts - Sequential Flow
```typescript
// Token starts at 'start'
// say-hello consumes from 'start', produces to 'hello-done'
// say-world consumes from 'hello-done', produces to 'complete'
```

### parallel.ts - Fork/Join
```typescript
// Fork: 1 token in 'start' → 3 tokens out (task-a-done, task-b-done, task-c-done)
// Join: Waits for ALL 3 tokens → produces 1 token to 'complete'
```

### conditional.ts - Guarded Branches
```typescript
// evaluate: produces token with { score: N } data
// pass: guard "ctx.score >= 70" - only fires if score is high enough
// fail: guard "ctx.score < 70" - only fires if score is too low
// Only ONE branch fires based on the guard conditions
```

---

## Next Steps

After understanding these basics, explore:

1. **[CI Pipeline Example](../ci-pipeline/)** - Real-world CI/CD workflow
2. **Colored Petri Nets** - Tokens carrying complex data structures
3. **Dagger Integration** - Container-native actions
4. **Resource Requirements** - CPU/memory/GPU allocation
5. **Retry Policies** - Handling failures gracefully
