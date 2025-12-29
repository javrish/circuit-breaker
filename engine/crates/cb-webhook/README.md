# cb-webhook

HTTP webhook server for ingesting external events into Circuit Breaker.

## Overview

`cb-webhook` is a high-performance webhook server that receives HTTP POST requests from external services (GitHub, GitLab, Docker Hub, Stripe, etc.), validates their authenticity, normalizes them to [CloudEvents](https://cloudevents.io/) format, and publishes them to NATS for downstream processing by the Circuit Breaker workflow engine.

## Features

- **Multi-Source Support**: Built-in support for GitHub, GitLab, Docker Hub, Stripe, Slack, and generic webhooks
- **Authentication**: HMAC-SHA256, Bearer tokens, Basic auth, IP allowlists
- **CloudEvents Normalization**: Converts provider-specific payloads to standard CloudEvents format
- **Trigger Matching**: CEL-based filtering and input mapping
- **Rate Limiting**: Per-endpoint rate limiting with configurable keys
- **Observability**: Prometheus metrics, structured logging, distributed tracing
- **Hot Reload**: Dynamic endpoint configuration without restarts
- **Event Replay**: Debug and replay events for troubleshooting

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              External Services                                   │
├─────────────────────────────────────────────────────────────────────────────────┤
│   GitHub    │   GitLab    │  Docker Hub  │   Stripe    │   Custom Apps          │
└──────┬──────┴──────┬──────┴──────┬───────┴──────┬──────┴──────┬─────────────────┘
       │             │             │              │             │
       └─────────────┴──────┬──────┴──────────────┴─────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                            cb-webhook Server                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐        │
│  │   Auth       │  │  Normalizer  │  │   Trigger    │  │   NATS       │        │
│  │  Validator   │→ │ (CloudEvents)│→ │   Matcher    │→ │  Publisher   │        │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘        │
└─────────────────────────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           NATS JetStream                                         │
│                     (cb.external.webhook.*.received)                             │
└─────────────────────────────────────────────────────────────────────────────────┘
                            │
                            ▼
                 Circuit Breaker Workflow Engine
```

## Installation

```bash
# Build from source
cargo build --release -p cb-webhook

# The binary will be at target/release/cb-webhook
```

## Usage

### Starting the Server

```bash
# Basic usage with NATS
cb-webhook --nats-url nats://localhost:4222

# With configuration file
cb-webhook --config /etc/circuit-breaker/webhooks.yaml --nats-url nats://localhost:4222

# With configuration directory
cb-webhook --config /etc/circuit-breaker/webhooks.d/ --nats-url nats://localhost:4222

# Full options
cb-webhook \
  --host 0.0.0.0 \
  --port 8081 \
  --nats-url nats://localhost:4222 \
  --config /etc/circuit-breaker/webhooks.yaml \
  --metrics-enabled \
  --metrics-port 9091 \
  --log-level info \
  --log-json
```

### Command Line Options

| Option | Environment Variable | Default | Description |
|--------|---------------------|---------|-------------|
| `--host` | `CB_WEBHOOK_HOST` | `0.0.0.0` | Host to bind to |
| `--port` | `CB_WEBHOOK_PORT` | `8081` | Port to listen on |
| `--nats-url` | `NATS_URL` | `nats://localhost:4222` | NATS server URL |
| `--config` | `CB_WEBHOOK_CONFIG` | - | Path to config file or directory |
| `--metrics-enabled` | `CB_WEBHOOK_METRICS_ENABLED` | `true` | Enable Prometheus metrics |
| `--metrics-port` | `CB_WEBHOOK_METRICS_PORT` | `9091` | Metrics endpoint port |
| `--log-level` | `CB_LOG_LEVEL` | `info` | Log level (trace, debug, info, warn, error) |
| `--log-json` | `CB_LOG_JSON` | `false` | Enable JSON log format |

## Configuration

### Endpoint Configuration

Webhook endpoints are configured using YAML files that follow a Kubernetes-like CRD format:

```yaml
apiVersion: circuitbreaker.io/v1
kind: WebhookEndpoint
metadata:
  name: github-webhook
  namespace: production
spec:
  path: /webhooks/github
  
  auth:
    type: hmac-sha256
    secretRef:
      name: github-webhook-secret
      key: secret
    headerName: X-Hub-Signature-256
    signaturePrefix: "sha256="
  
  triggers:
    - event: push
      filter:
        ref: "refs/heads/main"
      workflow: build-and-deploy
      inputs:
        repository: "{{ .repository.full_name }}"
        commit: "{{ .head_commit.id }}"
        branch: "{{ .ref }}"
    
    - event: pull_request
      filter:
        action: "opened"
      workflow: pr-validation
      inputs:
        prNumber: "{{ .number }}"
        headSha: "{{ .pull_request.head.sha }}"
  
  maxPayloadBytes: 10485760
  rateLimit:
    requests: 100
    period: 1m
  
  enabled: true
```

### Authentication Types

| Type | Description | Headers |
|------|-------------|---------|
| `hmac-sha256` | HMAC-SHA256 signature validation | GitHub, Stripe |
| `hmac-sha1` | HMAC-SHA1 signature validation (legacy) | Older integrations |
| `bearer-token` | Bearer token in Authorization header | GitLab, custom |
| `basic` | Basic authentication | Custom integrations |
| `ip-allowlist` | IP-based access control | Trusted networks |

### Input Mapping

Input mappings use a template syntax to extract values from the webhook payload:

```yaml
inputs:
  # Direct path access
  repository: "{{ .repository.full_name }}"
  
  # Nested paths
  authorEmail: "{{ .head_commit.author.email }}"
  
  # Array access
  firstCommit: "{{ .commits[0].id }}"
  
  # Literal values
  environment: "production"
```

## API Endpoints

### Health & Status

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check with NATS status |
| `/ready` | GET | Readiness probe |
| `/live` | GET | Liveness probe |

### Webhook Reception

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/webhooks/{path}` | POST | Receive webhook events |

### Management API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/endpoints` | GET | List all endpoints |
| `/api/v1/endpoints` | POST | Create new endpoint |
| `/api/v1/endpoints/{id}` | GET | Get endpoint details |
| `/api/v1/endpoints/{id}` | PUT | Update endpoint |
| `/api/v1/endpoints/{id}` | DELETE | Delete endpoint |
| `/api/v1/events` | GET | List recent events |
| `/api/v1/events/{id}` | GET | Get event details |
| `/api/v1/events/{id}/replay` | POST | Replay an event |

## Metrics

The following Prometheus metrics are exposed on the metrics port:

| Metric | Type | Description |
|--------|------|-------------|
| `cb_webhook_events_received_total` | Counter | Total events received by source and type |
| `cb_webhook_events_processed_total` | Counter | Events processed by status |
| `cb_webhook_trigger_matches_total` | Counter | Trigger matches by trigger name |
| `cb_webhook_auth_failures_total` | Counter | Authentication failures by endpoint |
| `cb_webhook_payload_bytes` | Histogram | Payload size distribution |
| `cb_webhook_processing_duration_seconds` | Histogram | Processing time distribution |
| `cb_webhook_endpoints_active` | Gauge | Number of active endpoints |
| `cb_webhook_nats_publish_success_total` | Counter | Successful NATS publishes |
| `cb_webhook_nats_publish_failures_total` | Counter | Failed NATS publishes |

## NATS Subjects

Events are published to NATS JetStream with the following subject pattern:

```
cb.external.webhook.{endpoint_name}.received
```

The message payload is a CloudEvent with the following structure:

```json
{
  "specversion": "1.0",
  "id": "evt-abc123",
  "source": "github.com/myorg/myrepo",
  "type": "com.github.push",
  "subject": "refs/heads/main",
  "time": "2024-01-15T10:30:00Z",
  "datacontenttype": "application/json",
  "data": { /* original webhook payload */ },
  "circuitbreaker": {
    "workflowName": "build-and-deploy",
    "namespace": "production",
    "inputs": {
      "repository": "myorg/myrepo",
      "commit": "abc123"
    },
    "triggerName": "github/push",
    "endpointId": "production/github",
    "traceId": "evt-abc123"
  }
}
```

## Examples

See the `examples/` directory for complete configuration examples:

- `github-webhook.yaml` - GitHub webhook configuration
- `multi-source.yaml` - Multiple webhook sources in one file

## Testing Webhooks

You can test webhooks locally using curl:

```bash
# GitHub push event
curl -X POST http://localhost:8081/webhooks/github \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: push" \
  -H "X-GitHub-Delivery: test-123" \
  -H "X-Hub-Signature-256: sha256=<computed_signature>" \
  -d '{
    "ref": "refs/heads/main",
    "repository": {
      "full_name": "myorg/myrepo"
    },
    "head_commit": {
      "id": "abc123",
      "message": "Test commit"
    }
  }'
```

## Development

```bash
# Run tests
cargo test -p cb-webhook

# Run with debug logging
RUST_LOG=cb_webhook=debug cargo run -p cb-webhook -- --nats-url nats://localhost:4222

# Check for issues
cargo clippy -p cb-webhook
```

## License

MIT License - see the [LICENSE](../../../../LICENSE) file for details.