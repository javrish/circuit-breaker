/**
 * Reusable CI/CD Workflow Templates
 *
 * This module provides composable workflow building blocks that can be
 * shared across your organization. Instead of copying workflow definitions,
 * you compose them from these templates.
 *
 * @example
 * ```ts
 * import { workflow } from '@circuit-breaker/core';
 * import { withBuildAndTest, withDeploy, withNotifications } from './ci-templates';
 *
 * // Compose a full pipeline from templates
 * const myPipeline = withNotifications(
 *   withDeploy(
 *     withBuildAndTest(
 *       workflow('my-service')
 *     ),
 *     { cluster: 'prod-us-east-1' }
 *   ),
 *   { slack: '#my-team' }
 * ).build();
 * ```
 *
 * @module
 */

import { workflow, WorkflowBuilder, TransitionBuilder } from "@circuit-breaker/core";
import { DaggerModule, build, test, deploy, notify } from "@circuit-breaker/dagger";

// ============================================================================
// Types
// ============================================================================

/**
 * Configuration for build templates
 */
export interface BuildConfig {
  /** Build context path */
  context?: string;
  /** Dockerfile path */
  dockerfile?: string;
  /** Build target */
  target?: string;
  /** Target platforms */
  platforms?: string[];
  /** Custom Dagger module for build */
  module?: DaggerModule;
  /** Timeout for build */
  timeout?: string;
}

/**
 * Configuration for test templates
 */
export interface TestConfig {
  /** Run unit tests */
  unit?: boolean;
  /** Run integration tests */
  integration?: boolean;
  /** Run e2e tests */
  e2e?: boolean;
  /** Enable coverage */
  coverage?: boolean;
  /** Custom Dagger module for tests */
  module?: DaggerModule;
  /** Integration test memory requirement (GB) */
  integrationMemoryGb?: number;
}

/**
 * Configuration for deploy templates
 */
export interface DeployConfig {
  /** Kubernetes cluster name */
  cluster: string;
  /** Kubernetes namespace */
  namespace?: string;
  /** Environments to deploy to */
  environments?: ("staging" | "production")[];
  /** Require approval for production */
  requireApproval?: boolean;
  /** Number of approvals required */
  approvalCount?: number;
  /** Deployment strategy */
  strategy?: "rolling" | "blue-green" | "canary";
  /** Custom Dagger module for deploy */
  module?: DaggerModule;
}

/**
 * Configuration for notification templates
 */
export interface NotificationConfig {
  /** Slack channel for notifications */
  slack?: string;
  /** Email recipients */
  email?: string[];
  /** GitHub repo for issue comments */
  github?: string;
  /** Notify on success */
  onSuccess?: boolean;
  /** Notify on failure */
  onFailure?: boolean;
}

/**
 * Configuration for security scanning
 */
export interface SecurityConfig {
  /** Enable security scanning */
  enabled?: boolean;
  /** Minimum severity to fail */
  severity?: "low" | "medium" | "high" | "critical";
  /** Policy path for validation */
  policyPath?: string;
}

// ============================================================================
// Default Modules
// ============================================================================

/** Default CI module - override with your own */
const defaultCiModule = new DaggerModule("github.com/circuit-breaker/ci-modules");

/** Default infra module - override with your own */
const defaultInfraModule = new DaggerModule("github.com/circuit-breaker/infra-modules");

// ============================================================================
// Template Functions
// ============================================================================

/**
 * Add lint step to workflow.
 *
 * Adds: lint transition from 'start' to 'linted'
 *
 * @example
 * ```ts
 * const wf = withLint(workflow('my-app')).build();
 * ```
 */
export function withLint(
  builder: WorkflowBuilder,
  options: { timeout?: string; fix?: boolean } = {}
): WorkflowBuilder {
  return builder
    .place("start", { initialTokens: 1 })
    .place("linted")
    .transition("lint")
    .from("start")
    .to("linted")
    .local()
    .action(test.lint({ fix: options.fix ?? false }))
    .timeout(options.timeout ?? "5m")
    .done();
}

/**
 * Add build step to workflow.
 *
 * Adds: build transition from 'linted' (or 'start') to 'built'
 */
export function withBuild(
  builder: WorkflowBuilder,
  config: BuildConfig = {}
): WorkflowBuilder {
  const module = config.module ?? defaultCiModule;

  // Ensure we have the input place
  const inputPlace = hasPlace(builder, "linted") ? "linted" : "start";
  if (inputPlace === "start" && !hasPlace(builder, "start")) {
    builder = builder.place("start", { initialTokens: 1 });
  }

  return builder
    .place("built")
    .transition("build")
    .from(inputPlace)
    .to("built")
    .action(
      module.fn("build", {
        context: config.context ?? ".",
        dockerfile: config.dockerfile,
        target: config.target ?? "production",
        platforms: config.platforms ?? ["linux/amd64"],
      })
    )
    .policy("./policies/build")
    .timeout(config.timeout ?? "15m")
    .retries(2)
    .done();
}

