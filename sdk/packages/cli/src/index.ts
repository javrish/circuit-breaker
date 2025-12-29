#!/usr/bin/env bun
/**
 * Circuit Breaker CLI
 *
 * Command-line interface for managing workflows and interacting with the engine.
 *
 * @module
 */

import { Command } from "commander";
import { resolve, extname } from "path";
import chalk from "chalk";
import ora from "ora";
import {
  WorkflowSchema,
  CircuitBreakerClient,
  validateWorkflow,
  visualize as generateVisualization,
  getGraphvizUrl,
  getMermaidUrl,
  type Workflow,
} from "@circuit-breaker/core";
import logs from "./commands/logs";

const VERSION = "0.1.0";

// ============ Workflow Loading ============

async function loadWorkflow(filePath: string): Promise<Workflow> {
  const absolutePath = resolve(process.cwd(), filePath);
  const ext = extname(filePath).toLowerCase();

  if (ext === ".json") {
    const file = Bun.file(absolutePath);
    const content = await file.json();
    return WorkflowSchema.parse(content);
  }

  if (ext === ".ts" || ext === ".js" || ext === ".mts" || ext === ".mjs") {
    const module = await import(absolutePath);
    const workflow = module.default ?? module.workflow;

    if (!workflow) {
      throw new Error(
        `No workflow found in ${filePath}. Export your workflow as default or named 'workflow'.`,
      );
    }

    if (typeof workflow.build === "function") {
      return workflow.build();
    }

    return WorkflowSchema.parse(workflow);
  }

  throw new Error(`Unsupported file extension: ${ext}. Use .ts, .js, or .json`);
}

// ============ Commands ============

const program = new Command()
  .name("cb")
  .description("Circuit Breaker CLI - Petri-net workflow orchestration")
  .version(VERSION)
  .option(
    "--api-url <url>",
    "Circuit Breaker API URL",
    process.env.CIRCUIT_BREAKER_API ?? "http://localhost:8080",
  )
  .option(
    "--api-key <key>",
    "API key for authentication",
    process.env.CIRCUIT_BREAKER_API_KEY,
  )
  .option("-o, --output <format>", "Output format (json, table)", "table")
  .option("-v, --verbose", "Enable verbose output");

// Validate command
program
  .command("validate <workflow>")
  .description("Validate a workflow definition")
  .option("--strict", "Treat warnings as errors")
  .action(async (workflowPath: string, options: { strict?: boolean }) => {
    const spinner = ora();

    try {
      spinner.start(`Loading workflow from ${chalk.cyan(workflowPath)}...`);
      const workflow = await loadWorkflow(workflowPath);
      spinner.succeed(`Loaded workflow: ${chalk.bold(workflow.name)}`);

      spinner.start("Validating workflow...");
      const result = validateWorkflow(workflow, { strict: options.strict });

      if (result.valid) {
        spinner.succeed(chalk.green("Workflow is valid"));

        console.log("\n" + chalk.bold("Workflow Summary:"));
        console.log(`  Name:        ${workflow.name}`);
        console.log(`  Namespace:   ${workflow.namespace}`);
        console.log(`  Places:      ${workflow.places.length}`);
        console.log(`  Transitions: ${workflow.transitions.length}`);

        const initialPlaces = workflow.places.filter(
          (p) => p.initialTokens > 0,
        );
        if (initialPlaces.length > 0) {
          console.log(
            `  Initial:     ${initialPlaces.map((p) => `${p.id}(${p.initialTokens})`).join(", ")}`,
          );
        }
      } else {
        spinner.fail(chalk.red("Workflow validation failed"));

        console.log("\n" + chalk.red.bold("Errors:"));
        for (const error of result.errors) {
          console.log(chalk.red(`  ✗ [${error.code}] ${error.message}`));
        }
        process.exit(1);
      }

      if (result.warnings.length > 0) {
        console.log("\n" + chalk.yellow.bold("Warnings:"));
        for (const warning of result.warnings) {
          console.log(chalk.yellow(`  ⚠ [${warning.code}] ${warning.message}`));
        }
      }
    } catch (error) {
      spinner.fail("Validation failed");
      console.error(
        chalk.red(`Error: ${error instanceof Error ? error.message : error}`),
      );
      process.exit(1);
    }
  });

