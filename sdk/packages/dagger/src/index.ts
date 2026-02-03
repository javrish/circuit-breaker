/**
 * @circuit-breaker/dagger - Reusable Dagger action builders for Circuit Breaker workflows.
 *
 * This module provides composable, reusable action builders for common CI/CD patterns
 * using Dagger modules. Instead of defining everything inline, you can create and share
 * standardized pipeline components.
 *
 * @example
 * ```ts
 * import { workflow } from '@circuit-breaker/core';
 * import { build, test, deploy, DaggerModule } from '@circuit-breaker/dagger';
 *
 * // Define your CI module once
 * const ci = new DaggerModule('github.com/myorg/ci-modules');
 *
 * // Reuse across workflows
 * const pipeline = workflow('my-pipeline')
 *   .place('start', { initialTokens: 1 })
 *   .place('built')
 *   .place('tested')
 *   .place('deployed')
 *
 *   .transition('build')
 *     .from('start').to('built')
 *     .action(ci.fn('build', { target: 'production' }))
 *     .done()
 *
 *   .transition('test')
 *     .from('built').to('tested')
 *     .action(ci.fn('test', { parallel: true }))
 *     .gpu(32) // ML tests need GPU
 *     .done()
 *
 *   .transition('deploy')
 *     .from('tested').to('deployed')
 *     .action(deploy.toKubernetes({ cluster: 'prod' }))
 *     .audit() // Requires cloud for audit trail
 *     .done()
 *
 *   .build();
 * ```
 *
 * @module
 */

// ============================================================================
// Types
// ============================================================================

/**
 * Dagger action configuration that can be passed to TransitionBuilder.action()
 */
export interface DaggerAction {
  type: "dagger";
  module: string;
  function?: string;
  args?: Record<string, unknown>;
  image?: string;
  cache?: boolean;
}

/**
 * Options for configuring a Dagger function call.
 */
export interface FunctionOptions {
  /** Arguments to pass to the function */
  args?: Record<string, unknown>;
  /** Override the container image */
  image?: string;
  /** Enable/disable caching (default: true) */
  cache?: boolean;
}

/**
 * Options for build actions.
 */
export interface BuildOptions {
  /** Build context path */
  context?: string;
  /** Dockerfile path */
  dockerfile?: string;
  /** Build target stage */
  target?: string;
  /** Target platforms */
  platforms?: string[];
  /** Build arguments */
  buildArgs?: Record<string, string>;
  /** Enable caching */
  cache?: boolean;
}

/**
 * Options for test actions.
 */
export interface TestOptions {
  /** Test suite to run */
  suite?: string;
  /** Run tests in parallel */
  parallel?: boolean;
  /** Enable coverage collection */
  coverage?: boolean;
  /** Test filter pattern */
  filter?: string;
  /** Timeout per test */
  timeout?: string;
}

/**
 * Options for deployment actions.
 */
export interface DeployOptions {
  /** Target environment */
  environment?: string;
  /** Number of replicas */
  replicas?: number;
  /** Enable health checks */
  healthCheck?: boolean;
  /** Deployment strategy */
  strategy?: "rolling" | "blue-green" | "canary";
  /** Canary percentage (if strategy is canary) */
  canaryPercent?: number;
}

/**
 * Options for Kubernetes deployment.
 */
export interface KubernetesDeployOptions extends DeployOptions {
  /** Kubernetes cluster name */
  cluster: string;
  /** Kubernetes namespace */
  namespace?: string;
  /** Path to Kubernetes manifests */
  manifests?: string;
}

/**
 * Options for container registry operations.
 */
export interface RegistryOptions {
  /** Registry URL */
  registry?: string;
  /** Image repository */
  repository: string;
  /** Image tag */
  tag?: string;
  /** Additional tags */
  additionalTags?: string[];
}

// ============================================================================
// DaggerModule - Reusable module reference
// ============================================================================

/**
 * A reusable reference to a Dagger module.
 *
 * Create a module reference once and use it across multiple transitions
 * for consistent, DRY workflow definitions.
 *
 * @example
 * ```ts
 * // Define module once
 * const ci = new DaggerModule('github.com/myorg/ci');
 *
 * // Use in transitions
 * .transition('build')
 *   .action(ci.fn('build'))
 *   .done()
 *
 * .transition('test')
 *   .action(ci.fn('test', { coverage: true }))
 *   .done()
 * ```
 */
export class DaggerModule {
  constructor(
    /** Path to the Dagger module (git URL, OCI reference, or local path) */
    public readonly path: string,
    /** Default options applied to all function calls */
    private defaults: Partial<FunctionOptions> = {},
  ) {}

  /**
   * Create a function call action for this module.
   *
   * @param name - Function name to call
   * @param args - Arguments to pass to the function
   * @param options - Additional options (merged with defaults)
   * @returns A DaggerAction that can be passed to TransitionBuilder.action()
   */
  fn(
    name: string,
    args?: Record<string, unknown>,
    options?: Partial<FunctionOptions>,
  ): DaggerAction {
    return {
      type: "dagger",
      module: this.path,
      function: name,
      args: { ...args },
      image: options?.image ?? this.defaults.image,
      cache: options?.cache ?? this.defaults.cache ?? true,
    };
  }

