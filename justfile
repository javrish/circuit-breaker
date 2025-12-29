# Circuit Breaker - Task Runner
# https://github.com/casey/just

set dotenv-load := true

# Default recipe - show available commands
default:
    @just --list

# ============ Setup ============

# Install all dependencies
setup: setup-rust setup-bun setup-tools

# Setup Rust toolchain
setup-rust:
    rustup update stable
    rustup component add clippy rustfmt
    cd engine && cargo fetch

# Setup Bun and install dependencies
setup-bun:
    cd sdk && bun install

# Install development tools
setup-tools:
    cargo install cargo-watch cargo-nextest

# ============ Development ============

# Run the full development stack
dev: dev-nats
    @echo "Starting development environment..."
    just dev-engine &
    just dev-api &
    wait

# Start NATS server (requires docker)
dev-nats:
    docker run -d --name cb-nats -p 4222:4222 -p 8222:8222 nats:latest -js || true

# Stop NATS server
stop-nats:
    docker stop cb-nats && docker rm cb-nats || true

# Watch and rebuild Rust engine
dev-engine:
    cd engine && cargo watch -x 'run --bin cb-engine'

# Watch and rebuild API server
dev-api:
    cd engine && cargo watch -x 'run --bin cb-api'

# ============ Build ============

# Build everything
build: build-engine build-sdk

# Build Rust engine (release)
build-engine:
    cd engine && cargo build --release

# Build TypeScript SDK
build-sdk:
    cd sdk && bun run build

# Build Docker images
build-docker:
    docker build -t circuit-breaker/engine:latest -f docker/engine.Dockerfile .
    docker build -t circuit-breaker/runner:latest -f docker/runner.Dockerfile .

# ============ Test ============

# Run all tests
test: test-engine test-sdk

# Run Rust tests
test-engine:
    cd engine && cargo nextest run

# Run Rust tests with coverage
test-engine-cov:
    cd engine && cargo llvm-cov nextest

# Run TypeScript tests
test-sdk:
    cd sdk && bun test

# ============ Lint & Format ============

# Lint and format everything
lint: lint-engine lint-sdk

# Lint Rust code
lint-engine:
    cd engine && cargo fmt --check
    cd engine && cargo clippy -- -D warnings

# Lint TypeScript code
lint-sdk:
    cd sdk && bun run lint

# Format all code
fmt: fmt-engine fmt-sdk

# Format Rust code
fmt-engine:
    cd engine && cargo fmt

# Format TypeScript code
fmt-sdk:
    cd sdk && bun run fmt

# ============ Schema ============

# Generate types from JSON schemas
schema-gen: schema-gen-rust schema-gen-ts

# Generate Rust types from JSON schemas
schema-gen-rust:
    cd engine && cargo run --bin schema-gen

# Generate TypeScript types from JSON schemas
schema-gen-ts:
    cd sdk && bun run schema:gen

# Validate all JSON schemas
schema-validate:
    cd schemas && npx ajv compile -s "*.schema.json"

# ============ Kubernetes ============

# Install CRDs
k8s-crds:
    kubectl apply -f k8s/crds/

# Deploy to local cluster (kind/minikube)
k8s-local: k8s-crds
    kubectl apply -f k8s/local/

# Deploy Karpenter configuration
k8s-karpenter:
    kubectl apply -f k8s/karpenter/

# ============ CLI ============

# Run CLI command
cli *args:
    cd sdk && bun run cli {{args}}

# Submit example workflow
example-submit:
    cd sdk && bun run cli submit ../examples/ci-pipeline/workflow.ts

# ============ Docs ============

# Generate documentation
docs:
    cd engine && cargo doc --no-deps --open
    cd sdk && bun run docs

# ============ Clean ============

# Clean all build artifacts
clean: clean-engine clean-sdk

# Clean Rust artifacts
clean-engine:
    cd engine && cargo clean

# Clean TypeScript artifacts
clean-sdk:
    cd sdk && rm -rf node_modules packages/*/dist packages/*/.turbo

# ============ Release ============

# Create a release build
release: test lint build
    @echo "Release build complete"

# Publish SDK packages
publish-sdk:
    cd sdk && bun run publish

# ============ CI ============

# Run CI checks (used in GitHub Actions)
ci: lint test build
    @echo "CI checks passed"