/**
 * Add test steps to workflow.
 *
 * Adds: unit-test, integration-test, and/or e2e-test transitions
 */
export function withTest(
  builder: WorkflowBuilder,
  config: TestConfig = {}
): WorkflowBuilder {
  const module = config.module ?? defaultCiModule;
  const runUnit = config.unit ?? true;
  const runIntegration = config.integration ?? true;
  const runE2e = config.e2e ?? false;

  // Ensure we have the input place
  const inputPlace = hasPlace(builder, "built") ? "built" : "start";
  if (inputPlace === "start" && !hasPlace(builder, "start")) {
    builder = builder.place("start", { initialTokens: 1 });
  }

  let lastPlace = inputPlace;

  // Unit tests
  if (runUnit) {
    builder = builder
      .place("unit-tested")
      .transition("unit-test")
      .from(lastPlace)
      .to("unit-tested")
      .action(
        module.fn("test", {
          suite: "unit",
          parallel: true,
          coverage: config.coverage ?? false,
        })
      )
      .timeout("10m")
      .done();
    lastPlace = "unit-tested";
  }

  // Integration tests
  if (runIntegration) {
    let tb = builder
      .place("integration-tested")
      .transition("integration-test")
      .from(lastPlace)
      .to("integration-tested");

    // Integration tests may need more resources
    if (config.integrationMemoryGb) {
      tb = tb.engine("auto", { memoryGb: config.integrationMemoryGb });
    }

    builder = tb
      .action(module.fn("test", { suite: "integration" }))
      .timeout("30m")
      .done();
    lastPlace = "integration-tested";
  }

  // E2E tests
  if (runE2e) {
    builder = builder
      .place("e2e-tested")
      .transition("e2e-test")
      .from(lastPlace)
      .to("e2e-tested")
      .engine("auto", { memoryGb: 8 })
      .action(module.fn("test", { suite: "e2e" }))
      .timeout("45m")
      .done();
    lastPlace = "e2e-tested";
  }

  // Add a 'tested' alias place for downstream templates
  if (lastPlace !== "tested") {
    builder = builder
      .place("tested")
      .transition("tests-complete")
      .from(lastPlace)
      .to("tested")
      .noop()
      .done();
  }

  return builder;
}

/**
 * Add build and test steps to workflow (common pattern).
 *
 * This is equivalent to: withTest(withBuild(withLint(builder)))
 */
export function withBuildAndTest(
  builder: WorkflowBuilder,
  options: {
    build?: BuildConfig;
    test?: TestConfig;
    lint?: boolean;
  } = {}
): WorkflowBuilder {
  if (options.lint !== false) {
    builder = withLint(builder);
  }
  builder = withBuild(builder, options.build);
  builder = withTest(builder, options.test);
  return builder;
}

/**
 * Add security scanning to workflow.
 */
export function withSecurity(
  builder: WorkflowBuilder,
  config: SecurityConfig = {}
): WorkflowBuilder {
  if (config.enabled === false) {
    return builder;
  }

  // Find the last test place
  const inputPlace = findLastPlace(builder, ["tested", "e2e-tested", "integration-tested", "unit-tested", "built"]);

  return builder
    .place("security-scanned")
    .transition("security-scan")
    .from(inputPlace)
    .to("security-scanned")
    .action(test.security({ severity: config.severity ?? "high" }))
    .policy(config.policyPath ?? "./policies/security")
    .timeout("15m")
    .done();
}

/**
 * Add deployment steps to workflow.
 *
 * Adds: staging and/or production deployment transitions
 */
export function withDeploy(
  builder: WorkflowBuilder,
  config: DeployConfig
): WorkflowBuilder {
  const module = config.module ?? defaultInfraModule;
  const environments = config.environments ?? ["staging", "production"];

  // Find the last place before deployment
  const inputPlace = findLastPlace(builder, ["security-scanned", "tested", "built"]);
  let lastPlace = inputPlace;

  // Staging deployment
  if (environments.includes("staging")) {
    builder = builder
      .place("staged")
      .transition("deploy-staging")
      .from(lastPlace)
      .to("staged")
      .action(
        module.fn("deploy", {
          cluster: config.cluster,
          namespace: config.namespace ?? "staging",
          environment: "staging",
          replicas: 1,
          healthCheck: true,
        })
      )
      .timeout("10m")
      .done();
    lastPlace = "staged";
  }

  // Production deployment
  if (environments.includes("production")) {
    let tb = builder
      .place("deployed")
      .transition("deploy-production")
      .from(lastPlace)
      .to("deployed")
      .cloud({ requireAudit: true }); // Always audit production

    // Add approval guard if required
    if (config.requireApproval !== false) {
      const count = config.approvalCount ?? 1;
      tb = tb.guard(`ctx.approvals >= ${count}`);
    }

    builder = tb
      .action(
        module.fn("deploy", {
          cluster: config.cluster,
          namespace: config.namespace ?? "production",
          environment: "production",
          replicas: 3,
          strategy: config.strategy ?? "rolling",
          healthCheck: true,
        })
      )
      .policy("./policies/production-deploy")
      .timeout("20m")
      .priority(100)
      .done();
  }

  return builder;
}

