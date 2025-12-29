/**
 * Workflow validation utilities for Circuit Breaker.
 *
 * Provides structural and semantic validation of Petri-net workflows,
 * including deadlock detection and reachability analysis.
 *
 * @module
 */

import { WorkflowSchema, type Workflow, type Place, type Transition } from './schema';
import { z } from 'zod';

/**
 * Validation error with detailed information.
 */
export class ValidationError extends Error {
  constructor(
    message: string,
    public readonly code: ValidationErrorCode,
    public readonly details?: Record<string, unknown>
  ) {
    super(message);
    this.name = 'ValidationError';
  }
}

/**
 * Error codes for validation failures.
 */
export type ValidationErrorCode =
  | 'INVALID_SCHEMA'
  | 'DUPLICATE_PLACE_ID'
  | 'DUPLICATE_TRANSITION_ID'
  | 'INVALID_ARC_REFERENCE'
  | 'UNREACHABLE_PLACE'
  | 'DEAD_TRANSITION'
  | 'POTENTIAL_DEADLOCK'
  | 'NO_INITIAL_MARKING'
  | 'NO_TERMINAL_PLACE'
  | 'INVALID_GUARD_EXPRESSION'
  | 'INVALID_RESOURCE_SPEC';

/**
 * Result of workflow validation.
 */
export interface ValidationResult {
  valid: boolean;
  errors: ValidationError[];
  warnings: ValidationWarning[];
}

/**
 * Non-fatal validation warning.
 */
export interface ValidationWarning {
  code: string;
  message: string;
  details?: Record<string, unknown>;
}

/**
 * Options for workflow validation.
 */
export interface ValidationOptions {
  /** Check for potential deadlocks (expensive for large nets) */
  checkDeadlocks?: boolean;
  /** Check for unreachable places */
  checkReachability?: boolean;
  /** Validate CEL guard expressions (requires CEL parser) */
  validateGuards?: boolean;
  /** Strict mode: treat warnings as errors */
  strict?: boolean;
}

const DEFAULT_OPTIONS: ValidationOptions = {
  checkDeadlocks: true,
  checkReachability: true,
  validateGuards: false,
  strict: false,
};

/**
 * Validate a workflow definition.
 *
 * @param workflow - The workflow to validate (can be unknown for schema validation)
 * @param options - Validation options
 * @returns Validation result with errors and warnings
 */
