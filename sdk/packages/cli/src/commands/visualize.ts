/**
 * Visualize command - generates visual representations of workflows.
 *
 * @module
 */

import { resolve } from 'path';
import { WorkflowSchema, visualize as generateVisualization, getGraphvizUrl, getMermaidUrl } from '@circuit-breaker/core';
import type { Workflow } from '@circuit-breaker/core';

interface VisualizeOptions {
  format?: 'dot' | 'mermaid';
  output?: string;
  open?: boolean;
  showTokens?: boolean;
  showGuards?: boolean;
  showResources?: boolean;
}

/**
 * Load a workflow from file (shared utility - should be moved to common module).
 */
async function loadWorkflow(filePath: string): Promise<Workflow> {
  const absolutePath = resolve(process.cwd(), filePath);
  const ext = filePath.split('.').pop()?.toLowerCase();

  if (ext === 'json') {
    const file = Bun.file(absolutePath);
    const content = await file.json();
    return WorkflowSchema.parse(content);
  }

  if (ext === 'ts' || ext === 'js' || ext === 'mts' || ext === 'mjs') {
    const module = await import(absolutePath);
    const workflow = module.default ?? module.workflow;

    if (!workflow) {
      throw new Error(
        `No workflow found in ${filePath}. Export your workflow as default or named 'workflow'.`
      );
    }

    if (typeof workflow.build === 'function') {
      return workflow.build();
    }

    return WorkflowSchema.parse(workflow);
  }

  throw new Error(`Unsupported file extension: ${ext}. Use .ts, .js, or .json`);
}

/**
 * Execute the visualize command.
 */
export async function visualize(workflowPath: string, options: VisualizeOptions): Promise<void> {
  console.log(`Loading workflow from ${workflowPath}...`);

  let workflow: Workflow;
  try {
    workflow = await loadWorkflow(workflowPath);
  } catch (error) {
    if (error instanceof Error) {
      console.error(`Failed to load workflow: ${error.message}`);
    }
    process.exit(1);
  }

  const format = options.format ?? 'dot';

  const output = generateVisualization(workflow, {
    format,
    showTokens: options.showTokens ?? true,
    showGuards: options.showGuards ?? true,
    showResources: options.showResources ?? false,
  });

  // Write to file or stdout
  if (options.output) {
    await Bun.write(options.output, output);
    console.log(`Visualization written to ${options.output}`);
  } else {
    console.log(output);
  }

  // Open in browser if requested
  if (options.open) {
    const url = format === 'mermaid'
      ? getMermaidUrl(workflow, { format: 'mermaid' })
      : getGraphvizUrl(workflow, { format: 'dot' });

    console.log(`\nOpening in browser: ${url}`);

    // Use Bun's spawn to open URL
    const opener = process.platform === 'darwin' ? 'open' :
                   process.platform === 'win32' ? 'start' : 'xdg-open';

    Bun.spawn([opener, url]);
  }
}

export default visualize;
