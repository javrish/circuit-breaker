# Engine Execution Flow

This document explains how a workflow definition (TypeScript/JSON) connects to the Rust engine system for executing Dagger modules.

## Overview

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           Workflow to Engine Execution Flow                          │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                     │
│  ┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐            │
│  │  workflow.ts    │      │  cb-controller  │      │   cb-runner     │            │
│  │                 │      │                 │      │                 │            │
│  │ • Places        │─────▶│ • Validates     │─────▶│ • Claims task   │            │
│  │ • Transitions   │ HTTP │ • Stores        │ NATS │ • Selects engine│            │
│  │ • Actions       │      │ • Monitors      │      │ • Executes      │            │
│  │ • Engine config │      │ • Fires         │      │ • Reports       │            │
│  └─────────────────┘      └─────────────────┘      └────────┬────────┘            │
│                                                             │                      │
│                                                             ▼                      │
│                                               ┌─────────────────────────┐          │
│                                               │    EngineSelector       │          │
│                                               │                         │          │
│                                               │  mode=auto? ────────┐   │          │
│                                               │         │           │   │          │
│                                               │         ▼           ▼   │          │
│                                               │  ┌──────────┐ ┌──────┐ │          │
│                                               │  │  Local   │ │Cloud │ │          │
│                                               │  │  Engine  │ │Engine│ │          │
│                                               │  └────┬─────┘ └──┬───┘ │          │
│                                               └───────┼──────────┼─────┘          │
│                                                       │          │                 │
│                                                       ▼          ▼                 │
│                                               ┌──────────┐  ┌──────────┐          │
│                                               │  Local   │  │ Engine   │          │
│                                               │  Dagger  │  │ Service  │          │
│                                               │  + Docker│  │   API    │          │
│                                               └──────────┘  └──────────┘          │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

## Step-by-Step Flow

### 1. Workflow Definition (TypeScript)

A workflow is defined in TypeScript with places, transitions, and actions:

```typescript
// examples/dagger-ci/workflow.ts
const workflow = {
  version: "1.0",
  name: "dagger-ci-pipeline",
  
  // Workflow-level engine configuration (default for all transitions)
  engine: {
    mode: "auto",
    requirements: {
      engine_version: "0.18.0",
    },
  },

  places: [
    { id: "start", initialTokens: 1 },
    { id: "built", initialTokens: 0 },
  ],

  transitions: [
    {
      id: "build",
      inputs: [{ place: "start", weight: 1 }],
      outputs: [{ place: "built", weight: 1 }],
      
      // Transition-level engine override (optional)
      engine: {
        mode: "local",  // Force local for fast builds
      },
      
      action: {
        type: "dagger",
        module: "github.com/myorg/ci",
        function: "build",
        args: { target: "production" },
      },
    },
  ],
};
```

### 2. Workflow Submission

When you run `cb submit ./workflow.ts`:

1. CLI validates the workflow against the JSON Schema
2. CLI sends HTTP POST to Controller API
3. Controller stores workflow in database
4. Controller creates initial marking (tokens in places)
5. Controller publishes `WorkflowSubmitted` event to NATS

### 3. Transition Enablement

The Controller monitors the Petri net:

1. Checks which transitions are enabled (all input places have tokens)
2. For each enabled transition, publishes `TransitionEnabled` event to NATS

```
NATS Subject: cb.runs.{run_id}.transitions.{transition_id}.enabled
Payload: {
  workflow_id: "dagger-ci-pipeline",
  run_id: "run-abc123",
  transition_id: "build",
  action: { type: "dagger", module: "...", ... },
  engine: { mode: "local", ... },
  context: { tokens: [...], environment: {...} }
}
```

### 4. Runner Claims Task

A `cb-runner` instance receives the event:

1. Runner subscribes to `cb.runs.*.transitions.*.enabled`
2. Runner claims the task (work queue semantics)
3. Runner examines `action.type` to determine executor

```rust
// cb-runner/src/lib.rs
impl ActionExecutor {
    pub async fn execute(&self, action: &Action, context: ExecutionContext) -> Result<ExecutionResult> {
        match action {
            Action::Dagger(dagger_action) => {
                self.execute_dagger(dagger_action, &context).await
            }
            Action::Script(script_action) => {
                self.execute_script(script_action, &context).await
            }
            Action::Http(http_action) => {
                self.execute_http(http_action, &context).await
            }
            Action::Noop => Ok(ExecutionResult::default()),
        }
    }
}
```

