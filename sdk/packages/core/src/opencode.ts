/**
 * OpenCode Integration for Circuit Breaker
 *
 * This module provides integration with OpenCode (https://github.com/sst/opencode)
 * as an executable task within Circuit Breaker workflows. OpenCode is an open-source
 * AI coding agent that can perform code generation, review, refactoring, and more.
 *
 * @module
 */

import type { TaskAction } from "./workflow";

// ============ Types ============

/**
 * OpenCode provider configuration.
 */
export type OpenCodeProvider =
  | "anthropic"
  | "openai"
  | "google"
  | "opencode" // OpenCode Zen
  | "ollama"
  | "custom";

/**
 * OpenCode model specification.
 */
export interface OpenCodeModel {
  /** Provider name */
  provider: OpenCodeProvider;
  /** Model identifier (e.g., "claude-sonnet-4-20250514", "gpt-4o") */
  model: string;
}

/**
 * OpenCode agent mode.
 */
export type OpenCodeAgent = "build" | "plan";

/**
 * OpenCode task configuration.
 */
export interface OpenCodeConfig {
  /** The prompt/message to send to OpenCode */
  prompt: string;

  /** Model to use (provider/model format) */
  model?: OpenCodeModel | string;

  /** Agent mode: "build" (default, full access) or "plan" (read-only) */
  agent?: OpenCodeAgent;

  /** Continue from a previous session */
  continueSession?: boolean;

  /** Specific session ID to continue */
  sessionId?: string;

  /** Files to attach to the prompt */
  files?: string[];

  /** Custom command to run instead of prompt */
  command?: string;

  /** Arguments for the command */
  commandArgs?: string;

  /** Output format: "default" or "json" */
  format?: "default" | "json";

  /** Session title */
  title?: string;

  /** Working directory (defaults to /workspace) */
  workdir?: string;

  /** Environment variables to pass */
  env?: Record<string, string>;

  /** Timeout in seconds (default: 600 = 10 minutes) */
  timeout?: number;

  /** Auto-approve all permission requests */
  autoApprove?: boolean;
}

/**
 * OpenCode task result.
 */
export interface OpenCodeResult {
  /** Whether the task completed successfully */
  success: boolean;

  /** Output from OpenCode */
  output: string;

  /** Session ID for continuation */
  sessionId?: string;

  /** Share URL if sharing was enabled */
  shareUrl?: string;

  /** Files that were modified */
  modifiedFiles?: string[];

  /** Error message if failed */
  error?: string;

  /** Duration in milliseconds */
  durationMs: number;
}

// ============ Docker Image ============

/**
 * OpenCode Docker image with all dependencies.
 * This is a custom image that bundles OpenCode with necessary tools.
 */
export const OPENCODE_IMAGE = "ghcr.io/sst/opencode:latest";

/**
 * Alternative: Build OpenCode image from npm
 */
export const OPENCODE_DOCKERFILE = `
FROM oven/bun:1-alpine

# Install git and other common tools
RUN apk add --no-cache git curl bash openssh-client

# Install opencode globally
RUN bun install -g opencode-ai@latest

# Create workspace directory
WORKDIR /workspace

# Set up non-interactive mode
ENV CI=true
ENV OPENCODE_NON_INTERACTIVE=true

ENTRYPOINT ["opencode"]
`;

// ============ Task Builder ============

/**
 * Builder for OpenCode tasks.
 */
export class OpenCodeTaskBuilder {
  private config: OpenCodeConfig;

  constructor(prompt: string) {
    this.config = {
      prompt,
      timeout: 600,
      workdir: "/workspace",
    };
  }

  /**
   * Set the model to use.
   *
   * @example
   * opencode("Fix the bug")
   *   .model("anthropic", "claude-sonnet-4-20250514")
   */
  model(provider: OpenCodeProvider, model: string): this;
  model(modelString: string): this;
  model(providerOrString: OpenCodeProvider | string, model?: string): this {
    if (model) {
      this.config.model = {
        provider: providerOrString as OpenCodeProvider,
        model,
      };
    } else {
      this.config.model = providerOrString;
    }
    return this;
  }

  /**
   * Set the agent mode.
   *
   * @param agent - "build" for full access, "plan" for read-only analysis
   */
  agent(agent: OpenCodeAgent): this {
    this.config.agent = agent;
    return this;
  }

  /**
   * Use plan mode (read-only, no file modifications).
   */
  plan(): this {
    this.config.agent = "plan";
    return this;
  }

  /**
   * Use build mode (full access, can modify files).
   */
  build(): this {
    this.config.agent = "build";
    return this;
  }

  /**
   * Attach files to the prompt.
   *
   * @param files - File paths relative to workspace
   */
  files(...files: string[]): this {
    this.config.files = files;
    return this;
  }

  /**
   * Run a specific command instead of a prompt.
   *
   * @param command - Command name (e.g., "init", "pr")
   * @param args - Command arguments
   */
  command(command: string, args?: string): this {
    this.config.command = command;
    if (args !== undefined) {
      this.config.commandArgs = args;
    }
    return this;
  }

