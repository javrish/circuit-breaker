# Circuit Breaker Engine Service Specification

## Overview

The **Engine Service** is a managed service that provisions and manages Dagger Engine instances for Circuit Breaker workflow execution. It enables hybrid cloud deployments where runners can execute locally or remotely while engines run in Circuit Breaker's managed infrastructure.

This design is inspired by Dagger Cloud's remote execution model, adapted for Circuit Breaker's Petri net orchestration and policy-gated pipelines.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Circuit Breaker Engine Service Architecture               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     cb-runner (User Environment)                     │   │
│  │                                                                      │   │
│  │  • Local development machine                                         │   │
│  │  • Self-hosted Kubernetes cluster                                    │   │
│  │  • CI/CD runner (GitHub Actions, GitLab, etc.)                      │   │
│  │  • Circuit Breaker managed runner                                    │   │
│  └──────────────────────────────┬──────────────────────────────────────┘   │
│                                 │                                           │
│                                 │ 1. POST /v1/engines                       │
│                                 │    (request engine for module/function)   │
│                                 ▼                                           │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     cb-engine-service (Control Plane)                │   │
│  │                                                                      │   │
│  │  • Authenticates requests (API key, OIDC, mTLS)                     │   │
│  │  • Provisions Dagger Engine pods on-demand                          │   │
│  │  • Manages engine pool (warm instances, autoscaling)                │   │
│  │  • Generates per-session mTLS certificates                          │   │
│  │  • Tracks usage, billing, quotas                                    │   │
│  └──────────────────────────────┬──────────────────────────────────────┘   │
│                                 │                                           │
│                                 │ 2. Provision engine pod                   │
│                                 │    (or assign from warm pool)             │
│                                 ▼                                           │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     Kubernetes Cluster (Data Plane)                  │   │
│  │                                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │   │
│  │  │   Engine    │  │   Engine    │  │   Engine    │   ...           │   │
│  │  │   Pod A     │  │   Pod B     │  │   Pod C     │                 │   │
│  │  │             │  │             │  │             │                 │   │
│  │  │ Dagger      │  │ Dagger      │  │ Dagger      │                 │   │
│  │  │ Engine      │  │ Engine      │  │ Engine      │                 │   │
│  │  │ v0.18.0     │  │ v0.18.0     │  │ v0.18.0     │                 │   │
│  │  └──────┬──────┘  └─────────────┘  └─────────────┘                 │   │
│  │         │                                                           │   │
│  │         │ Karpenter autoscaling                                     │   │
│  │         │ Scale-to-zero when idle                                   │   │
│  └─────────┼───────────────────────────────────────────────────────────┘   │
│            │                                                               │
│            │ 3. mTLS connection                                            │
│            │    (GraphQL API)                                              │
│            ▼                                                               │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     cb-runner executes pipeline                      │   │
│  │                                                                      │   │
│  │  • Loads module from source (git, OCI, local)                       │   │
│  │  • Calls function via GraphQL                                       │   │
│  │  • Streams logs/output back                                         │   │
│  │  • Validates output with policy gate (conftest)                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## API Specification

### Base URL

```
Production: https://engines.circuitbreaker.io
Staging:    https://engines.staging.circuitbreaker.io
Local:      http://localhost:9090
```

### Authentication

All API requests require authentication via one of:

1. **API Key** (Header: `Authorization: Bearer <api-key>`)
2. **OIDC Token** (Header: `Authorization: Bearer <oidc-token>`)
3. **mTLS Client Certificate** (for runner-to-service communication)

Organization context is provided via header:
```
X-CB-Organization: <org-id>
```

---

### Endpoints

#### `POST /v1/engines`

Request a Dagger Engine instance for pipeline execution.

**Request:**