// Visualize command
program
  .command("visualize <workflow>")
  .alias("viz")
  .description("Generate a visual representation of the workflow")
  .option("-f, --format <format>", "Output format (dot, mermaid)", "mermaid")
  .option("-o, --output <file>", "Output file (default: stdout)")
  .option("--open", "Open in browser")
  .action(
    async (
      workflowPath: string,
      options: { format?: string; output?: string; open?: boolean },
    ) => {
      try {
        console.log(chalk.dim(`Loading workflow from ${workflowPath}...`));
        const workflow = await loadWorkflow(workflowPath);

        const format = (options.format ?? "mermaid") as "dot" | "mermaid";
        const output = generateVisualization(workflow, {
          format,
          showTokens: true,
          showGuards: true,
        });

        if (options.output) {
          await Bun.write(options.output, output);
          console.log(chalk.green(`✓ Written to ${options.output}`));
        } else {
          console.log("\n" + output);
        }

        if (options.open) {
          const url =
            format === "mermaid"
              ? getMermaidUrl(workflow)
              : getGraphvizUrl(workflow);

          console.log(chalk.dim(`\nOpening: ${url}`));

          const opener =
            process.platform === "darwin"
              ? "open"
              : process.platform === "win32"
                ? "start"
                : "xdg-open";
          Bun.spawn([opener, url]);
        }
      } catch (error) {
        console.error(
          chalk.red(`Error: ${error instanceof Error ? error.message : error}`),
        );
        process.exit(1);
      }
    },
  );

// Submit command
program
  .command("submit <workflow>")
  .description("Submit a workflow definition to the engine")
  .option("--dry-run", "Validate only, don't submit")
  .action(
    async (
      workflowPath: string,
      options: { dryRun?: boolean },
      cmd: Command,
    ) => {
      const spinner = ora();
      const globalOpts = cmd.optsWithGlobals();

      try {
        spinner.start(`Loading workflow from ${chalk.cyan(workflowPath)}...`);
        const workflow = await loadWorkflow(workflowPath);
        spinner.succeed(`Loaded: ${chalk.bold(workflow.name)}`);

        // Validate first
        spinner.start("Validating...");
        const validation = validateWorkflow(workflow);
        if (!validation.valid) {
          spinner.fail("Validation failed");
          for (const err of validation.errors) {
            console.error(chalk.red(`  ✗ ${err.message}`));
          }
          process.exit(1);
        }
        spinner.succeed("Validation passed");

        if (options.dryRun) {
          console.log(chalk.yellow("\n--dry-run specified, skipping submit"));
          return;
        }

        // Submit to API
        spinner.start("Submitting to engine...");
        const client = new CircuitBreakerClient({
          baseUrl: globalOpts.apiUrl,
          apiKey: globalOpts.apiKey,
        });

        const result = await client.submitWorkflow(workflow);
        spinner.succeed(chalk.green("Submitted successfully"));

        console.log("\n" + chalk.bold("Workflow ID:"), result.workflowId);
        console.log(chalk.bold("Name:"), result.name);
        console.log(chalk.bold("Namespace:"), result.namespace);
      } catch (error) {
        spinner.fail("Submit failed");
        console.error(
          chalk.red(`Error: ${error instanceof Error ? error.message : error}`),
        );
        process.exit(1);
      }
    },
  );

