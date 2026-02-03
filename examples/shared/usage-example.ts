/**
 * Usage Examples for Composable CI/CD Templates
 *
 * This file demonstrates how to use the reusable workflow templates
 * from ci-templates.ts to build production-ready CI/CD pipelines
 * with minimal code.
 *
 * Key benefits of this approach:
 * - DRY: Define patterns once, use everywhere
 * - Consistent: All services follow the same CI/CD patterns
 * - Flexible: Override any part of the template as needed
 * - Type-safe: Full TypeScript support with IntelliSense
 */

import { workflow } from "@circuit-breaker/core";
import { DaggerModule } from "@circuit-breaker/dagger";

// Import our reusable templates
import {
  standardPipeline,
  prPipeline,
  deployPipeline,
  withBuildAndTest,
  withDeploy,
  withNotifications,
  withSecurity,
} from "./ci-templates";

// ============================================================================
// Example 1: Quick Standard Pipeline
// ============================================================================
// The simplest way - just provide the required config

export const apiService = standardPipeline("api-service", {
  namespace: "backend",
  deploy: {
    cluster: "prod-us-east-1",
    environments: ["staging", "production"],
    strategy: "blue-green",
    requireApproval: true,
    approvalCount: 2,
  },
  notifications: {
    slack: "#api-team-deploys",
    email: ["api-team@company.com"],
  },
}).build();

// ============================================================================
// Example 2: PR Validation Pipeline
// ============================================================================
// Lightweight pipeline for pull request validation

export const apiServicePR = prPipeline("api-service-pr", {
  test: {
    unit: true,
    integration: true,
    e2e: false, // Skip e2e for PRs
    coverage: true,
  },
  security: {
    severity: "high",
  },
}).build();

// ============================================================================
// Example 3: Deploy-Only Pipeline
// ============================================================================
// For when you have pre-built artifacts

export const hotfix = deployPipeline("api-service-hotfix", {
  cluster: "prod-us-east-1",
  environments: ["production"], // Skip staging for hotfixes
  requireApproval: true,
  approvalCount: 1, // Faster approval for hotfixes
  notifications: {
    slack: "#incidents",
  },
}).build();

// ============================================================================
// Example 4: Composing Templates Manually
// ============================================================================
// For when you need more control over the pipeline structure

export const frontendService = withNotifications(
  withDeploy(
    withSecurity(
      withBuildAndTest(
        workflow("frontend-service")
          .namespace("frontend")
          .description("Frontend service CI/CD pipeline")
          .engine("auto", { engineVersion: "0.18.0" }),
        {
          build: {
            target: "production",
            platforms: ["linux/amd64"],
          },
          test: {
            unit: true,
            integration: false, // Frontend has no integration tests
            e2e: true,
            coverage: true,
          },
        }
      ),
      { severity: "medium" }
    ),
    {
      cluster: "prod-us-west-2",
      environments: ["staging", "production"],
      strategy: "canary",
    }
  ),
  { slack: "#frontend-deploys" }
).build();

// ============================================================================
// Example 5: Custom Module Integration
// ============================================================================
// Use your organization's custom Dagger modules

const myOrgCi = new DaggerModule("github.com/myorg/ci-modules");
const myOrgInfra = new DaggerModule("github.com/myorg/infra-modules");

export const customService = standardPipeline("custom-service", {
  build: {
    module: myOrgCi,
    target: "optimized",
    platforms: ["linux/amd64", "linux/arm64"],
  },
  test: {
    module: myOrgCi,
    unit: true,
    integration: true,
    integrationMemoryGb: 16, // Integration tests need more memory
  },
  deploy: {
    cluster: "prod-eu-west-1",
    module: myOrgInfra,
    strategy: "rolling",
  },
  notifications: {
    slack: "#platform-deploys",
  },
}).build();

// ============================================================================
// Example 6: ML Pipeline with GPU
// ============================================================================
// Demonstrating GPU workloads that require cloud execution

import { build, test, deploy } from "@circuit-breaker/dagger";

const mlModule = new DaggerModule("github.com/myorg/ml-modules");

