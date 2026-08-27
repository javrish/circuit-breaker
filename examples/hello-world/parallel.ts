/**
 * Parallel Execution Workflow (Fork/Join Pattern)
 *
 * Demonstrates parallel execution in Petri nets using the fork/join pattern.
 * This workflow starts a task, fans out to three parallel tasks, then
 * synchronizes (joins) before completing.
 *
 * Petri Net Structure:
 *
 *                          ┌──▶ [task-a-done] ──┐
 *                          │                    │
 *   [start] ──▶ (fork) ────┼──▶ [task-b-done] ──┼──▶ (join) ──▶ [complete]
 *      •                   │                    │
 *                          └──▶ [task-c-done] ──┘
 *
 * Key Concepts:
 *   - Fork (AND-split): One transition with multiple output places
 *   - Join (AND-join): One transition with multiple input places
 *   - All parallel branches must complete before the join fires
 *
 * Legend:
 *   [place]      = Place (state/condition)
 *   (transition) = Transition (action)
 *   •            = Initial token
 */

import { workflow } from "@circuit-breaker/core";

const parallelWorkflow = workflow("parallel-example")
  .namespace("examples")
  .description("Demonstrates parallel execution with fork/join synchronization")
  .labels({
    example: "true",
    complexity: "intermediate",
    pattern: "fork-join",
  })

  // ============ Places (States) ============

  // Starting state
  .place("start", { initialTokens: 1 })

  // Parallel branch results (one place per branch)
  .place("task-a-done")
  .place("task-b-done")
  .place("task-c-done")

  // Final state after synchronization
  .place("complete")

  // ============ Transitions (Actions) ============

  // Fork: single input, multiple outputs (AND-split)
  // When this fires, it produces a token in EACH output place
  .transition("fork")
  .from("start")
  .to("task-a-done", "task-b-done", "task-c-done")
  .script(
    `
      publish('Forking into parallel tasks...');
      return { forkedAt: new Date().toISOString() };
    `,
  )
  .timeout("30s")
  .done()

  // Note: In a real workflow, you'd have separate transitions for each branch:
  //
  //   .transition('task-a')
  //     .from('ready-a')
  //     .to('task-a-done')
  //     .dagger('./tasks', 'taskA')
  //     .done()
  //
  // But for this simple example, the fork directly produces "done" tokens.

  // Join: multiple inputs, single output (AND-join)
  // This transition ONLY fires when ALL input places have tokens
  .transition("join")
  .from("task-a-done", "task-b-done", "task-c-done")
  .to("complete")
  .script(
    `
      publish('All parallel tasks complete! Joining...');
      return {
        joinedAt: new Date().toISOString(),
        message: 'All branches synchronized successfully'
      };
    `,
  )
  .timeout("30s")
  .done()

  .build();

// Export as default for CLI usage
export default parallelWorkflow;

// Named export for programmatic usage
export { parallelWorkflow };

/**
 * Execution Flow:
 *
 * 1. Initial state: start[•], task-a-done[], task-b-done[], task-c-done[], complete[]
 *
 * 2. Fork fires (consumes token from start, produces 3 tokens):
 *    start[], task-a-done[•], task-b-done[•], task-c-done[•], complete[]
 *
 * 3. Join fires (consumes all 3 tokens, produces 1):
 *    start[], task-a-done[], task-b-done[], task-c-done[], complete[•]
 *
 * 4. Workflow complete - token in terminal place
 */