  /**
   * Create a new module reference with different defaults.
   *
   * @param defaults - New default options
   * @returns A new DaggerModule with the specified defaults
   */
  withDefaults(defaults: Partial<FunctionOptions>): DaggerModule {
    return new DaggerModule(this.path, { ...this.defaults, ...defaults });
  }

  /**
   * Create a module reference with caching disabled.
   */
  noCache(): DaggerModule {
    return this.withDefaults({ cache: false });
  }

  /**
   * Create a module reference with a custom image.
   */
  withImage(image: string): DaggerModule {
    return this.withDefaults({ image });
  }
}

// ============================================================================
// Pre-built Action Factories
// ============================================================================

/**
 * Factory for creating build-related Dagger actions.
 *
 * @example
 * ```ts
 * import { build } from '@circuit-breaker/dagger';
 *
 * .transition('build')
 *   .action(build.container({ target: 'production' }))
 *   .done()
 * ```
 */
export const build = {
  /**
   * Build a container image.
   */
  container(options: BuildOptions = {}): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "container-build",
      args: {
        context: options.context ?? ".",
        dockerfile: options.dockerfile ?? "Dockerfile",
        target: options.target,
        platforms: options.platforms,
        buildArgs: options.buildArgs,
      },
      cache: options.cache ?? true,
    };
  },

  /**
   * Build with a specific Dagger module.
   */
  withModule(
    module: string,
    fn = "build",
    options: BuildOptions = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module,
      function: fn,
      args: {
        context: options.context ?? ".",
        dockerfile: options.dockerfile,
        target: options.target,
        platforms: options.platforms,
        buildArgs: options.buildArgs,
      },
      cache: options.cache ?? true,
    };
  },

  /**
   * Build a Go binary.
   */
  go(
    options: { package?: string; output?: string; ldflags?: string } = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "go-build",
      args: {
        package: options.package ?? "./...",
        output: options.output,
        ldflags: options.ldflags,
      },
      cache: true,
    };
  },

  /**
   * Build a Node.js/TypeScript project.
   */
  node(
    options: {
      script?: string;
      packageManager?: "npm" | "yarn" | "pnpm" | "bun";
    } = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "node-build",
      args: {
        script: options.script ?? "build",
        packageManager: options.packageManager ?? "npm",
      },
      cache: true,
    };
  },

  /**
   * Build a Rust project.
   */
  rust(
    options: { target?: string; release?: boolean; features?: string[] } = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "rust-build",
      args: {
        target: options.target,
        release: options.release ?? true,
        features: options.features,
      },
      cache: true,
    };
  },
};

/**
 * Factory for creating test-related Dagger actions.
 *
 * @example
 * ```ts
 * import { test } from '@circuit-breaker/dagger';
 *
 * .transition('unit-tests')
 *   .action(test.run({ parallel: true, coverage: true }))
 *   .done()
 * ```
 */
export const test = {
  /**
   * Run tests with a generic test runner.
   */
  run(options: TestOptions = {}): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "test-run",
      args: {
        suite: options.suite,
        parallel: options.parallel ?? false,
        coverage: options.coverage ?? false,
        filter: options.filter,
        timeout: options.timeout,
      },
      cache: false, // Tests should not be cached by default
    };
  },

  /**
   * Run tests with a specific Dagger module.
   */
  withModule(
    module: string,
    fn = "test",
    options: TestOptions = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module,
      function: fn,
      args: {
        suite: options.suite,
        parallel: options.parallel,
        coverage: options.coverage,
        filter: options.filter,
        timeout: options.timeout,
      },
      cache: false,
    };
  },

  /**
   * Run unit tests.
   */
  unit(options: Omit<TestOptions, "suite"> = {}): DaggerAction {
    return test.run({ ...options, suite: "unit" });
  },

  /**
   * Run integration tests.
   */
  integration(options: Omit<TestOptions, "suite"> = {}): DaggerAction {
    return test.run({ ...options, suite: "integration" });
  },

  /**
   * Run end-to-end tests.
   */
  e2e(options: Omit<TestOptions, "suite"> = {}): DaggerAction {
    return test.run({ ...options, suite: "e2e" });
  },

  /**
   * Run linting/static analysis.
   */
  lint(options: { fix?: boolean; config?: string } = {}): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "lint",
      args: {
        fix: options.fix ?? false,
        config: options.config,
      },
      cache: true,
    };
  },

  /**
   * Run security scanning.
   */
  security(
    options: { severity?: "low" | "medium" | "high" | "critical" } = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "security-scan",
      args: {
        severity: options.severity ?? "high",
      },
      cache: false,
    };
  },
};

/**
 * Factory for creating deployment-related Dagger actions.
 *
 * @example
 * ```ts
 * import { deploy } from '@circuit-breaker/dagger';
 *
 * .transition('deploy-prod')
 *   .action(deploy.toKubernetes({ cluster: 'prod-us-east-1' }))
 *   .audit()
 *   .done()
 * ```
 */