export const mlTraining = workflow("ml-model-training")
  .namespace("ml")
  .description("ML model training pipeline with GPU support")
  .engine("auto", { gpu: true, memoryGb: 64 }) // Default to GPU

  .place("data-ready", { initialTokens: 1 })
  .place("validated")
  .place("trained")
  .place("evaluated")
  .place("registered")

  // Data validation - can run locally
  .transition("validate-data")
  .from("data-ready")
  .to("validated")
  .local()
  .action(mlModule.fn("validate", { schema: "./data/schema.json" }))
  .timeout("10m")
  .done()

  // Training - needs GPU
  .transition("train-model")
  .from("validated")
  .to("trained")
  .gpu(64) // 64GB GPU memory
  .action(
    mlModule.fn("train", {
      config: "./training/config.yaml",
      epochs: 100,
      checkpoint: true,
    })
  )
  .timeout("8h")
  .retries(1)
  .done()

  // Evaluation - needs GPU but less memory
  .transition("evaluate-model")
  .from("trained")
  .to("evaluated")
  .gpu(32)
  .action(
    mlModule.fn("evaluate", {
      metrics: ["accuracy", "f1", "auc"],
      threshold: 0.95,
    })
  )
  .policy("./policies/model-quality")
  .timeout("2h")
  .done()

  // Model registration - audit required
  .transition("register-model")
  .from("evaluated")
  .to("registered")
  .guard("ctx.metrics.accuracy >= 0.95")
  .audit()
  .action(
    mlModule.fn("register", {
      registry: "models.myorg.com",
      tags: ["production", "${ctx.version}"],
    })
  )
  .timeout("30m")
  .done()

  .build();

// ============================================================================
// Example 7: Multi-Service Monorepo
// ============================================================================
// Generate pipelines for multiple services programmatically

const services = [
  { name: "users-api", namespace: "auth", cluster: "prod-us-east-1" },
  { name: "orders-api", namespace: "commerce", cluster: "prod-us-east-1" },
  { name: "payments-api", namespace: "commerce", cluster: "prod-eu-west-1" },
  { name: "notifications-api", namespace: "platform", cluster: "prod-us-west-2" },
];

export const monorepoWorkflows = services.map((svc) =>
  standardPipeline(svc.name, {
    namespace: svc.namespace,
    build: {
      context: `./services/${svc.name}`,
      dockerfile: `./services/${svc.name}/Dockerfile`,
    },
    test: {
      unit: true,
      integration: true,
      coverage: true,
    },
    deploy: {
      cluster: svc.cluster,
      environments: ["staging", "production"],
      requireApproval: true,
    },
    notifications: {
      slack: `#${svc.namespace}-deploys`,
    },
  }).build()
);

// ============================================================================
// Example 8: Environment-Specific Pipelines
// ============================================================================
// Generate different pipelines based on environment

type Environment = "development" | "staging" | "production";

function createPipeline(serviceName: string, env: Environment) {
  const baseWorkflow = workflow(`${serviceName}-${env}`)
    .namespace(env)
    .description(`${serviceName} pipeline for ${env}`);

  switch (env) {
    case "development":
      // Dev: local only, fast feedback
      return withBuildAndTest(baseWorkflow.localOnly(), {
        test: { unit: true, integration: false },
      }).build();

    case "staging":
      // Staging: full tests, auto-deploy
      return withDeploy(
        withSecurity(
          withBuildAndTest(baseWorkflow.engine("auto"), {
            test: { unit: true, integration: true, e2e: true },
          })
        ),
        {
          cluster: "staging-cluster",
          environments: ["staging"],
          requireApproval: false,
        }
      ).build();

    case "production":
      // Production: everything, approval required, cloud audit
      return withNotifications(
        withDeploy(
          withSecurity(
            withBuildAndTest(baseWorkflow.engine("auto")),
            { severity: "high" }
          ),
          {
            cluster: "prod-cluster",
            environments: ["staging", "production"],
            requireApproval: true,
            approvalCount: 2,
            strategy: "blue-green",
          }
        ),
        { slack: "#prod-deploys" }
      ).build();
  }
}

export const devPipeline = createPipeline("my-service", "development");
export const stagingPipeline = createPipeline("my-service", "staging");
export const prodPipeline = createPipeline("my-service", "production");

// ============================================================================
// Default Export
// ============================================================================

export default apiService;

/**
 * CLI Usage:
 *
 *   # Run the default export (apiService)
 *   cb run ./usage-example.ts
 *
 *   # Run a specific named export
 *   cb run ./usage-example.ts --export mlTraining
 *   cb run ./usage-example.ts --export apiServicePR
 *
 *   # Validate all exports
 *   cb validate ./usage-example.ts --all
 *
 *   # Visualize pipeline
 *   cb visualize ./usage-example.ts --export frontendService
 */
