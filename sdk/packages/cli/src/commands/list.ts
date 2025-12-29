/**
 * List command - lists workflows or runs.
 *
 * @module
 */

import { CircuitBreakerClient } from '@circuit-breaker/core';

interface ListOptions {
  status?: string;
  label?: string[];
  limit?: string;
  offset?: string;
}

/**
 * Execute the list command.
 */
export async function list(
  type: string = 'workflows',
  options: ListOptions,
  command: { parent: { opts: () => { apiUrl?: string; apiKey?: string; output?: string } } }
): Promise<void> {
  const parentOpts = command.parent.opts();
  const client = new CircuitBreakerClient({
    baseUrl: parentOpts.apiUrl,
    apiKey: parentOpts.apiKey,
  });

  const limit = options.limit ? parseInt(options.limit, 10) : 20;
  const offset = options.offset ? parseInt(options.offset, 10) : 0;

  // Parse labels from key=value format
  const labels: Record<string, string> = {};
  if (options.label) {
    for (const label of options.label) {
      const [key, value] = label.split('=');
      if (key && value) {
        labels[key] = value;
      }
    }
  }

  try {
    if (type === 'runs' || type === 'run') {
      // List runs
      const result = await client.listRuns({
        status: options.status as any,
        limit,
        offset,
      });

      if (parentOpts.output === 'json') {
        console.log(JSON.stringify(result, null, 2));
        return;
      }

      console.log(`Runs (${result.total} total):\n`);
      if (result.runs.length === 0) {
        console.log('  No runs found.');
        return;
      }

      // Table header
      console.log(
        '  ' +
          'RUN ID'.padEnd(38) +
          'WORKFLOW'.padEnd(24) +
          'STATUS'.padEnd(12) +
          'STARTED'
      );
      console.log('  ' + '-'.repeat(90));

      for (const run of result.runs) {
        console.log(
          '  ' +
            run.runId.padEnd(38) +
            run.workflowName.substring(0, 22).padEnd(24) +
            run.status.padEnd(12) +
            run.startedAt
        );
      }

      if (result.total > offset + limit) {
        console.log(`\n  ... and ${result.total - offset - limit} more. Use --offset to paginate.`);
      }
    } else {
      // List workflows (default)
      const result = await client.listWorkflows({
        labels: Object.keys(labels).length > 0 ? labels : undefined,
        limit,
        offset,
      });

      if (parentOpts.output === 'json') {
        console.log(JSON.stringify(result, null, 2));
        return;
      }

      console.log(`Workflows (${result.total} total):\n`);
      if (result.workflows.length === 0) {
        console.log('  No workflows found.');
        return;
      }

      // Table header
      console.log(
        '  ' +
          'ID'.padEnd(38) +
          'NAME'.padEnd(24) +
          'NAMESPACE'.padEnd(14) +
          'CREATED'
      );
      console.log('  ' + '-'.repeat(90));

      for (const wf of result.workflows) {
        console.log(
          '  ' +
            wf.workflowId.padEnd(38) +
            wf.name.substring(0, 22).padEnd(24) +
            wf.namespace.padEnd(14) +
            wf.createdAt
        );
      }

      if (result.total > offset + limit) {
        console.log(`\n  ... and ${result.total - offset - limit} more. Use --offset to paginate.`);
      }
    }
  } catch (error) {
    console.error('Failed to list:', error instanceof Error ? error.message : error);
    process.exit(1);
  }
}

export default list;
