# Dagger CI Example

A multi-step code quality pipeline using separate Dagger modules for VCS, linting, and policy evaluation.

## Architecture

```
┌─────────┐     ┌──────────────┐     ┌────────┐     ┌───────────┐     ┌──────┐
│  start  │────▶│  checked-out │────▶│ linted │────▶│ validated │────▶│ done │
└─────────┘     └──────────────┘     └────────┘     └───────────┘     └──────┘
      │                │                  │               │
      │                │                  │               │
   checkout          lint            policy-check      complete
   (vcs)           (lint)            (policy)          (noop)
```

## How It Works

This workflow is designed for **local directories**. Each step mounts the host path via Dagger's `--source` flag:

1. **checkout** - Validates the path and detects VCS type (git, atomic, unknown)
2. **lint** - Runs ESLint on the source directory
3. **policy-check** - Evaluates OPA/Rego policies against lint results

## Modules Used

| Module | Function | Purpose |
|--------|----------|---------|
| `modules/vcs` | `checkoutToPath` | Validate path and detect VCS type |
| `modules/lint` | `eslint` | Run ESLint on source code |
| `modules/policy` | `evaluate` | Evaluate OPA/Rego policies |

## Usage

### Run with a local directory

```bash
# Start the workflow
./cb run examples/dagger-ci/workflow.ts

# Inject the path to your local repo
./cb inject <run-id> start --data '{"url": "/path/to/your/repo"}'

# Watch progress
./cb status <run-id> --watch

# View logs
./cb logs <run-id>
```

### Example with hello-world project

```bash
./cb run examples/dagger-ci/workflow.ts
./cb inject <run-id> start --data '{"url": "/Users/leefaus/Projects/hello-world"}'
```

## Remote Repositories

This workflow is optimized for local directories. For remote repositories (git-https://, atomic-https://), you have two options:

1. **Clone locally first**, then run this workflow:
   ```bash
   git clone https://github.com/your-org/repo /tmp/repo
   ./cb inject <run-id> start --data '{"url": "/tmp/repo"}'
   ```

2. **Use the VCS module directly** in a single-step workflow that clones and processes in one Dagger session.

## Policies

The `policies/` directory contains OPA/Rego policies for evaluating lint results.

### Example Policy (`policies/quality.rego`)

```rego
package quality

deny[msg] {
    input.lint.errorCount > 0
    msg := sprintf("ESLint found %d errors", [input.lint.errorCount])
}

warn[msg] {
    input.lint.warningCount > 10
    msg := sprintf("ESLint found %d warnings (threshold: 10)", [input.lint.warningCount])
}
```

## Token Flow

| Place | Token Schema |
|-------|--------------|
| **start** | `{ url: "/path/to/repo" }` |
| **checked-out** | `{ vcs: "git", path: "/path/to/repo", info: {...} }` |
| **linted** | `{ vcs, path, lint: { errorCount, warningCount, ... } }` |
| **validated** | `{ vcs, path, lint, policy: { passed, failures, warnings } }` |
| **done** | (empty) |

## Development

### Initialize the Dagger modules

```bash
cd modules && ./develop.sh
```

### Test modules individually

```bash
# Test VCS detection
dagger call -m modules/vcs checkout-to-path --source=/path/to/repo

# Test lint
dagger call -m modules/lint eslint --source=/path/to/repo

# Test policy
dagger call -m modules/policy evaluate \
  --input='{"lint":{"errorCount":0,"warningCount":1}}' \
  --policies=./examples/dagger-ci/policies
```

## Customization

### Add more linters

```typescript
wf.place("biome-linted", { /* token schema */ });

wf.transition("biome")
  .from("linted")
  .to("biome-linted")
  .dagger(LINT_MODULE, "biome", {
    source: "ctx.token.path",
  })
  .done();
```

### Change policy namespace

```typescript
wf.transition("policy-check")
  .from("linted")
  .to("validated")
  .dagger(POLICY_MODULE, "evaluate", {
    input: "ctx.token.lint",
    policies: POLICIES_PATH,
    namespace: "quality",  // Only evaluate quality.* rules
  })
  .done();
```