// Run command
program
  .command("run <workflow>")
  .description("Submit and run a workflow")
  .option("-w, --watch", "Watch execution progress")
  .action(
    async (
      workflowPath: string,
      options: { watch?: boolean },
      cmd: Command,
    ) => {
      const spinner = ora();
      const globalOpts = cmd.optsWithGlobals();

      try {
        spinner.start(`Loading workflow from ${chalk.cyan(workflowPath)}...`);
        const workflow = await loadWorkflow(workflowPath);
        spinner.succeed(`Loaded: ${chalk.bold(workflow.name)}`);

        const client = new CircuitBreakerClient({
          baseUrl: globalOpts.apiUrl,
          apiKey: globalOpts.apiKey,
        });

        // Submit
        spinner.start("Submitting...");
        const submitResult = await client.submitWorkflow(workflow);
        spinner.succeed(`Submitted: ${submitResult.workflowId}`);

        // Run
        spinner.start("Starting run...");
        const runResult = await client.runWorkflow(submitResult.workflowId);
        spinner.succeed(`Started run: ${chalk.cyan(runResult.runId)}`);

        if (options.watch) {
          console.log(chalk.dim("\nWatching for updates...\n"));

          for await (const status of client.watchRun(runResult.runId)) {
            const icon =
              status.status === "completed"
                ? chalk.green("✓")
                : status.status === "failed"
                  ? chalk.red("✗")
                  : chalk.blue("⟳");

            console.log(`${icon} Status: ${status.status}`);

            if (status.status === "completed") {
              console.log(chalk.green("\n✓ Workflow completed"));
              break;
            }
            if (status.status === "failed") {
              console.log(chalk.red("\n✗ Workflow failed"));
              if (status.error) {
                console.error(chalk.red(`  ${status.error.message}`));
              }
              process.exit(1);
            }
            if (status.status === "cancelled") {
              console.log(chalk.yellow("\n⊘ Workflow cancelled"));
              break;
            }
          }
        } else {
          console.log(chalk.dim(`\nUse: cb status ${runResult.runId} --watch`));
        }
      } catch (error) {
        spinner.fail("Run failed");
        console.error(
          chalk.red(`Error: ${error instanceof Error ? error.message : error}`),
        );
        process.exit(1);
      }
    },
  );

// Status command
program
  .command("status <runId>")
  .description("Get status of a workflow run")
  .option("-w, --watch", "Watch for updates")
  .action(async (runId: string, options: { watch?: boolean }, cmd: Command) => {
    const globalOpts = cmd.optsWithGlobals();

    const client = new CircuitBreakerClient({
      baseUrl: globalOpts.apiUrl,
      apiKey: globalOpts.apiKey,
    });

    try {
      if (options.watch) {
        console.log(chalk.dim(`Watching run ${runId}...\n`));

        for await (const status of client.watchRun(runId)) {
          console.clear();
          console.log(chalk.bold(`Run: ${runId}`));
          console.log(`Status: ${status.status}`);
          console.log(`Workflow: ${status.workflowName}`);

          if (["completed", "failed", "cancelled"].includes(status.status)) {
            break;
          }
        }
      } else {
        const status = await client.getRunStatus(runId);

        if (globalOpts.output === "json") {
          console.log(JSON.stringify(status, null, 2));
        } else {
          console.log(chalk.bold(`Run: ${runId}`));
          console.log(`  Status:   ${status.status}`);
          console.log(`  Workflow: ${status.workflowName}`);
          console.log(`  Started:  ${status.startedAt}`);
          if (status.completedAt) {
            console.log(`  Completed: ${status.completedAt}`);
          }
        }
      }
    } catch (error) {
      console.error(
        chalk.red(`Error: ${error instanceof Error ? error.message : error}`),
      );
      process.exit(1);
    }
  });

// Logs command
program
  .command("logs <runId>")
  .description("Get logs for a workflow run")
  .option("-t, --transition <id>", "Filter by transition ID")
  .option("-f, --follow", "Follow log output")
  .option("--tail <lines>", "Number of lines to show", "100")
  .option("--json", "Output as JSON")
  .action(logs);

// Health command
program
  .command("health")
  .description("Check API server health")
  .action(async (_, cmd: Command) => {
    const globalOpts = cmd.optsWithGlobals();

    const client = new CircuitBreakerClient({
      baseUrl: globalOpts.apiUrl,
      apiKey: globalOpts.apiKey,
    });

    try {
      const health = await client.health();
      console.log(chalk.green("✓ API is healthy"));
      console.log(`  Status:  ${health.status}`);
      console.log(`  Version: ${health.version}`);
    } catch (error) {
      console.error(chalk.red("✗ API is unreachable"));
      console.error(
        chalk.red(`  ${error instanceof Error ? error.message : error}`),
      );
      process.exit(1);
    }
  });

// Parse and run
program.parse(process.argv);