export function validateWorkflow(
  workflow: unknown,
  options: ValidationOptions = {}
): ValidationResult {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  const errors: ValidationError[] = [];
  const warnings: ValidationWarning[] = [];

  // 1. Schema validation
  const schemaResult = WorkflowSchema.safeParse(workflow);
  if (!schemaResult.success) {
    const zodError = schemaResult.error;
    for (const issue of zodError.issues) {
      errors.push(
        new ValidationError(
          `Schema validation failed at ${issue.path.join('.')}: ${issue.message}`,
          'INVALID_SCHEMA',
          { path: issue.path, code: issue.code }
        )
      );
    }
    // Can't continue with semantic validation if schema is invalid
    return { valid: false, errors, warnings };
  }

  const validWorkflow = schemaResult.data;

  // 2. Check for duplicate IDs (should be caught by schema, but double-check)
  const placeIds = new Set<string>();
  for (const place of validWorkflow.places) {
    if (placeIds.has(place.id)) {
      errors.push(
        new ValidationError(`Duplicate place ID: ${place.id}`, 'DUPLICATE_PLACE_ID', {
          placeId: place.id,
        })
      );
    }
    placeIds.add(place.id);
  }

  const transitionIds = new Set<string>();
  for (const transition of validWorkflow.transitions) {
    if (transitionIds.has(transition.id)) {
      errors.push(
        new ValidationError(
          `Duplicate transition ID: ${transition.id}`,
          'DUPLICATE_TRANSITION_ID',
          { transitionId: transition.id }
        )
      );
    }
    transitionIds.add(transition.id);
  }

  // 3. Validate arc references
  for (const transition of validWorkflow.transitions) {
    for (const input of transition.inputs) {
      if (!placeIds.has(input.place)) {
        errors.push(
          new ValidationError(
            `Transition '${transition.id}' references non-existent input place '${input.place}'`,
            'INVALID_ARC_REFERENCE',
            { transitionId: transition.id, placeId: input.place, direction: 'input' }
          )
        );
      }
    }
    for (const output of transition.outputs) {
      if (!placeIds.has(output.place)) {
        errors.push(
          new ValidationError(
            `Transition '${transition.id}' references non-existent output place '${output.place}'`,
            'INVALID_ARC_REFERENCE',
            { transitionId: transition.id, placeId: output.place, direction: 'output' }
          )
        );
      }
    }
  }

  // 4. Check for initial marking
  const hasInitialTokens = validWorkflow.places.some((p) => p.initialTokens > 0);
  if (!hasInitialTokens) {
    errors.push(
      new ValidationError(
        'Workflow has no initial marking (no places with initialTokens > 0)',
        'NO_INITIAL_MARKING'
      )
    );
  }

  // 5. Check for terminal places (places with no outgoing transitions)
  if (opts.checkReachability) {
    const placesWithOutgoing = new Set<string>();
    for (const transition of validWorkflow.transitions) {
      for (const input of transition.inputs) {
        placesWithOutgoing.add(input.place);
      }
    }

    const terminalPlaces = validWorkflow.places.filter((p) => !placesWithOutgoing.has(p.id));
    if (terminalPlaces.length === 0) {
      warnings.push({
        code: 'NO_TERMINAL_PLACE',
        message: 'Workflow has no terminal places (all places have outgoing transitions)',
        details: { hint: 'This may indicate an infinite loop or missing end state' },
      });
    }
  }

  // 6. Check for unreachable places
  if (opts.checkReachability) {
    const reachable = computeReachablePlaces(validWorkflow);
    for (const place of validWorkflow.places) {
      if (!reachable.has(place.id)) {
        warnings.push({
          code: 'UNREACHABLE_PLACE',
          message: `Place '${place.id}' is not reachable from any initial place`,
          details: { placeId: place.id },
        });
      }
    }
  }

  // 7. Check for dead transitions (transitions that can never fire)
  if (opts.checkReachability) {
    const deadTransitions = findDeadTransitions(validWorkflow);
    for (const transitionId of deadTransitions) {
      warnings.push({
        code: 'DEAD_TRANSITION',
        message: `Transition '${transitionId}' may never fire (no reachable input marking)`,
        details: { transitionId },
      });
    }
  }

  // 8. Basic deadlock detection (simplified - full analysis is computationally expensive)
  if (opts.checkDeadlocks) {
    const potentialDeadlocks = detectPotentialDeadlocks(validWorkflow);
    for (const deadlock of potentialDeadlocks) {
      warnings.push({
        code: 'POTENTIAL_DEADLOCK',
        message: deadlock.message,
        details: deadlock.details,
      });
    }
  }

  // If strict mode, promote warnings to errors
  if (opts.strict) {
    for (const warning of warnings) {
      errors.push(new ValidationError(warning.message, warning.code as ValidationErrorCode, warning.details));
    }
    return { valid: errors.length === 0, errors, warnings: [] };
  }

  return {
    valid: errors.length === 0,
    errors,
    warnings,
  };
}

/**
 * Compute the set of places reachable from initial places via BFS.
 */
function computeReachablePlaces(workflow: Workflow): Set<string> {
  const reachable = new Set<string>();

  // Start from places with initial tokens
  const queue: string[] = [];
  for (const place of workflow.places) {
    if (place.initialTokens > 0) {
      reachable.add(place.id);
      queue.push(place.id);
    }
  }

  // Build adjacency: place -> transitions that consume from it -> places they produce to
  const placeToTransitions = new Map<string, Transition[]>();
  for (const transition of workflow.transitions) {
    for (const input of transition.inputs) {
      const list = placeToTransitions.get(input.place) ?? [];
      list.push(transition);
      placeToTransitions.set(input.place, list);
    }
  }

  // BFS
  while (queue.length > 0) {
    const placeId = queue.shift()!;
    const transitions = placeToTransitions.get(placeId) ?? [];

    for (const transition of transitions) {
      // Simplified: assume transition can fire if we've reached any input place
      // Full analysis would require considering all input places simultaneously
      for (const output of transition.outputs) {
        if (!reachable.has(output.place)) {
          reachable.add(output.place);
          queue.push(output.place);
        }
      }
    }
  }

  return reachable;
}

/**
 * Find transitions that can never fire because their input places are unreachable.
 */
function findDeadTransitions(workflow: Workflow): string[] {
  const reachable = computeReachablePlaces(workflow);
  const deadTransitions: string[] = [];

  for (const transition of workflow.transitions) {
    // A transition is dead if any of its required input places is unreachable
    const hasUnreachableInput = transition.inputs.some((input) => !reachable.has(input.place));
    if (hasUnreachableInput) {
      deadTransitions.push(transition.id);
    }
  }

  return deadTransitions;
}

