/**
 * Visualization utilities for Circuit Breaker workflows.
 *
 * Generates DOT (Graphviz) and Mermaid diagram representations of Petri nets.
 *
 * @module
 */

import type { Workflow, Place, Transition } from './schema';

/**
 * Options for visualization output.
 */
export interface VisualizationOptions {
  /** Output format */
  format?: 'dot' | 'mermaid';
  /** Include token counts in place labels */
  showTokens?: boolean;
  /** Include resource requirements in transition labels */
  showResources?: boolean;
  /** Include guard conditions in transition labels */
  showGuards?: boolean;
  /** Graph direction for DOT (TB = top-bottom, LR = left-right) */
  direction?: 'TB' | 'LR' | 'BT' | 'RL';
  /** Custom colors for places */
  placeColor?: string;
  /** Custom colors for transitions */
  transitionColor?: string;
  /** Title for the diagram */
  title?: string;
}

const defaultOptions: Required<VisualizationOptions> = {
  format: 'dot',
  showTokens: true,
  showResources: false,
  showGuards: true,
  direction: 'TB',
  placeColor: '#E3F2FD',
  transitionColor: '#FFF3E0',
  title: '',
};

/**
 * Generate a visualization of the workflow Petri net.
 *
 * @param workflow - The workflow to visualize
 * @param options - Visualization options
 * @returns String representation in the specified format
 */
export function visualize(workflow: Workflow, options: VisualizationOptions = {}): string {
  const opts = { ...defaultOptions, ...options };

  if (opts.format === 'mermaid') {
    return generateMermaid(workflow, opts);
  }

  return generateDot(workflow, opts);
}

/**
 * Generate DOT (Graphviz) representation.
 */
function generateDot(workflow: Workflow, options: Required<VisualizationOptions>): string {
  const lines: string[] = [];

  lines.push(`digraph "${workflow.name}" {`);
  lines.push(`  rankdir=${options.direction};`);
  lines.push('  node [fontname="Arial"];');
  lines.push('  edge [fontname="Arial"];');

  if (options.title || workflow.metadata?.description) {
    const title = options.title || workflow.metadata?.description || '';
    lines.push(`  labelloc="t";`);
    lines.push(`  label="${escapeString(title)}";`);
  }

  lines.push('');
  lines.push('  // Places (circles)');
  lines.push('  node [shape=circle, style=filled, fillcolor="' + options.placeColor + '"];');

  for (const place of workflow.places) {
    const label = formatPlaceLabel(place, options);
    lines.push(`  "${place.id}" [label="${escapeString(label)}"];`);
  }

  lines.push('');
  lines.push('  // Transitions (rectangles)');
  lines.push(
    '  node [shape=box, style="filled,rounded", fillcolor="' + options.transitionColor + '"];'
  );

  for (const transition of workflow.transitions) {
    const label = formatTransitionLabel(transition, options);
    lines.push(`  "${transition.id}" [label="${escapeString(label)}"];`);
  }

  lines.push('');
  lines.push('  // Arcs');

  for (const transition of workflow.transitions) {
    // Input arcs (place -> transition)
    for (const input of transition.inputs) {
      const weight = input.weight > 1 ? ` [label="${input.weight}"]` : '';
      lines.push(`  "${input.place}" -> "${transition.id}"${weight};`);
    }

    // Output arcs (transition -> place)
    for (const output of transition.outputs) {
      const weight = output.weight > 1 ? ` [label="${output.weight}"]` : '';
      lines.push(`  "${transition.id}" -> "${output.place}"${weight};`);
    }
  }

  lines.push('}');

  return lines.join('\n');
}

/**
 * Generate Mermaid diagram representation.
 */