```json
{
  "module": "github.com/org/repo/pipelines/trivy",
  "function": "scan",
  "args": {
    "target": "/src",
    "severity": "HIGH,CRITICAL"
  },
  "client_id": "runner-abc123",
  "runner_id": "runner-pool-default-xyz",
  "workflow_id": "wf-123",
  "run_id": "run-456",
  "transition_id": "security-scan",
  "minimum_engine_version": "v0.18.0",
  "resource_requirements": {
    "cpu": "2",
    "memory": "4Gi",
    "gpu": false
  },
  "timeout_seconds": 600,
  "trace_context": {
    "trace_id": "abc123def456",
    "span_id": "789ghi",
    "trace_flags": "01"
  }
}
```

**Request Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `module` | string | Yes | Dagger module reference (git URL, OCI, local path) |
| `function` | string | Yes | Function to call in the module |
| `args` | object | No | Arguments to pass to the function |
| `client_id` | string | Yes | Unique identifier for this client session |
| `runner_id` | string | No | Identifier of the runner making the request |
| `workflow_id` | string | No | Circuit Breaker workflow ID (for tracking) |
| `run_id` | string | No | Circuit Breaker run ID (for tracking) |
| `transition_id` | string | No | Transition being executed (for tracking) |
| `minimum_engine_version` | string | No | Minimum Dagger Engine version required |
| `resource_requirements` | object | No | Resource requests for the engine pod |
| `timeout_seconds` | integer | No | Maximum execution time (default: 300) |
| `trace_context` | object | No | OpenTelemetry trace context for distributed tracing |

**Response (201 Created):**

```json
{
  "engine_id": "engine-abc123",
  "url": "engine-abc123.engines.circuitbreaker.io:443",
  "instance_id": "i-abc123def456",
  "organization_id": "org-xyz",
  "user_id": "user-123",
  "certificate": {
    "certificate_chain": ["<base64-der-cert>", "<base64-der-intermediate>"],
    "private_key": "<base64-pkcs8-key>",
    "expires_at": "2024-01-15T12:00:00Z"
  },
  "engine_version": "v0.18.0",
  "status": "ready",
  "created_at": "2024-01-15T11:00:00Z",
  "expires_at": "2024-01-15T11:10:00Z",
  "metadata": {
    "region": "us-west-2",
    "node_type": "c6i.xlarge",
    "warm_start": true
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `engine_id` | string | Unique identifier for this engine instance |
| `url` | string | Host:port to connect to via mTLS |
| `instance_id` | string | Internal instance identifier |
| `certificate` | object | mTLS certificate for authentication |
| `certificate.certificate_chain` | array | DER-encoded certificate chain (base64) |
| `certificate.private_key` | string | PKCS#8-encoded private key (base64) |
| `certificate.expires_at` | string | Certificate expiration (ISO 8601) |
| `engine_version` | string | Dagger Engine version provisioned |
| `status` | string | Engine status: `provisioning`, `ready`, `error` |
| `created_at` | string | When the engine was provisioned |
| `expires_at` | string | When the engine will be terminated if unused |
| `metadata` | object | Additional metadata about the engine |

**Error Responses:**

```json
// 400 Bad Request
{
  "error": "invalid_request",
  "message": "module is required",
  "details": {
    "field": "module",
    "reason": "required"
  }
}

// 401 Unauthorized
{
  "error": "unauthorized",
  "message": "Invalid or expired API key"
}

// 403 Forbidden
{
  "error": "forbidden",
  "message": "Organization quota exceeded",
  "details": {
    "quota_type": "concurrent_engines",
    "limit": 10,
    "current": 10
  }
}

// 429 Too Many Requests
{
  "error": "rate_limited",
  "message": "Too many engine requests",
  "retry_after": 30
}