export const deploy = {
  /**
   * Deploy to Kubernetes.
   */
  toKubernetes(options: KubernetesDeployOptions): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "kubernetes-deploy",
      args: {
        cluster: options.cluster,
        namespace: options.namespace ?? "default",
        manifests: options.manifests,
        environment: options.environment,
        replicas: options.replicas,
        healthCheck: options.healthCheck ?? true,
        strategy: options.strategy ?? "rolling",
        canaryPercent: options.canaryPercent,
      },
      cache: false, // Never cache deployments
    };
  },

  /**
   * Deploy with a specific Dagger module.
   */
  withModule(
    module: string,
    fn = "deploy",
    options: DeployOptions = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module,
      function: fn,
      args: {
        environment: options.environment,
        replicas: options.replicas,
        healthCheck: options.healthCheck,
        strategy: options.strategy,
        canaryPercent: options.canaryPercent,
      },
      cache: false,
    };
  },

  /**
   * Push image to registry.
   */
  pushImage(options: RegistryOptions): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "registry-push",
      args: {
        registry: options.registry ?? "docker.io",
        repository: options.repository,
        tag: options.tag ?? "latest",
        additionalTags: options.additionalTags,
      },
      cache: false,
    };
  },

  /**
   * Deploy to AWS ECS.
   */
  toEcs(
    options: {
      cluster: string;
      service: string;
      taskDefinition?: string;
    } & DeployOptions,
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "ecs-deploy",
      args: {
        cluster: options.cluster,
        service: options.service,
        taskDefinition: options.taskDefinition,
        replicas: options.replicas,
        healthCheck: options.healthCheck ?? true,
      },
      cache: false,
    };
  },

  /**
   * Deploy to Google Cloud Run.
   */
  toCloudRun(
    options: {
      project: string;
      service: string;
      region: string;
    } & DeployOptions,
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "cloudrun-deploy",
      args: {
        project: options.project,
        service: options.service,
        region: options.region,
        replicas: options.replicas,
      },
      cache: false,
    };
  },

  /**
   * Deploy to Vercel.
   */
  toVercel(
    options: { project?: string; production?: boolean } = {},
  ): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "vercel-deploy",
      args: {
        project: options.project,
        production: options.production ?? false,
      },
      cache: false,
    };
  },
};

/**
 * Factory for creating notification actions.
 */
export const notify = {
  /**
   * Send Slack notification.
   */
  slack(options: {
    channel: string;
    message: string;
    webhook?: string;
  }): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "slack-notify",
      args: {
        channel: options.channel,
        message: options.message,
        webhook: options.webhook,
      },
      cache: false,
    };
  },

  /**
   * Send email notification.
   */
  email(options: {
    to: string[];
    subject: string;
    body: string;
  }): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "email-notify",
      args: {
        to: options.to,
        subject: options.subject,
        body: options.body,
      },
      cache: false,
    };
  },

  /**
   * Create GitHub issue or comment.
   */
  github(options: {
    repo: string;
    title?: string;
    body: string;
    issue?: number;
  }): DaggerAction {
    return {
      type: "dagger",
      module: "github.com/dagger/dagger",
      function: "github-notify",
      args: {
        repo: options.repo,
        title: options.title,
        body: options.body,
        issue: options.issue,
      },
      cache: false,
    };
  },
};

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Create a custom Dagger action.
 *
 * @param module - Path to Dagger module
 * @param fn - Function name to call
 * @param args - Arguments to pass
 * @param options - Additional options
 */
export function dagger(
  module: string,
  fn?: string,
  args?: Record<string, unknown>,
  options: { cache?: boolean; image?: string } = {},
): DaggerAction {
  return {
    type: "dagger",
    module,
    function: fn,
    args,
    cache: options.cache ?? true,
    image: options.image,
  };
}

/**
 * Create a sequence of Dagger actions to be executed in order.
 * Returns an array that can be used with workflow helpers.
 */
export function sequence(...actions: DaggerAction[]): DaggerAction[] {
  return actions;
}

// ============================================================================
// VCS Helpers
// ============================================================================

interface CloneOptions {
  /** Repository URL (https://, ssh://, or file:// for local) */
  repo?: string;
  /** Sparse checkout paths */
  sparse?: string[];
  /** Branch or tag to checkout */
  ref?: string;
  /** Shallow clone depth (ignored for file://) */
  depth?: number;
}

/**
 * Git VCS operations
 * Supports https://, ssh://, and file:// URLs
 */
export const git = {
  clone: (options: CloneOptions = {}): DaggerAction => ({
    type: "dagger",
    module: "github.com/circuit-breaker/vcs",
    function: "git-clone",
    args: options,
    cache: true,
  }),
};

/**
 * Atomic VCS operations
 * Supports https://, ssh://, and file:// URLs
 */
export const atomic = {
  clone: (options: CloneOptions = {}): DaggerAction => ({
    type: "dagger",
    module: "github.com/circuit-breaker/vcs",
    function: "atomic-clone",
    args: options,
    cache: true,
  }),
};

// ============================================================================
// Re-exports for convenience
// ============================================================================

export type { DaggerAction as Action };