  /**
   * Continue from the last session.
   */
  continue(): this {
    this.config.continueSession = true;
    return this;
  }

  /**
   * Continue from a specific session.
   *
   * @param sessionId - Session ID to continue
   */
  session(sessionId: string): this {
    this.config.sessionId = sessionId;
    return this;
  }

  /**
   * Set output format.
   *
   * @param format - "default" for human-readable, "json" for machine-parseable
   */
  format(format: "default" | "json"): this {
    this.config.format = format;
    return this;
  }

  /**
   * Set session title.
   */
  title(title: string): this {
    this.config.title = title;
    return this;
  }

  /**
   * Set working directory.
   */
  workdir(dir: string): this {
    this.config.workdir = dir;
    return this;
  }

  /**
   * Set environment variables.
   */
  env(vars: Record<string, string>): this {
    this.config.env = { ...this.config.env, ...vars };
    return this;
  }

  /**
   * Set timeout in seconds.
   */
  timeout(seconds: number): this {
    this.config.timeout = seconds;
    return this;
  }

  /**
   * Auto-approve all permission requests.
   * Use with caution - this allows OpenCode to run any command.
   */
  autoApprove(): this {
    this.config.autoApprove = true;
    return this;
  }

  /**
   * Build the task action for use in a workflow.
   */
  toAction(): TaskAction {
    const action: TaskAction = {
      type: "container",
      image: OPENCODE_IMAGE,
      command: this.buildCommand(),
      env: this.buildEnv(),
    };

    if (this.config.workdir) {
      action.workdir = this.config.workdir;
    }

    return action;
  }

  /**
   * Get the configuration object.
   */
  getConfig(): OpenCodeConfig {
    return { ...this.config };
  }

  /**
   * Build the OpenCode command.
   */
  private buildCommand(): string[] {
    const args: string[] = ["run"];

    // Model
    if (this.config.model) {
      const modelStr =
        typeof this.config.model === "string"
          ? this.config.model
          : `${this.config.model.provider}/${this.config.model.model}`;
      args.push("--model", modelStr);
    }

    // Agent
    if (this.config.agent) {
      args.push("--agent", this.config.agent);
    }

    // Format
    if (this.config.format) {
      args.push("--format", this.config.format);
    }

    // Session continuation
    if (this.config.continueSession) {
      args.push("--continue");
    } else if (this.config.sessionId) {
      args.push("--session", this.config.sessionId);
    }

    // Title
    if (this.config.title) {
      args.push("--title", this.config.title);
    }

    // Files
    if (this.config.files && this.config.files.length > 0) {
      for (const file of this.config.files) {
        args.push("--file", file);
      }
    }

    // Command mode
    if (this.config.command) {
      args.push("--command", this.config.command);
      if (this.config.commandArgs) {
        args.push(this.config.commandArgs);
      }
    } else {
      // Prompt mode
      args.push(this.config.prompt);
    }

    return args;
  }

  /**
   * Build environment variables.
   */
  private buildEnv(): Record<string, string> {
    const env: Record<string, string> = {
      CI: "true",
      OPENCODE_NON_INTERACTIVE: "true",
      ...this.config.env,
    };

    if (this.config.autoApprove) {
      env.OPENCODE_AUTO_APPROVE = "true";
    }

    return env;
  }
}

// ============ Factory Functions ============

/**
 * Create an OpenCode task.
 *
 * @param prompt - The prompt/message to send to OpenCode
 * @returns OpenCodeTaskBuilder for fluent configuration
 *
 * @example
 * // Simple code review
 * opencode("Review this code for bugs and security issues")
 *   .plan()
 *   .files("src/auth.ts")
 *
 * @example
 * // Code generation
 * opencode("Add input validation to the user registration endpoint")
 *   .model("anthropic", "claude-sonnet-4-20250514")
 *   .files("src/routes/register.ts", "src/schemas/user.ts")
 *
 * @example
 * // Refactoring
 * opencode("Refactor this function to use async/await instead of callbacks")
 *   .files("src/legacy/database.js")
 *   .autoApprove()
 *
 * @example
 * // Continue previous session
 * opencode("Now add unit tests for the changes you made")
 *   .continue()
 */
export function opencode(prompt: string): OpenCodeTaskBuilder {
  return new OpenCodeTaskBuilder(prompt);
}

// ============ Preset Tasks ============

/**
 * Preset OpenCode tasks for common operations.
 */