// 503 Service Unavailable
{
  "error": "capacity_exceeded",
  "message": "No capacity available in region",
  "details": {
    "region": "us-west-2",
    "estimated_wait_seconds": 60
  }
}
```

---

#### `GET /v1/engines/{engine_id}`

Get status of a provisioned engine.

**Response (200 OK):**

```json
{
  "engine_id": "engine-abc123",
  "status": "running",
  "url": "engine-abc123.engines.circuitbreaker.io:443",
  "created_at": "2024-01-15T11:00:00Z",
  "last_activity_at": "2024-01-15T11:05:00Z",
  "expires_at": "2024-01-15T11:15:00Z",
  "resource_usage": {
    "cpu_seconds": 45.2,
    "memory_peak_bytes": 2147483648,
    "network_rx_bytes": 104857600,
    "network_tx_bytes": 52428800
  }
}
```

---

#### `DELETE /v1/engines/{engine_id}`

Terminate an engine instance early.

**Response (204 No Content)**

---

#### `POST /v1/engines/{engine_id}/keepalive`

Extend the lifetime of an engine instance.

**Request:**

```json
{
  "extend_seconds": 300
}
```

**Response (200 OK):**

```json
{
  "engine_id": "engine-abc123",
  "expires_at": "2024-01-15T11:20:00Z"
}
```

---

#### `GET /v1/engines`

List active engines for the organization.

**Query Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `status` | string | Filter by status: `provisioning`, `ready`, `running`, `terminating` |
| `runner_id` | string | Filter by runner |
| `workflow_id` | string | Filter by workflow |
| `limit` | integer | Max results (default: 50) |
| `cursor` | string | Pagination cursor |

**Response (200 OK):**

```json
{
  "engines": [
    {
      "engine_id": "engine-abc123",
      "status": "running",
      "module": "github.com/org/repo/pipelines/trivy",
      "function": "scan",
      "runner_id": "runner-xyz",
      "workflow_id": "wf-123",
      "created_at": "2024-01-15T11:00:00Z"
    }
  ],
  "next_cursor": "eyJsYXN0X2lkIjoiZW5naW5lLWFiYzEyMyJ9"
}
```

---

## Connection Protocol

### Step 1: Request Engine

```rust
let response = http_client
    .post("https://engines.circuitbreaker.io/v1/engines")
    .header("Authorization", format!("Bearer {}", api_key))
    .header("X-CB-Organization", org_id)
    .json(&EngineRequest {
        module: "./pipelines/trivy".to_string(),
        function: "scan".to_string(),
        client_id: uuid::Uuid::new_v4().to_string(),
        ..Default::default()
    })
    .send()
    .await?;

let engine_spec: EngineSpec = response.json().await?;
```

### Step 2: Establish mTLS Connection

```rust
use rustls::{Certificate, PrivateKey, ClientConfig};

// Parse certificate from response
let cert_chain: Vec<Certificate> = engine_spec
    .certificate
    .certificate_chain
    .iter()
    .map(|c| Certificate(base64::decode(c).unwrap()))
    .collect();

let private_key = PrivateKey(
    base64::decode(&engine_spec.certificate.private_key).unwrap()
);

// Build TLS config with client certificate
let tls_config = ClientConfig::builder()
    .with_safe_defaults()
    .with_root_certificates(root_store)
    .with_client_auth_cert(cert_chain, private_key)?;

// Connect to engine
let connector = TlsConnector::from(Arc::new(tls_config));
let stream = TcpStream::connect(&engine_spec.url).await?;
let tls_stream = connector.connect(server_name, stream).await?;
```

### Step 3: Execute via GraphQL

Once connected via mTLS, the client uses the standard Dagger GraphQL API:

```graphql
# Load module and call function
query {
  moduleSource(refString: "github.com/org/repo/pipelines/trivy") {
    asModule {
      # Module is now loaded and functions are available
      # The exact query depends on the module's schema
    }
  }
}