### 5. Engine Selection (Dagger Actions)

For Dagger actions, the runner uses `EngineSelector`:

```rust
// cb-runner/src/lib.rs
impl ActionExecutor {
    async fn execute_dagger(
        &self,
        action: &DaggerAction,
        context: &ExecutionContext,
    ) -> Result<ExecutionResult> {
        // 1. Build engine configuration from context
        //    (merges runner default → workflow → transition configs)
        let engine_config = self.resolve_engine_config(context);
        
        // 2. Create engine selector
        let selector = EngineSelector::new(engine_config).await?;
        
        // 3. Extract requirements from action
        let requirements = context.engine_requirements.clone().unwrap_or_default();
        
        // 4. Select appropriate engine
        let engine = selector.select(&requirements).await?;
        
        tracing::info!(
            engine_type = engine.engine_type(),
            module = %action.module,
            function = action.function.as_deref().unwrap_or("default"),
            "Executing Dagger action"
        );
        
        // 5. Execute via selected engine
        let output = engine.executor().execute(
            &action.module,
            action.function.as_deref(),
            action.args.as_ref(),
        ).await?;
        
        // 6. Convert to ExecutionResult
        Ok(ExecutionResult {
            success: true,
            outputs: output.outputs,
            duration: output.duration,
            resource_usage: output.resource_usage.map(Into::into),
            ..Default::default()
        })
    }
}
```

### 6. Engine Selection Algorithm

The `EngineSelector` implements this logic:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Auto Mode Selection Algorithm                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Input: EngineConfig + EngineRequirements                                   │
│                                                                             │
│  1. Check explicit mode:                                                    │
│     ├─ mode == "local" → TRY LOCAL ONLY                                    │
│     ├─ mode == "cloud" → TRY CLOUD ONLY                                    │
│     └─ mode == "auto"  → CONTINUE                                          │
│                                                                             │
│  2. Check if requirements FORCE cloud:                                      │
│     ├─ gpu == true           → SELECT CLOUD                                │
│     ├─ require_audit == true → SELECT CLOUD                                │
│     └─ else                  → CONTINUE                                    │
│                                                                             │
│  3. Check if requirements SUGGEST cloud:                                    │
│     ├─ memory_gb > 16        → PREFER CLOUD (if available)                 │
│     ├─ cpu_cores > 8         → PREFER CLOUD (if available)                 │
│     └─ else                  → CONTINUE                                    │
│                                                                             │
│  4. Try local (if prefer_local == true):                                    │
│     ├─ Dagger installed?     → YES: SELECT LOCAL                           │
│     ├─ Docker running?       → YES: SELECT LOCAL                           │
│     └─ else                  → CONTINUE                                    │
│                                                                             │
│  5. Fallback to cloud (if fallback_to_cloud == true):                       │
│     ├─ Cloud credentials?    → YES: SELECT CLOUD                           │
│     └─ else                  → ERROR: No engine available                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7. Local Engine Execution

When `LocalEngine` is selected:

```rust
// cb-runner/src/engine/local.rs
impl EngineExecutor for LocalEngine {
    async fn execute(
        &self,
        module: &str,
        function: Option<&str>,
        args: Option<&HashMap<String, serde_json::Value>>,
    ) -> EngineResult<ExecutionOutput> {
        // 1. Ensure local environment is ready
        let _env = self.ensure_environment().await?;
        
        // 2. Connect to local Dagger via SDK
        let result = dagger_sdk::connect(|client| async move {
            // 3. Load module
            // 4. Call function with args
            // 5. Capture output
        }).await;
        
        // 6. Return result
        Ok(ExecutionOutput { ... })
    }
}
```

**What happens under the hood:**
- Dagger SDK connects to local Docker daemon
- Dagger engine container is started (if not running)
- Module is fetched from source (git, OCI, local path)
- Function is executed in a container
- Output is captured and returned

### 8. Cloud Engine Execution (Future)

When `CloudEngine` is selected (not yet implemented):