export const OpenCodeTasks = {
  /**
   * Code review task.
   *
   * @param files - Files to review
   * @param focus - Optional focus area (e.g., "security", "performance")
   */
  review(files: string[], focus?: string): OpenCodeTaskBuilder {
    const prompt = focus
      ? `Review the following code with a focus on ${focus}. Identify issues and suggest improvements.`
      : "Review the following code. Identify bugs, security issues, and suggest improvements.";

    return opencode(prompt)
      .plan()
      .files(...files);
  },

  /**
   * Bug fix task.
   *
   * @param description - Description of the bug
   * @param files - Relevant files
   */
  fix(description: string, files: string[]): OpenCodeTaskBuilder {
    return opencode(`Fix the following bug: ${description}`)
      .build()
      .files(...files);
  },

  /**
   * Add tests task.
   *
   * @param files - Files to add tests for
   * @param framework - Test framework (e.g., "jest", "vitest", "pytest")
   */
  addTests(files: string[], framework?: string): OpenCodeTaskBuilder {
    const prompt = framework
      ? `Add comprehensive unit tests using ${framework} for the following code.`
      : "Add comprehensive unit tests for the following code.";

    return opencode(prompt)
      .build()
      .files(...files);
  },

  /**
   * Refactor task.
   *
   * @param goal - Refactoring goal
   * @param files - Files to refactor
   */
  refactor(goal: string, files: string[]): OpenCodeTaskBuilder {
    return opencode(`Refactor the following code: ${goal}`)
      .build()
      .files(...files);
  },

  /**
   * Documentation task.
   *
   * @param files - Files to document
   * @param style - Documentation style (e.g., "jsdoc", "docstring", "markdown")
   */
  document(files: string[], style?: string): OpenCodeTaskBuilder {
    const prompt = style
      ? `Add ${style} documentation to the following code.`
      : "Add comprehensive documentation to the following code.";

    return opencode(prompt)
      .build()
      .files(...files);
  },

  /**
   * Security audit task.
   *
   * @param files - Files to audit (or empty for full project)
   */
  securityAudit(files?: string[]): OpenCodeTaskBuilder {
    const task = opencode(
      "Perform a security audit. Identify vulnerabilities, insecure patterns, and suggest fixes.",
    ).plan();

    if (files && files.length > 0) {
      task.files(...files);
    }

    return task;
  },

  /**
   * Performance optimization task.
   *
   * @param files - Files to optimize
   */
  optimize(files: string[]): OpenCodeTaskBuilder {
    return opencode(
      "Analyze and optimize the following code for performance. Explain the optimizations.",
    )
      .build()
      .files(...files);
  },

  /**
   * Migration task.
   *
   * @param from - Current technology/version
   * @param to - Target technology/version
   * @param files - Files to migrate
   */
  migrate(from: string, to: string, files: string[]): OpenCodeTaskBuilder {
    return opencode(`Migrate the following code from ${from} to ${to}.`)
      .build()
      .files(...files);
  },

  /**
   * Initialize project with AGENTS.md.
   */
  init(): OpenCodeTaskBuilder {
    return opencode("").command("init");
  },
};

// ============ Dagger Integration ============

/**
 * Generate Dagger pipeline code for an OpenCode task.
 *
 * @param task - OpenCode task builder
 * @returns Dagger pipeline function
 */
export function opencodeToDagger(task: OpenCodeTaskBuilder): string {
  const config = task.getConfig();
  const action = task.toAction();

  return `
import { dag, Container, Directory } from "@dagger.io/dagger";

export async function runOpenCode(source: Directory): Promise<Container> {
  return dag
    .container()
    .from("${action.image}")
    .withDirectory("/workspace", source)
    .withWorkdir("${config.workdir || "/workspace"}")
    ${Object.entries(action.env || {})
      .map(([k, v]) => `.withEnvVariable("${k}", "${v}")`)
      .join("\n    ")}
    .withExec(${JSON.stringify(action.command)});
}
`;
}

// ============ Workflow Integration Example ============

/**
 * Example workflow using OpenCode for AI-powered code review and fixes.
 *
 * @example
 * ```typescript
 * import { workflow, task } from "@circuit-breaker/core";
 * import { opencode, OpenCodeTasks } from "@circuit-breaker/core/opencode";
 *
 * export default workflow("ai-code-review")
 *   .namespace("ci")
 *   .place("start", { initial: 1 })
 *   .place("reviewed")
 *   .place("fixed")
 *   .place("tested")
 *   .place("done")
 *
 *   // AI Code Review
 *   .transition("review")
 *     .from("start").to("reviewed")
 *     .task(task("ai-review")
 *       .opencode(OpenCodeTasks.review(["src/**‍/*.ts"], "security")))
 *
 *   // AI Bug Fix (if issues found)
 *   .transition("fix")
 *     .from("reviewed").to("fixed")
 *     .guard("review.output.includes('issue')")
 *     .task(task("ai-fix")
 *       .opencode(opencode("Fix the issues identified in the review")
 *         .continue()
 *         .autoApprove()))
 *
 *   // AI Test Generation
 *   .transition("test")
 *     .from("fixed").to("tested")
 *     .task(task("ai-tests")
 *       .opencode(OpenCodeTasks.addTests(["src/**‍/*.ts"], "vitest")))
 *
 *   // Complete
 *   .transition("complete")
 *     .from("tested").to("done")
 *
 *   .build();
 * ```
 */
export const OPENCODE_WORKFLOW_EXAMPLE = "See JSDoc above";