# Or use container operations directly
query {
  container {
    from(address: "alpine:latest") {
      withExec(args: ["echo", "hello"]) {
        stdout
      }
    }
  }
}
```

---

## Engine Lifecycle

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Engine Lifecycle States                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Request received                                                           │
│       │                                                                     │
│       ▼                                                                     │
│  ┌─────────────┐                                                           │
│  │ PROVISIONING│ ──────────────────────────────────────────┐               │
│  └──────┬──────┘                                           │               │
│         │                                                  │               │
│         │ Pod scheduled                                    │ Timeout       │
│         │ Engine started                                   │ or error      │
│         ▼                                                  ▼               │
│  ┌─────────────┐                                    ┌─────────────┐       │
│  │    READY    │                                    │    ERROR    │       │
│  └──────┬──────┘                                    └─────────────┘       │
│         │                                                                  │
│         │ Client connects                                                  │
│         ▼                                                                  │
│  ┌─────────────┐                                                          │
│  │   RUNNING   │ ◀─────────────────────────────────────────┐              │
│  └──────┬──────┘                                           │              │
│         │                                                  │              │
│         ├── Client activity ─────────────────────────────▶─┘              │
│         │   (resets idle timer)                                           │
│         │                                                                  │
│         ├── Idle timeout                                                   │
│         │   (no activity for N seconds)                                   │
│         │                                                                  │
│         ├── Explicit termination                                          │
│         │   (DELETE /v1/engines/{id})                                     │
│         │                                                                  │
│         ├── Max lifetime reached                                          │
│         │   (hard limit, e.g., 1 hour)                                    │
│         │                                                                  │
│         ▼                                                                  │
│  ┌─────────────┐                                                          │
│  │ TERMINATING │                                                          │
│  └──────┬──────┘                                                          │
│         │                                                                  │
│         │ Pod deleted                                                      │
│         │ Resources released                                               │
│         ▼                                                                  │
│  ┌─────────────┐                                                          │
│  │ TERMINATED  │                                                          │
│  └─────────────┘                                                          │
│                                                                            │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Engine Pool Management

### Warm Pool

To reduce cold-start latency, the engine service maintains a pool of pre-provisioned engines:

```yaml
# Engine pool configuration
pool:
  name: default
  min_warm_instances: 2
  max_warm_instances: 10
  warm_instance_ttl: 300s  # 5 minutes
  
  # Engine configuration
  engine:
    version: v0.18.0
    resources:
      cpu: "2"
      memory: "4Gi"
    
  # Autoscaling
  autoscaling:
    enabled: true
    target_utilization: 0.7
    scale_up_cooldown: 30s
    scale_down_cooldown: 300s
```

### Pool Assignment Algorithm

```
1. Request arrives for module M, function F
2. Check warm pool for compatible engine:
   a. Engine version matches minimum requirement
   b. Resource requirements satisfied
   c. Engine is in READY state
3. If warm engine available:
   a. Assign to client
   b. Transition to RUNNING
   c. Generate session certificate
   d. Return connection details
4. If no warm engine:
   a. Check capacity quota
   b. Provision new engine pod
   c. Wait for READY state
   d. Return connection details
5. Replenish warm pool if below minimum
```

---

## Security Model

### Authentication Flow

```
┌──────────┐         ┌─────────────────┐         ┌─────────────┐
│ cb-runner│         │cb-engine-service│         │ Dagger      │
│          │         │                 │         │ Engine Pod  │
└────┬─────┘         └────────┬────────┘         └──────┬──────┘
     │                        │                         │
     │ 1. POST /v1/engines    │                         │
     │    + API Key           │                         │
     │───────────────────────▶│                         │
     │                        │                         │
     │                        │ 2. Validate API key     │
     │                        │    Check org quota      │
     │                        │    Check permissions    │
     │                        │                         │
     │                        │ 3. Provision engine     │
     │                        │    or assign from pool  │
     │                        │─────────────────────────▶
     │                        │                         │
     │                        │ 4. Generate mTLS cert   │
     │                        │    (per-session)        │
     │                        │                         │
     │ 5. Return engine URL   │                         │
     │    + mTLS certificate  │                         │
     │◀───────────────────────│                         │
     │                        │                         │
     │ 6. Connect via mTLS    │                         │
     │─────────────────────────────────────────────────▶│
     │                        │                         │
     │ 7. GraphQL queries     │                         │
     │◀────────────────────────────────────────────────▶│
     │                        │                         │