```rust
// cb-runner/src/engine/cloud.rs (future)
impl EngineExecutor for CloudEngine {
    async fn execute(
        &self,
        module: &str,
        function: Option<&str>,
        args: Option<&HashMap<String, serde_json::Value>>,
    ) -> EngineResult<ExecutionOutput> {
        // 1. Request engine from Engine Service
        let engine_spec = self.client
            .post("/v1/engines")
            .json(&EngineRequest {
                module: module.to_string(),
                function: function.map(String::from),
                args: args.cloned(),
                ..Default::default()
            })
            .send()
            .await?;
        
        // 2. Establish mTLS connection using returned certificate
        let connection = self.connect_mtls(&engine_spec.certificate).await?;
        
        // 3. Execute via GraphQL
        let result = connection
            .query(module, function, args)
            .await?;
        
        // 4. Release engine
        self.client.delete(&format!("/v1/engines/{}", engine_spec.engine_id)).await?;
        
        Ok(result)
    }
}
```

### 9. Result Handling

After execution completes:

1. Runner evaluates policy gate (if configured)
2. Runner publishes result event to NATS:
   - Success: `TransitionCompleted`
   - Failure: `TransitionFailed`
3. Controller receives event
4. Controller updates Petri net marking
5. Controller checks for newly enabled transitions
6. Cycle continues until workflow completes

## Configuration Hierarchy

Engine configuration is resolved in priority order:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Configuration Resolution Order                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LOWEST PRIORITY                                                            │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  1. Runner Default (cb-runner.toml)                                 │   │
│  │     [engine]                                                        │   │
│  │     mode = "auto"                                                   │   │
│  │                                                                     │   │
│  │     [engine.auto]                                                   │   │
│  │     prefer_local = true                                             │   │
│  │     cloud_memory_threshold_gb = 16                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              ▲                                              │
│                              │ overridden by                                │
│                              │                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  2. Workflow Level (workflow.ts → engine field)                     │   │
│  │     engine: {                                                       │   │
│  │       mode: "auto",                                                 │   │
│  │       requirements: { engine_version: "0.18.0" }                    │   │
│  │     }                                                               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              ▲                                              │
│                              │ overridden by                                │
│                              │                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  3. Transition Level (transition → engine field)                    │   │
│  │     engine: {                                                       │   │
│  │       mode: "cloud",                                                │   │
│  │       requirements: { require_audit: true }                         │   │
│  │     }                                                               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│  HIGHEST PRIORITY                                                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Environment Variables

Engine behavior can be overridden via environment variables:

| Variable | Description | Example |
|----------|-------------|---------|
| `CB_ENGINE_MODE` | Force engine mode | `local`, `cloud`, `auto` |
| `CB_API_KEY` | Engine Service API key | `cb_xxx...` |
| `CB_ORG_ID` | Organization ID | `org_123` |
| `CB_ENGINE_FALLBACK_TO_CLOUD` | Allow cloud fallback | `true`, `false` |
| `DOCKER_HOST` | Custom Docker socket | `unix:///var/run/docker.sock` |

## Example Scenarios

### Scenario 1: Local Development

```typescript
// Developer working on a feature
engine: { mode: "local" }
```

- Uses local Dagger CLI
- Connects to local Docker Desktop
- Fast iteration, no network latency
- No cloud credentials needed

### Scenario 2: CI/CD Pipeline

```typescript
// GitHub Actions runner with Dagger installed
engine: { mode: "auto" }
```

- Auto mode detects local Dagger
- Uses local execution for most steps
- Falls back to cloud for GPU workloads

### Scenario 3: Production Deployment

```typescript
// Production deploy requiring audit
engine: {
  mode: "cloud",
  requirements: { require_audit: true }
}
```

- Forces cloud execution
- All activity logged and auditable
- Usage tracked for billing

### Scenario 4: ML Training

```typescript
// GPU-intensive ML training
engine: {
  mode: "auto",
  requirements: {
    gpu: true,
    memory_gb: 64
  }
}
```

- GPU requirement forces cloud
- Engine Service provisions GPU-enabled pod
- High memory allocated

## Related Documentation

- [Engine Service Specification](./engine-service-spec.md)
- [Engine Service Tasks](./engine-service-tasks.md)
- [Event-Driven Architecture](./event-driven-flow.md)
- [Dagger CI Example](../../examples/dagger-ci/workflow.ts)