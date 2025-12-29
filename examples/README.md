# Circuit Breaker Examples

This directory contains example workflows demonstrating various features and patterns of the Circuit Breaker workflow orchestration system.

## Examples

### 1. Hello World (`hello-world/`)

Basic introduction to Circuit Breaker workflows with simple sequential execution.

- **workflow.ts** - TypeScript SDK version
- **workflow.json** - Plain JSON version
- **parallel.ts** - Demonstrates parallel execution (fan-out/fan-in)
- **conditional.ts** - Shows conditional transitions with guards

**Run:**
```bash
cb run examples/hello-world/workflow.ts
```

### 2. CI Pipeline (`ci-pipeline/`)

A complete CI/CD pipeline demonstrating:
- Sequential build steps
- Parallel test execution (fan-out pattern)
- Synchronization points (fan-in/AND-join)
- Conditional deployment with guards
- Resource management
- Retry strategies

**Features:**
- Build → Test/Lint/Security (parallel) → Deploy (conditional)
- Dagger integration for containerized tasks
- Resource requirements (CPU/Memory)
- Timeout and retry configuration

**Run:**
```bash
cb run examples/ci-pipeline/workflow.ts
```

### 3. OpenCode Hello (`open-code/`)

Simple example demonstrating OpenCode AI agent integration:
- Minimal Petri-net workflow
- AI-powered project analysis
- Read-only (plan) mode

**Features:**
- Single transition workflow
- OpenCode task configuration
- Model and timeout settings

**Run:**
```bash
cb run examples/open-code/workflow.ts --watch
```

### 4. AI Code Review (`ai-code-review/`)

AI-powered workflow using OpenCode integration for:
- Automated code review (plan mode - read-only)
- Bug fixing (build mode - can modify files)
- Test generation
- Pull request creation

**Features:**
- OpenCode AI agent integration
- Multiple agent modes (plan vs build)
- Session continuation
- Auto-approval for file changes
- GitHub integration

**Run:**
```bash
cb run examples/ai-code-review/workflow.ts --input repository=https://github.com/org/repo --input branch=feature-branch
```

## Setup

### Prerequisites

