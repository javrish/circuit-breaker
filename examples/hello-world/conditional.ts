/**
 * Conditional Branching Workflow (XOR Pattern)
 *
 * Demonstrates conditional execution in Petri nets using guards.
 * This workflow checks a condition and branches to either a "success"
 * or "failure" path based on the result.
 *
 * Petri Net Structure:
 *
 *                     [guard: score >= 70]
 *                          ┌──▶ (pass) ──▶ [passed]
 *                          │
 *   [start] ──▶ (evaluate) ┤
 *      •                   │
 *                          └──▶ (fail) ──▶ [failed]
 *                     [guard: score < 70]
 *
 * Key Concepts:
 *   - XOR-split: Multiple transitions from same place with mutually exclusive guards
 *   - Guards: CEL expressions that control when a transition can fire
 *   - Only ONE branch will execute based on the condition
 *
 * Legend:
 *   [place]      = Place (state/condition)
 *   (transition) = Transition (action)
 *   •            = Initial token
 */

import { workflow } from '@circuit-breaker/core';

const conditionalWorkflow = workflow('conditional-example')
  .namespace('examples')
  .description('Demonstrates conditional branching with guard expressions')
  .labels({
    example: 'true',
    complexity: 'intermediate',
    pattern: 'xor-split',
  })

  // ============ Places (States) ============

  // Starting state
  .place('start', { initialTokens: 1 })

  // After evaluation - waiting for branching decision
  .place('evaluated')

  // Terminal states (mutually exclusive outcomes)
  .place('passed')
  .place('failed')

  // ============ Transitions (Actions) ============

  // Evaluate: compute a score that will determine the branch
  .transition('evaluate')
    .from('start')
    .to('evaluated')
    .script(`
      // Simulate evaluating something (e.g., test results, health check)
      const score = Math.floor(Math.random() * 100);
      console.log('Evaluation score:', score);

      // Return the score - this will be available in guard expressions
      return { score };
    `)
    .timeout('30s')
    .done()

  // Pass branch: fires only if score >= 70
  .transition('pass')
    .from('evaluated')
    .to('passed')
    .guard('ctx.score >= 70')
    .script(`
      console.log('✓ Passed! Score:', ctx.score);
      return {
        result: 'passed',
        score: ctx.score,
        message: 'Congratulations! You passed.'
      };
    `)
    .timeout('30s')
    .done()

  // Fail branch: fires only if score < 70
  .transition('fail')
    .from('evaluated')
    .to('failed')
    .guard('ctx.score < 70')
    .script(`
      console.log('✗ Failed. Score:', ctx.score);
      return {
        result: 'failed',
        score: ctx.score,
        message: 'Sorry, you did not pass. Try again!'
      };
    `)
    .timeout('30s')
    .done()

  .build();

// Export as default for CLI usage
export default conditionalWorkflow;

// Named export for programmatic usage
export { conditionalWorkflow };

/**
 * Execution Flow:
 *
 * 1. Initial state: start[•], evaluated[], passed[], failed[]
 *
 * 2. Evaluate fires (computes score, e.g., 85):
 *    start[], evaluated[•], passed[], failed[]
 *    Token data: { score: 85 }
 *
 * 3a. If score >= 70, "pass" guard is true, "pass" fires:
 *     start[], evaluated[], passed[•], failed[]
 *
 * 3b. If score < 70, "fail" guard is true, "fail" fires:
 *     start[], evaluated[], passed[], failed[•]
 *
 * 4. Workflow complete - token in one of the terminal places
 *
 * Note: Guards ensure mutual exclusion - only one transition can fire.
 * In Petri net theory, this is called a "conflict" that guards resolve.
 */
