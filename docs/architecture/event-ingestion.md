# Circuit Breaker: Event Ingestion Architecture

This document describes how Circuit Breaker ingests events from various sources beyond the CLI, enabling true event-driven workflow orchestration.

## Overview

In production environments, workflows are rarely triggered by humans typing CLI commands. Instead, they're triggered by **events from other systems**:

- Git pushes and pull requests
- Container registry image pushes
- Message queue events (Kafka, RabbitMQ, SQS)
- Webhook callbacks
- Scheduled triggers (cron)
- Database change events (CDC)
- Cloud provider events (S3, CloudWatch, Pub/Sub)
- Custom application events

Circuit Breaker acts as an **event sink** that translates these external events into workflow executions.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              Event Sources                                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐│
│  │  GitHub  │  │  GitLab  │  │  Kafka   │  │   AWS    │  │  Custom  │  │   CLI    ││
│  │ Webhooks │  │ Webhooks │  │  Topics  │  │  Events  │  │   Apps   │  │   SDK    ││
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘│
│       │             │             │             │             │             │       │
└───────┼─────────────┼─────────────┼─────────────┼─────────────┼─────────────┼───────┘
        │             │             │             │             │             │
        ▼             ▼             ▼             ▼             ▼             ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           Event Ingestion Layer                                      │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │  Webhook Server │  │  Kafka Consumer │  │  Cloud Adapter  │  │   REST API      │ │
│  │   (cb-webhook)  │  │  (cb-kafka)     │  │  (cb-cloud)     │  │   (cb-api)      │ │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘ │
│           │                    │                    │                    │          │
│           └────────────────────┴────────────────────┴────────────────────┘          │
│                                         │                                            │
│                                         ▼                                            │
│                          ┌──────────────────────────────┐                           │
│                          │      Event Normalizer        │                           │
│                          │   (CloudEvents format)       │                           │
│                          └──────────────┬───────────────┘                           │
│                                         │                                            │
│                                         ▼                                            │
│                          ┌──────────────────────────────┐                           │
│                          │     Trigger Matcher          │                           │
│                          │  (Event → Workflow mapping)  │                           │
│                          └──────────────┬───────────────┘                           │
│                                         │                                            │
└─────────────────────────────────────────┼───────────────────────────────────────────┘
                                          │
                                          ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              NATS JetStream                                          │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│    ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐             │
│    │  EXTERNAL_EVENTS│     │    WORKFLOWS    │     │      RUNS       │             │
│    │     Stream      │     │     Stream      │     │     Stream      │             │
│    └─────────────────┘     └─────────────────┘     └─────────────────┘             │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
                                          │
                                          ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           Circuit Breaker Core                                       │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│         ┌────────────┐      ┌────────────┐      ┌────────────┐                      │
│         │ Controller │      │ Scheduler  │      │  Runners   │                      │
│         └────────────┘      └────────────┘      └────────────┘                      │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

## Event Ingestion Components

### 1. Webhook Server (`cb-webhook`)

Receives HTTP webhooks from external services and converts them to internal events.

```yaml
# Webhook endpoint configuration
apiVersion: circuitbreaker.io/v1
kind: WebhookEndpoint
metadata:
  name: github-webhook
  namespace: production
spec:
  # Authentication
  auth:
    type: hmac-sha256
    secretRef:
      name: github-webhook-secret
      key: secret
  
  # Event mapping
  triggers:
    - event: push
      filter:
        ref: "refs/heads/main"
      workflow: build-and-deploy
      inputs:
        repository: "{{ .repository.full_name }}"
        commit: "{{ .head_commit.id }}"
        branch: "{{ .ref | trimPrefix \"refs/heads/\" }}"
    
    - event: pull_request
      filter:
        action: ["opened", "synchronize"]
      workflow: pr-validation
      inputs:
        prNumber: "{{ .number }}"
        headSha: "{{ .pull_request.head.sha }}"
```

### 2. Message Queue Consumers

#### Kafka Consumer (`cb-kafka`)

```yaml
apiVersion: circuitbreaker.io/v1
kind: KafkaTrigger
metadata:
  name: order-events
spec:
  # Kafka connection
  bootstrap:
    servers:
      - kafka-1.example.com:9092
      - kafka-2.example.com:9092
    auth:
      mechanism: SASL_SSL
      secretRef: kafka-credentials
  
  # Topic subscription
  topics:
    - name: orders.created
      consumerGroup: circuit-breaker-orders
      
  # Event to workflow mapping
  triggers:
    - filter:
        jsonPath: "$.eventType"
        equals: "ORDER_CREATED"
      workflow: order-fulfillment
      inputs:
        orderId: "{{ .orderId }}"
        customerId: "{{ .customerId }}"
        items: "{{ .items | toJson }}"
```

#### AWS EventBridge / SQS (`cb-aws`)