function generateMermaid(workflow: Workflow, options: Required<VisualizationOptions>): string {
  const lines: string[] = [];

  const direction = options.direction === 'LR' ? 'LR' : 'TD';
  lines.push(`flowchart ${direction}`);

  if (options.title || workflow.metadata?.description) {
    const title = options.title || workflow.metadata?.description || '';
    lines.push(`  %% ${title}`);
  }

  lines.push('');
  lines.push('  %% Places (circles)');

  for (const place of workflow.places) {
    const label = formatPlaceLabel(place, options);
    lines.push(`  ${place.id}((${escapeMermaid(label)}))`);
  }

  lines.push('');
  lines.push('  %% Transitions (rectangles)');

  for (const transition of workflow.transitions) {
    const label = formatTransitionLabel(transition, options);
    lines.push(`  ${transition.id}[${escapeMermaid(label)}]`);
  }

  lines.push('');
  lines.push('  %% Arcs');

  for (const transition of workflow.transitions) {
    // Input arcs (place -> transition)
    for (const input of transition.inputs) {
      if (input.weight > 1) {
        lines.push(`  ${input.place} -->|${input.weight}| ${transition.id}`);
      } else {
        lines.push(`  ${input.place} --> ${transition.id}`);
      }
    }

    // Output arcs (transition -> place)
    for (const output of transition.outputs) {
      if (output.weight > 1) {
        lines.push(`  ${transition.id} -->|${output.weight}| ${output.place}`);
      } else {
        lines.push(`  ${transition.id} --> ${output.place}`);
      }
    }
  }

  // Add styling
  lines.push('');
  lines.push('  %% Styling');
  lines.push(`  classDef place fill:${options.placeColor},stroke:#1976D2`);
  lines.push(`  classDef transition fill:${options.transitionColor},stroke:#F57C00`);

  const placeIds = workflow.places.map((p) => p.id).join(',');
  const transitionIds = workflow.transitions.map((t) => t.id).join(',');

  lines.push(`  class ${placeIds} place`);
  lines.push(`  class ${transitionIds} transition`);

  return lines.join('\n');
}

/**
 * Format a place label based on options.
 */
function formatPlaceLabel(place: Place, options: Required<VisualizationOptions>): string {
  let label = place.id;

  if (options.showTokens && place.initialTokens > 0) {
    label += `\\n[${place.initialTokens}]`;
  }

  if (place.capacity !== null && place.capacity !== undefined) {
    label += `\\n(cap: ${place.capacity})`;
  }

  return label;
}

/**
 * Format a transition label based on options.
 */
function formatTransitionLabel(
  transition: Transition,
  options: Required<VisualizationOptions>
): string {
  const parts: string[] = [transition.id];

  // Show action type
  if (transition.action) {
    parts.push(`[${transition.action.type}]`);
  }

  // Show guard condition
  if (options.showGuards && transition.guard) {
    const shortGuard =
      transition.guard.length > 30 ? transition.guard.substring(0, 27) + '...' : transition.guard;
    parts.push(`guard: ${shortGuard}`);
  }

  // Show resources
  if (options.showResources && transition.resources) {
    const resources: string[] = [];
    if (transition.resources.cpu) resources.push(`cpu:${transition.resources.cpu}`);
    if (transition.resources.memory) resources.push(`mem:${transition.resources.memory}`);
    if (resources.length > 0) {
      parts.push(resources.join(', '));
    }
  }

  return parts.join('\\n');
}

/**
 * Escape special characters for DOT strings.
 */
function escapeString(str: string): string {
  return str
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\n/g, '\\n');
}

/**
 * Escape special characters for Mermaid strings.
 */
function escapeMermaid(str: string): string {
  return str
    .replace(/"/g, "'")
    .replace(/\[/g, '(')
    .replace(/\]/g, ')')
    .replace(/\\n/g, '<br/>');
}

/**
 * Generate a URL to visualize the workflow using Graphviz online.
 */
export function getGraphvizUrl(workflow: Workflow, options: VisualizationOptions = {}): string {
  const dot = visualize(workflow, { ...options, format: 'dot' });
  const encoded = encodeURIComponent(dot);
  return `https://dreampuf.github.io/GraphvizOnline/#${encoded}`;
}

/**
 * Generate a URL to visualize the workflow using Mermaid.live.
 */
export function getMermaidUrl(workflow: Workflow, options: VisualizationOptions = {}): string {
  const mermaid = visualize(workflow, { ...options, format: 'mermaid' });
  // Mermaid.live uses base64 encoded JSON
  const state = {
    code: mermaid,
    mermaid: { theme: 'default' },
    autoSync: true,
    updateDiagram: true,
  };
  const encoded = btoa(JSON.stringify(state));
  return `https://mermaid.live/edit#base64:${encoded}`;
}