```

### Certificate Lifecycle

- **Duration**: Certificates are valid for the engine's lifetime (max 1 hour)
- **Scope**: Each certificate is bound to a specific engine instance
- **Revocation**: Certificates are implicitly revoked when engine terminates
- **Rotation**: Clients must request a new engine (and cert) for extended sessions

### Network Security

```yaml
# Engine pod network policy
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: engine-pod-policy
spec:
  podSelector:
    matchLabels:
      app: dagger-engine
  policyTypes:
    - Ingress
    - Egress
  ingress:
    # Only allow mTLS connections on engine port
    - ports:
        - port: 443
          protocol: TCP
  egress:
    # Allow outbound for module fetching, container pulls
    - to: []
      ports:
        - port: 443  # HTTPS
        - port: 80   # HTTP (redirects)
```

---

## Hybrid Execution Mode

The Engine Service supports three execution modes to accommodate different deployment scenarios:

### Engine Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| `local` | Use local Dagger installation | Development, CI runners with Dagger |
| `cloud` | Use Engine Service | Production, GPU workloads, managed billing |
| `auto` | Automatic selection | Default - prefers local, falls back to cloud |

### Mode Selection Logic (Auto)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Auto Mode Selection Algorithm                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. Check requirements:                                                     │
│     ├─ GPU required?           ──────────────────────▶ Use CLOUD           │
│     ├─ Audit trail required?   ──────────────────────▶ Use CLOUD           │
│     ├─ Memory > 16GB?          ──────────────────────▶ Use CLOUD           │
│     └─ CPU > 8 cores?          ──────────────────────▶ Use CLOUD           │
│                                                                             │
│  2. Check local availability:                                               │
│     ├─ Dagger installed?                                                    │
│     ├─ Container runtime running (Docker/Podman)?                           │
│     └─ If yes to both          ──────────────────────▶ Use LOCAL           │
│                                                                             │
│  3. Fallback:                                                               │
│     └─ Local unavailable + cloud configured ─────────▶ Use CLOUD           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Workflow Configuration

```yaml
# Workflow-level engine configuration
name: build-and-deploy
engine:
  mode: auto  # local, cloud, or auto (default)
  requirements:
    memory_gb: 8

transitions:
  # Simple tasks use inherited mode (auto → local)
  - name: lint
    action:
      dagger:
        module: "github.com/myorg/ci"
        function: "lint"

  # Resource-intensive tasks override to cloud
  - name: ml-training
    engine:
      mode: cloud
      requirements:
        gpu: true
        memory_gb: 64
    action:
      dagger:
        module: "github.com/myorg/ml"
        function: "train"
```

### Runner Configuration

```toml
# cb-runner.toml

[engine]
mode = "auto"  # Default mode for all workflows

[engine.local]
# Auto-detected if not specified
dagger_path = "/usr/local/bin/dagger"
runtime = "docker"  # docker, podman, or auto

[engine.cloud]
url = "https://engines.circuitbreaker.io"
api_key = "${CB_API_KEY}"
organization_id = "${CB_ORG_ID}"

[engine.auto]
prefer_local = true
fallback_to_cloud = true
cloud_memory_threshold_gb = 16
cloud_cpu_threshold_cores = 8
```

### Environment Overrides

```bash
# Force local mode (development)
CB_ENGINE_MODE=local cb-runner start

# Force cloud mode (production)
CB_ENGINE_MODE=cloud cb-runner start

# Disable cloud fallback (air-gapped environments)
CB_ENGINE_FALLBACK_TO_CLOUD=false cb-runner start
```

---

## Integration with cb-runner

### Updated DaggerAction Execution

```rust
// cb-runner/src/lib.rs

