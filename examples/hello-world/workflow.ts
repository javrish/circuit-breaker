/**
 * Hello World Workflow
 *
 * A simple introductory workflow demonstrating Circuit Breaker basics.
 * This workflow prints "Hello" then "World" in sequence.
 *
 * Petri Net Structure:
 *
 *   [start] ──▶ (say-hello) ──▶ [hello-done] ──▶ (say-world) ──▶ [complete]
 *      •
 *
 * Legend:
 *   [place]      = Place (state/condition)
 *   (transition) = Transition (action)
 *   •            = Initial token
 */

// This workflow is defined as a plain object that matches the Circuit Breaker schema.
// It can be used with the SDK or directly as JSON.

const helloWorld = {
  version: "1.0" as const,
  name: "hello-world",
  namespace: "examples",
  metadata: {
    description:
      "A simple hello world workflow demonstrating sequential execution",
    labels: {
      example: "true",
      complexity: "beginner",
    },
  },

  // ============ Places (States) ============
  places: [
    // Starting state - has one token to kick off the workflow
    {
      id: "start",
      initialTokens: 1,
      capacity: null,
    },
    // Intermediate state after saying hello
    {
      id: "hello-done",
      initialTokens: 0,
      capacity: null,
    },
    // Final state - workflow complete
    {
      id: "complete",
      initialTokens: 0,
      capacity: null,
    },
  ],

  // ============ Transitions (Actions) ============
  transitions: [
    // First transition: say hello
    {
      id: "say-hello",
      inputs: [{ place: "start", weight: 1 }],
      outputs: [{ place: "hello-done", weight: 1 }],
      action: {
        type: "script" as const,
        runtime: "bun" as const,
        code: `
          publish('Hello');
        `,
      },
      timeout: "1m",
      retries: 0,
      retryBackoff: "exponential" as const,
      priority: 50,
    },
    // Second transition: say world
    {
      id: "say-world",
      inputs: [{ place: "hello-done", weight: 1 }],
      outputs: [{ place: "complete", weight: 1 }],
      action: {
        type: "script" as const,
        runtime: "bun" as const,
        code: `
          publish('World!');
        `,
      },
      timeout: "1m",
      retries: 0,
      retryBackoff: "exponential" as const,
      priority: 50,
    },
  ],
};

// Export as default for CLI usage
export default helloWorld;

// Named export for programmatic usage
export { helloWorld };

/**
 * You can run this workflow using the Circuit Breaker CLI:
 *
 *   cb validate ./workflow.ts
 *   cb visualize ./workflow.ts
 *   cb submit ./workflow.ts
 *   cb run ./workflow.ts --watch
 *
 * Or convert to JSON:
 *
 *   console.log(JSON.stringify(helloWorld, null, 2));
 */
