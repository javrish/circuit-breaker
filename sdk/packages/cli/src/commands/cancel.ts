/**
 * Cancel command - cancels a running workflow.
 *
 * @module
 */

import type { Command } from 'commander';
import { CircuitBreakerClient } from '@circuit-breaker/core';
import chalk from 'chalk';
import ora from 'ora';

interface CancelOptions {
  reason?: string;
  force?: boolean;
}

/**
 * Execute the cancel command.
 */
export async function cancel(
  runId: string,
  options: CancelOptions,
  command: Command
): Promise<void> {
  const parentOpts = command.parent?.opts() ?? {};
  const spinner = ora();

  // Confirm cancellation unless --force is specified
  if (!options.force) {
    const readline = await import('readline');
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });

    const answer = await new Promise<string>((resolve) => {
      rl.question(
        chalk.yellow(`Are you sure you want to cancel run ${runId}? [y/N] `),
        resolve
      );
    });
    rl.close();

    if (answer.toLowerCase() !== 'y' && answer.toLowerCase() !== 'yes') {
      console.log('Cancelled.');
      return;
    }
  }

  const client = new CircuitBreakerClient({
    baseUrl: parentOpts.apiUrl,
    apiKey: parentOpts.apiKey,
  });

  spinner.start(`Cancelling run ${runId}...`);

  try {
    await client.cancelRun(runId, options.reason);
    spinner.succeed(chalk.green(`Run ${runId} cancelled successfully`));

    if (options.reason) {
      console.log(chalk.dim(`Reason: ${options.reason}`));
    }
  } catch (error) {
    spinner.fail(chalk.red('Failed to cancel run'));
    if (error instanceof Error) {
      console.error(chalk.red(`Error: ${error.message}`));
    }
    process.exit(1);
  }
}

export default cancel;