```yaml
apiVersion: circuitbreaker.io/v1
kind: AWSTrigger
metadata:
  name: s3-uploads
spec:
  # AWS configuration
  region: us-west-2
  roleArn: arn:aws:iam::123456789:role/circuit-breaker
  
  # Event source
  source:
    type: sqs
    queueUrl: https://sqs.us-west-2.amazonaws.com/123456789/file-uploads
    
  # S3 event handling
  triggers:
    - filter:
        source: "aws.s3"
        detailType: "Object Created"
        detail:
          bucket:
            name: ["uploads-bucket"]
          object:
            key:
              prefix: "incoming/"
      workflow: process-upload
      inputs:
        bucket: "{{ .detail.bucket.name }}"
        key: "{{ .detail.object.key }}"
        size: "{{ .detail.object.size }}"
```

### 3. CloudEvents Normalization

All incoming events are normalized to [CloudEvents](https://cloudevents.io/) format:

```json
{
  "specversion": "1.0",
  "id": "evt-abc123",
  "source": "github.com/myorg/myrepo",
  "type": "com.github.push",
  "subject": "refs/heads/main",
  "time": "2024-01-15T10:30:00Z",
  "datacontenttype": "application/json",
  "data": {
    "repository": "myorg/myrepo",
    "commit": "abc123def",
    "branch": "main",
    "author": "developer@example.com"
  },
  "circuitbreaker": {
    "workflowName": "build-and-deploy",
    "namespace": "production",
    "inputs": {
      "repository": "myorg/myrepo",
      "commit": "abc123def"
    }
  }
}
```

## Trigger Configuration

### Trigger Custom Resource Definition

```yaml
apiVersion: circuitbreaker.io/v1
kind: Trigger
metadata:
  name: deploy-on-image-push
  namespace: production
spec:
  # Event source selector
  source:
    type: webhook
    name: docker-hub
    
  # Event filter (CEL expression)
  filter: |
    event.type == "image.push" &&
    event.data.repository.startsWith("myorg/") &&
    event.data.tag != "latest"
  
  # Workflow to trigger
  workflow:
    name: deploy-service
    namespace: production
    
  # Input mapping (CEL expressions)
  inputs:
    image: "event.data.repository + ':' + event.data.tag"
    digest: "event.data.digest"
    environment: |
      event.data.tag.contains("-rc") ? "staging" : "production"
    
  # Concurrency control
  concurrency:
    # Only one run per unique key at a time
    key: "event.data.repository"
    policy: replace  # cancel, wait, or replace
    
  # Rate limiting
  rateLimit:
    requests: 10
    period: 1m
```

## Event Flow Sequence

```
┌──────────┐    ┌─────────┐    ┌───────────┐    ┌─────────┐    ┌────────────┐
│  GitHub  │    │ Webhook │    │ Normalizer│    │ Trigger │    │    NATS    │
│  Server  │    │ Server  │    │           │    │ Matcher │    │ JetStream  │
└────┬─────┘    └────┬────┘    └─────┬─────┘    └────┬────┘    └─────┬──────┘
     │               │               │               │               │
     │ POST /webhook │               │               │               │
     │──────────────▶│               │               │               │
     │               │               │               │               │
     │               │ Validate      │               │               │
     │               │ HMAC signature│               │               │
     │               │               │               │               │
     │               │ Parse event   │               │               │
     │               │──────────────▶│               │               │
     │               │               │               │               │
     │               │               │ Convert to    │               │
     │               │               │ CloudEvent    │               │
     │               │               │──────────────▶│               │
     │               │               │               │               │
     │               │               │               │ Match triggers│
     │               │               │               │ Evaluate CEL  │
     │               │               │               │               │
     │               │               │               │ Map inputs    │
     │               │               │               │               │
     │               │               │               │ WorkflowSubmitted
     │               │               │               │──────────────▶│
     │               │               │               │               │
     │   202 Accepted│               │               │               │
     │◀──────────────│               │               │               │
     │               │               │               │               │
     │               │               │               │               │ ──▶ Controller
     │               │               │               │               │ ──▶ Scheduler
     │               │               │               │               │ ──▶ Runners
```

## NATS Subjects for External Events

```
cb.
├── external.                              # External event ingestion
│   ├── webhook.{endpoint}.received        # Raw webhook events
│   ├── kafka.{topic}.received             # Raw Kafka events
│   ├── aws.{source}.received              # Raw AWS events
│   └── normalized                         # CloudEvents format
│
├── triggers.
│   ├── matched                            # Events that matched a trigger
│   ├── filtered                           # Events filtered out
│   └── errors                             # Processing errors
│
└── workflows.{namespace}.submitted        # Workflow submissions (as before)
```

## Example: Complete GitOps Pipeline

### 1. Define the Trigger

```yaml
apiVersion: circuitbreaker.io/v1
kind: Trigger
metadata:
  name: gitops-deploy
spec:
  source:
    type: webhook
    name: github
    
  filter: |
    event.type == "push" &&
    event.data.ref == "refs/heads/main" &&
    event.data.repository.full_name.startsWith("myorg/")
    
  workflow:
    name: gitops-pipeline
    
  inputs:
    repo: "event.data.repository.full_name"
    commit: "event.data.head_commit.id"
    author: "event.data.head_commit.author.email"
    message: "event.data.head_commit.message"
```

### 2. Define the Workflow

```typescript
import { workflow, task } from "@circuit-breaker/sdk";

export default workflow("gitops-pipeline")
  .namespace("production")
  .inputs({
    repo: { type: "string" },
    commit: { type: "string" },
    author: { type: "string" },
    message: { type: "string" },
  })
  .place("start", { initial: 1 })
  .place("cloned")
  .place("tested")
  .place("built")
  .place("deployed")
  .place("done")
  
  .transition("clone")
    .from("start").to("cloned")
    .task(task("git-clone").container("alpine/git")
      .run(`git clone https://github.com/\${inputs.repo} /workspace`))
  
  .transition("test")
    .from("cloned").to("tested")
    .task(task("run-tests").container("node:20")
      .run("npm ci && npm test"))
  
  .transition("build")
    .from("tested").to("built")
    .task(task("docker-build").dagger()
      .pipeline((client) => 
        client.container()
          .from("node:20")
          .withDirectory("/app", client.host().directory("."))
          .withExec(["npm", "run", "build"])
          .publish(`myregistry.io/${inputs.repo}:${inputs.commit}`)))
  
  .transition("deploy")
    .from("built").to("deployed")
    .task(task("kubectl-apply").container("bitnami/kubectl")
      .run(`kubectl set image deployment/app app=myregistry.io/\${inputs.repo}:\${inputs.commit}`))
  
  .transition("notify")
    .from("deployed").to("done")
    .task(task("slack-notify").container("curlimages/curl")
      .run(`curl -X POST $SLACK_WEBHOOK -d '{"text":"Deployed ${inputs.commit} by ${inputs.author}"}'`))
  
  .build();
```

### 3. What Happens

1. **Developer pushes to `main`** → GitHub sends webhook
2. **Webhook Server** receives POST, validates HMAC signature
3. **Normalizer** converts GitHub event to CloudEvent format
4. **Trigger Matcher** evaluates CEL filter, extracts inputs
5. **NATS** receives `WorkflowSubmitted` event
6. **Controller** initializes Petri net marking
7. **Scheduler** enables `clone` transition
8. **Runner** executes each task via Dagger
9. **Pipeline completes** → Slack notification sent

## Multi-Source Event Correlation

Circuit Breaker can correlate events from multiple sources:

```yaml
apiVersion: circuitbreaker.io/v1
kind: CorrelatedTrigger
metadata:
  name: pr-complete-pipeline
spec:
  # Wait for multiple events
  events:
    - name: pr-merged
      source: github
      filter: |
        event.type == "pull_request" &&
        event.data.action == "closed" &&
        event.data.pull_request.merged == true
        
    - name: checks-passed
      source: github
      filter: |
        event.type == "check_suite" &&
        event.data.action == "completed" &&
        event.data.check_suite.conclusion == "success"
        
    - name: security-scan-clean
      source: snyk
      filter: |
        event.data.severity == "none"
  
  # Correlation key
  correlation:
    key: "event.data.repository.full_name + ':' + event.data.pull_request.head.sha"
    timeout: 1h
    
  # All events must occur
  condition: all
  
  # Then trigger workflow
  workflow:
    name: production-deploy
    inputs:
      repo: "events.pr_merged.data.repository.full_name"
      sha: "events.pr_merged.data.pull_request.head.sha"
```

## Security Considerations

### Webhook Authentication

| Method | Use Case |
|--------|----------|
| HMAC-SHA256 | GitHub, GitLab, Stripe |
| Bearer Token | Generic webhooks |
| mTLS | High-security environments |
| IP Allowlist | Known sender IPs |

### Event Validation

```yaml
spec:
  validation:
    # JSON Schema validation
    schema:
      $ref: "#/definitions/GitHubPushEvent"
    
    # Required fields
    required:
      - repository
      - head_commit
      
    # Content size limits
    maxPayloadBytes: 1048576  # 1MB
    
    # Rate limiting per source
    rateLimit:
      requests: 100
      period: 1m
      key: "source.ip"
```

## Observability

### Metrics

```
cb_external_events_received_total{source="github",type="push"}
cb_external_events_filtered_total{source="github",reason="no_match"}
cb_trigger_matches_total{trigger="gitops-deploy"}
cb_trigger_evaluation_duration_seconds{trigger="gitops-deploy"}
cb_workflows_triggered_total{source="external",trigger="gitops-deploy"}
```

### Tracing

External events carry trace context through the entire pipeline:

```
External Event (trace_id: abc123)
  └── Webhook Received (span: webhook-receive)
      └── Event Normalized (span: normalize)
          └── Trigger Evaluated (span: trigger-match)
              └── Workflow Submitted (span: workflow-submit)
                  └── ... (workflow execution spans)
```

## Benefits of Event-Driven Ingestion

1. **Decoupled**: Source systems don't know about Circuit Breaker
2. **Polyglot**: Accept events from any system that can send HTTP/messages
3. **Reliable**: Events persisted in NATS before processing
4. **Scalable**: Horizontal scaling of ingestion layer
5. **Auditable**: Every external event logged and traceable
6. **Flexible**: Add new triggers without code changes
7. **Testable**: Replay events for debugging and testing