- [Bun](https://bun.sh) >= 1.0.0
- TypeScript >= 5.3.0
- Circuit Breaker CLI (`cb`)

### Installation

From the project root:

```bash
# Install all dependencies
bun install

# Build the SDK
cd sdk/packages/core
bun run build
cd ../../..

# Typecheck examples
cd examples
bun run typecheck
```

## Development

### Project Structure

```
examples/
├── package.json           # Dependencies and scripts
├── tsconfig.json          # TypeScript configuration
├── hello-world/           # Basic examples
├── ci-pipeline/           # CI/CD workflow
└── ai-code-review/        # AI-powered workflow
```

### TypeScript Configuration

The examples use path mapping to reference the SDK:

```json
{
  "paths": {
    "@circuit-breaker/core": ["../sdk/packages/core/src/index.ts"]
  }
}
```

This allows importing the SDK without building it first:

```typescript
import { workflow, opencode, OpenCodeTasks } from "@circuit-breaker/core";
```

### Available Scripts

```bash
# Type check all examples
bun run typecheck

# Build (no-op for examples, just type checking)
bun run build

# Clean
bun run clean
```

## Writing Your Own Workflows

### Basic Workflow Structure

```typescript
import { workflow } from "@circuit-breaker/core";

export default workflow("my-workflow")
  .namespace("examples")
  .description("My custom workflow")
  
  // Define places (states)
  .place("start", { initialTokens: 1 })
  .place("done")
  
  // Define transitions (actions)
  .transition("do-something")
    .from("start")
    .to("done")
    .dagger("./ci", "build")
    .timeout("5m")
    .retries(2)
    .done()
  
  .build();
```

### Petri Net Patterns

#### Sequential Execution
```
[place1] → (transition1) → [place2] → (transition2) → [place3]
```

#### Parallel Execution (Fan-out)
```
                    → (task1) → [result1] →
[start] → (split) →  → (task2) → [result2] →  → (join) → [done]
                    → (task3) → [result3] →
```

#### Conditional Execution
```typescript
.transition("deploy-prod")
  .from("tested")
  .to("deployed")
  .guard('ctx.branch == "main" && ctx.event != "pull_request"')
  .dagger("./ci", "deploy", { environment: "production" })
  .done()
```

### Task Types

#### 1. Dagger Tasks
```typescript
.dagger("./ci", "build", {
  target: "release",
  platform: "linux/amd64"
})
```

#### 2. HTTP Requests
```typescript
.http("https://api.example.com/notify", {
  method: "POST",
  headers: { "Authorization": "Bearer ${secrets.token}" },
  body: JSON.stringify({ message: "Build complete" })
})
```

#### 3. Inline Scripts
```typescript
.script(`
  console.log('Running custom logic');
  return { success: true };
`, "bun")
```

#### 4. OpenCode AI Tasks
```typescript
.opencode(
  opencode("Review this code for security issues")
    .plan()
    .model("anthropic", "claude-sonnet-4-20250514")
    .files("src/auth.ts")
)
```

#### 5. No-op (Synchronization)
```typescript
.noop()  // Just moves tokens, no action
```

## CLI Commands

```bash
# Validate workflow syntax
cb validate examples/ci-pipeline/workflow.ts

# Visualize workflow as a graph
cb visualize examples/ci-pipeline/workflow.ts

# Submit workflow to the engine
cb submit examples/ci-pipeline/workflow.ts

# Run workflow and watch execution
cb run examples/ci-pipeline/workflow.ts --watch

# Run with inputs
cb run examples/ai-code-review/workflow.ts \
  --input repository=https://github.com/org/repo \
  --input branch=feature-branch
```

## Testing Workflows

Circuit Breaker workflows are deterministic and can be tested:

```typescript
import { describe, test, expect } from "bun:test";
import workflow from "./workflow.ts";

describe("CI Pipeline", () => {
  test("should have correct structure", () => {
    expect(workflow.name).toBe("ci-pipeline");
    expect(workflow.places).toHaveLength(8);
    expect(workflow.transitions).toHaveLength(7);
  });

  test("should have initial token in source-ready", () => {
    const sourcePlace = workflow.places.find(p => p.id === "source-ready");
    expect(sourcePlace?.initialTokens).toBe(1);
  });
});
```

## Best Practices

1. **Use Meaningful Names**: Place and transition IDs should be descriptive
2. **Set Timeouts**: Always configure appropriate timeouts for transitions
3. **Add Resources**: Specify CPU/memory requirements for proper scheduling
4. **Use Guards**: Implement conditional logic with guard expressions
5. **Handle Errors**: Configure retries with appropriate backoff strategies
6. **Document Workflows**: Add descriptions and comments
7. **Validate Early**: Use `cb validate` before submitting
8. **Visualize**: Use `cb visualize` to understand workflow structure

## Resources

- [Circuit Breaker Documentation](../../README.md)
- [Petri Net Theory](https://en.wikipedia.org/wiki/Petri_net)
- [Dagger Documentation](https://docs.dagger.io)
- [OpenCode Documentation](https://github.com/sst/opencode)

## Troubleshooting

### TypeScript Errors

If you see "Cannot find module '@circuit-breaker/core'":

```bash
# Ensure dependencies are installed
cd ../
bun install

# Rebuild the SDK
cd sdk/packages/core
bun run build
```

### Workflow Validation Errors

```bash
# Check syntax
cb validate examples/your-workflow/workflow.ts

# View detailed error messages
cb validate examples/your-workflow/workflow.ts --verbose
```

### Runtime Errors

- Check logs: `cb logs <workflow-id>`
- Inspect state: `cb status <workflow-id>`
- View events: `cb events <workflow-id>`

## Contributing

To add a new example:

1. Create a new directory under `examples/`
2. Add a `workflow.ts` file with your workflow definition
3. Add a `README.md` explaining the example
4. Update this README with a link to your example
5. Test with `cb validate` and `cb run`

## License

MIT