impl ActionExecutor {
    async fn execute_dagger(
        &self,
        action: &DaggerAction,
        context: &ExecutionContext,
    ) -> Result<ExecutionResult> {
        let start = std::time::Instant::now();

        // 1. Request engine from engine service
        let engine_spec = self.request_engine(&action, &context).await?;

        // 2. Connect via mTLS
        let connection = self.connect_to_engine(&engine_spec).await?;

        // 3. Execute module function via GraphQL
        let result = self.execute_module_function(
            &connection,
            &action.module,
            &action.function,
            &action.args,
        ).await;

        // 4. Release engine (or let it idle-timeout)
        if let Err(e) = self.release_engine(&engine_spec.engine_id).await {
            tracing::warn!(error = %e, "Failed to release engine");
        }

        // 5. Return result
        match result {
            Ok(output) => Ok(ExecutionResult {
                success: true,
                outputs: Some(output),
                duration: start.elapsed(),
                ..Default::default()
            }),
            Err(e) => Ok(ExecutionResult {
                success: false,
                error: Some(e.to_string()),
                duration: start.elapsed(),
                ..Default::default()
            }),
        }
    }

    async fn request_engine(
        &self,
        action: &DaggerAction,
        context: &ExecutionContext,
    ) -> Result<EngineSpec> {
        let request = EngineRequest {
            module: action.module.clone(),
            function: action.function.clone().unwrap_or_default(),
            args: action.args.clone(),
            client_id: context.task_id.to_string(),
            runner_id: Some(self.config.runner_id.clone()),
            workflow_id: Some(context.workflow_name.clone()),
            run_id: Some(context.run_id.to_string()),
            transition_id: Some(context.transition_id.clone()),
            timeout_seconds: Some(context.timeout.as_secs() as u32),
            ..Default::default()
        };

        let response = self.http_client
            .post(&format!("{}/v1/engines", self.config.engine_service_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("X-CB-Organization", &self.config.organization_id)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error: ErrorResponse = response.json().await?;
            return Err(anyhow::anyhow!("Engine request failed: {}", error.message));
        }

        response.json().await.map_err(Into::into)
    }
}
```

### Runner Configuration

```toml
# cb-runner configuration
[runner]
id = "runner-abc123"
pool = "default"

[engine_service]
url = "https://engines.circuitbreaker.io"
api_key = "${CB_ENGINE_API_KEY}"
organization_id = "${CB_ORGANIZATION_ID}"

# Connection settings
connect_timeout_seconds = 30
request_timeout_seconds = 600

# Retry settings
max_retries = 3
retry_backoff_seconds = 5
```

---

## Observability

### Metrics

The engine service exposes Prometheus metrics:

```
# Engine provisioning
cb_engine_requests_total{status="success|error",org="..."}
cb_engine_provision_duration_seconds{quantile="0.5|0.9|0.99"}
cb_engine_queue_depth{org="..."}

# Engine pool
cb_engine_pool_size{pool="...",state="warm|running|terminating"}
cb_engine_pool_utilization{pool="..."}

# Resource usage
cb_engine_cpu_seconds_total{engine_id="..."}
cb_engine_memory_bytes{engine_id="...",type="peak|current"}
cb_engine_network_bytes{engine_id="...",direction="rx|tx"}
```

### Tracing

All operations are traced with OpenTelemetry:

```
cb-runner
  └── execute_dagger
      ├── request_engine (span)
      │   └── HTTP POST /v1/engines
      ├── connect_to_engine (span)
      │   └── TLS handshake
      ├── execute_module_function (span)
      │   ├── load_module
      │   └── call_function
      └── release_engine (span)
```

### Logging

Structured JSON logs for all operations:

```json
{
  "timestamp": "2024-01-15T11:00:00Z",
  "level": "info",
  "message": "Engine provisioned",
  "engine_id": "engine-abc123",
  "organization_id": "org-xyz",
  "module": "github.com/org/repo/pipelines/trivy",
  "function": "scan",
  "provision_duration_ms": 1234,
  "warm_start": true
}
```

---

## Quotas and Limits

### Organization Quotas

| Quota | Free Tier | Team | Enterprise |
|-------|-----------|------|------------|
| Concurrent engines | 2 | 10 | Custom |
| Max engine lifetime | 10 min | 1 hour | Custom |
| Engine requests/hour | 100 | 1000 | Custom |
| Total compute hours/month | 10 | 100 | Custom |

### Engine Limits

| Limit | Default | Max |
|-------|---------|-----|
| CPU | 2 cores | 16 cores |
| Memory | 4 Gi | 64 Gi |
| Execution timeout | 5 min | 1 hour |
| Idle timeout | 2 min | 10 min |
| Network egress | 10 GB | 100 GB |

---

## Implementation Phases

### Phase 1: Core Engine Service

- [ ] Engine provisioning API (`POST /v1/engines`)
- [ ] Basic Kubernetes integration (create/delete pods)
- [ ] mTLS certificate generation
- [ ] Simple round-robin engine assignment

### Phase 2: Pool Management

- [ ] Warm engine pool
- [ ] Autoscaling based on demand
- [ ] Engine status tracking
- [ ] Idle timeout and cleanup

### Phase 3: cb-runner Integration

- [ ] Update `execute_dagger` to use engine service
- [ ] mTLS connection handling
- [ ] GraphQL client for module execution
- [ ] Error handling and retries

### Phase 4: Observability

- [ ] Prometheus metrics
- [ ] OpenTelemetry tracing
- [ ] Structured logging
- [ ] Dashboard and alerts

### Phase 5: Multi-tenancy and Billing

- [ ] Organization isolation
- [ ] Usage tracking
- [ ] Quota enforcement
- [ ] Billing integration

---

## Resolved Design Decisions

The following questions have been resolved. See [engine-service-tasks.md](./engine-service-tasks.md) for detailed implementation tasks.

| Decision | Resolution | Phase |
|----------|------------|-------|
| **Module caching** | Yes - use container filesystems with per-org isolation | Phase 1 |
| **Engine affinity** | Yes - route same module to same engine via affinity map | Phase 2 |
| **Geographic distribution** | Engines run in same region as engine service (region-local) | Phase 3 |
| **Self-hosted engines** | Yes - organizations can register their own engines | Phase 4 |
| **Offline mode** | Fail with "Engine service unavailable" (future: local fallback) | Phase 5 |

### Module Caching Strategy

- Cache key: `{org_id}/{module_source_hash}/{version}`
- Storage: Container filesystem volumes mounted to engine pods
- Isolation: Per-organization, no cross-tenant cache sharing
- Eviction: LRU with configurable TTL and size limits

### Engine Affinity Algorithm

```
1. Compute affinity_key = hash(org_id, module_source)
2. Lookup preferred engine in affinity map (Redis)
3. If engine healthy → route to it
4. Else → select from pool, prefer engines with cached module
5. Store new affinity mapping with TTL (30 min default)
```

### Self-Hosted Engine Registration

Organizations can register their own Dagger engines:
1. Generate registration token via API/UI
2. Deploy engine with `CB_ENGINE_TOKEN` environment variable
3. Engine calls `POST /v1/engines/register` on startup
4. Engine appears in organization's pool, selectable via `engine_preference`

---

## References

- [Dagger Cloud Driver](https://github.com/dagger/dagger/blob/main/engine/client/drivers/cloud.go)
- [Dagger Cloud Client](https://github.com/dagger/dagger/blob/main/internal/cloud/client.go)
- [Dagger GraphQL API](https://docs.dagger.io/api)
- [Circuit Breaker Architecture](./event-driven-flow.md)