/**
 * Add notification steps to workflow.
 */
export function withNotifications(
  builder: WorkflowBuilder,
  config: NotificationConfig
): WorkflowBuilder {
  const lastPlace = findLastPlace(builder, ["deployed", "staged", "tested", "built"]);

  if (config.slack) {
    builder = builder
      .place("slack-notified")
      .transition("notify-slack")
      .from(lastPlace)
      .to("slack-notified")
      .local()
      .action(
        notify.slack({
          channel: config.slack,
          message: "✅ Workflow ${workflow.name} completed successfully!",
        })
      )
      .timeout("1m")
      .done();
  }

  if (config.email && config.email.length > 0) {
    const emailInputPlace = hasPlace(builder, "slack-notified") ? "slack-notified" : lastPlace;
    builder = builder
      .place("email-notified")
      .transition("notify-email")
      .from(emailInputPlace)
      .to("email-notified")
      .local()
      .action(
        notify.email({
          to: config.email,
          subject: "Workflow ${workflow.name} completed",
          body: "The workflow has completed successfully.",
        })
      )
      .timeout("1m")
      .done();
  }

  return builder;
}

// ============================================================================
// Complete Pipeline Templates
// ============================================================================

/**
 * Create a standard CI/CD pipeline with all common steps.
 *
 * @example
 * ```ts
 * const pipeline = standardPipeline('my-service', {
 *   deploy: { cluster: 'prod-us-east-1' },
 *   notifications: { slack: '#deploys' }
 * });
 * ```
 */
export function standardPipeline(
  name: string,
  config: {
    namespace?: string;
    build?: BuildConfig;
    test?: TestConfig;
    security?: SecurityConfig;
    deploy: DeployConfig;
    notifications?: NotificationConfig;
  }
): WorkflowBuilder {
  let builder = workflow(name);

  if (config.namespace) {
    builder = builder.namespace(config.namespace);
  }

  builder = builder
    .description(`Standard CI/CD pipeline for ${name}`)
    .engine("auto");

  // Build and test
  builder = withBuildAndTest(builder, {
    build: config.build,
    test: config.test,
  });

  // Security
  builder = withSecurity(builder, config.security);

  // Deploy
  builder = withDeploy(builder, config.deploy);

  // Notifications
  if (config.notifications) {
    builder = withNotifications(builder, config.notifications);
  }

  return builder;
}

/**
 * Create a simple build-and-test pipeline (no deployment).
 *
 * Great for PRs and feature branches.
 */
export function prPipeline(
  name: string,
  config: {
    build?: BuildConfig;
    test?: TestConfig;
    security?: SecurityConfig;
  } = {}
): WorkflowBuilder {
  let builder = workflow(name)
    .description(`PR validation pipeline for ${name}`)
    .localOnly(); // PRs run locally for speed

  builder = withBuildAndTest(builder, {
    build: config.build,
    test: config.test,
  });

  builder = withSecurity(builder, config.security);

  return builder;
}

/**
 * Create a deploy-only pipeline.
 *
 * Useful when build artifacts already exist (e.g., from a separate build pipeline).
 */
export function deployPipeline(
  name: string,
  config: DeployConfig & {
    namespace?: string;
    notifications?: NotificationConfig;
  }
): WorkflowBuilder {
  let builder = workflow(name)
    .description(`Deployment pipeline for ${name}`)
    .engine("auto");

  if (config.namespace) {
    builder = builder.namespace(config.namespace);
  }

  // Start directly with deployment
  builder = builder.place("ready", { initialTokens: 1 });

  // Deploy
  builder = withDeploy(builder, config);

  // Notifications
  if (config.notifications) {
    builder = withNotifications(builder, config.notifications);
  }

  return builder;
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Check if a workflow builder has a specific place defined.
 */
function hasPlace(builder: WorkflowBuilder, placeId: string): boolean {
  const workflow = builder.inspect();
  return workflow.places.some((p) => p.id === placeId);
}

/**
 * Find the last place from a list of candidates that exists in the workflow.
 */
function findLastPlace(builder: WorkflowBuilder, candidates: string[]): string {
  const workflow = builder.inspect();
  const placeIds = new Set(workflow.places.map((p) => p.id));

  for (const candidate of candidates) {
    if (placeIds.has(candidate)) {
      return candidate;
    }
  }

  // Default to 'start' if nothing found
  return "start";
}

// ============================================================================
// Exports
// ============================================================================

export {
  // Template functions
  withLint,
  withBuild,
  withTest,
  withBuildAndTest,
  withSecurity,
  withDeploy,
  withNotifications,
  // Pipeline factories
  standardPipeline,
  prPipeline,
  deployPipeline,
  // Helpers
  hasPlace,
  findLastPlace,
};