/**
 * Detect potential deadlock patterns in the workflow.
 * This is a simplified heuristic analysis, not full state-space exploration.
 */
function detectPotentialDeadlocks(
  workflow: Workflow
): Array<{ message: string; details: Record<string, unknown> }> {
  const issues: Array<{ message: string; details: Record<string, unknown> }> = [];

  // Pattern 1: AND-join with unbalanced paths
  // If a transition requires tokens from multiple places, check if those places
  // can all be reached from the same initial state
  for (const transition of workflow.transitions) {
    if (transition.inputs.length > 1) {
      // This is an AND-join (requires tokens from multiple places)
      const inputPlaceIds = transition.inputs.map((i) => i.place);

      // Check if all inputs are produced by different transitions
      // that don't share a common predecessor - this could indicate
      // a synchronization issue
      const producers = new Map<string, string[]>();
      for (const t of workflow.transitions) {
        for (const output of t.outputs) {
          if (inputPlaceIds.includes(output.place)) {
            const list = producers.get(output.place) ?? [];
            list.push(t.id);
            producers.set(output.place, list);
          }
        }
      }

      // If any input place has no producer and no initial tokens, deadlock is guaranteed
      for (const inputPlace of inputPlaceIds) {
        const placeData = workflow.places.find((p) => p.id === inputPlace);
        const placeProducers = producers.get(inputPlace) ?? [];

        if (placeProducers.length === 0 && (!placeData || placeData.initialTokens === 0)) {
          issues.push({
            message: `Potential deadlock: AND-join '${transition.id}' requires tokens from '${inputPlace}' but nothing produces them`,
            details: {
              transitionId: transition.id,
              inputPlaces: inputPlaceIds,
              missingProducer: inputPlace,
            },
          });
        }
      }
    }
  }

  // Pattern 2: Circular dependencies without initial tokens
  const cycles = findCycles(workflow);
  for (const cycle of cycles) {
    // Check if any place in the cycle has initial tokens
    const hasInitialTokens = cycle.some((placeId) => {
      const place = workflow.places.find((p) => p.id === placeId);
      return place && place.initialTokens > 0;
    });

    if (!hasInitialTokens) {
      issues.push({
        message: `Potential deadlock: cycle detected with no initial tokens: ${cycle.join(' -> ')}`,
        details: { cycle },
      });
    }
  }

  return issues;
}

/**
 * Find cycles in the Petri net graph (simplified).
 */
function findCycles(workflow: Workflow): string[][] {
  const cycles: string[][] = [];

  // Build graph: place -> places reachable via one transition
  const graph = new Map<string, Set<string>>();
  for (const place of workflow.places) {
    graph.set(place.id, new Set());
  }

  for (const transition of workflow.transitions) {
    for (const input of transition.inputs) {
      for (const output of transition.outputs) {
        const edges = graph.get(input.place);
        if (edges) {
          edges.add(output.place);
        }
      }
    }
  }

  // DFS for cycle detection
  const visited = new Set<string>();
  const recStack = new Set<string>();
  const path: string[] = [];

  function dfs(node: string): boolean {
    visited.add(node);
    recStack.add(node);
    path.push(node);

    const neighbors = graph.get(node) ?? new Set();
    for (const neighbor of neighbors) {
      if (!visited.has(neighbor)) {
        if (dfs(neighbor)) {
          return true;
        }
      } else if (recStack.has(neighbor)) {
        // Found cycle
        const cycleStart = path.indexOf(neighbor);
        const cycle = path.slice(cycleStart);
        cycle.push(neighbor); // Close the cycle
        cycles.push(cycle);
      }
    }

    path.pop();
    recStack.delete(node);
    return false;
  }

  for (const place of workflow.places) {
    if (!visited.has(place.id)) {
      dfs(place.id);
    }
  }

  return cycles;
}

/**
 * Quick validation check - returns true if valid, false otherwise.
 */
export function isValidWorkflow(workflow: unknown): workflow is Workflow {
  return validateWorkflow(workflow, { checkDeadlocks: false, checkReachability: false }).valid;
}

/**
 * Assert that a workflow is valid, throwing if not.
 */
export function assertValidWorkflow(workflow: unknown, options?: ValidationOptions): asserts workflow is Workflow {
  const result = validateWorkflow(workflow, options);
  if (!result.valid) {
    const messages = result.errors.map((e) => e.message).join('\n');
    throw new Error(`Workflow validation failed:\n${messages}`);
  }
}
