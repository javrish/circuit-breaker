/**
 * Simple OpenCode Example: Tell Me About Yourself
 *
 * This is a minimal workflow demonstrating how to use OpenCode AI agents
 * within Circuit Breaker. When submitted to the engine, the Runner will
 * execute the OpenCode task via Dagger in a container.
 *
 * Flow:
 *   [start] → (analyze) → [done]
 *      •
 *
 * Usage:
 *   cb submit examples/open-code/workflow.ts
 *   cb run examples/open-code/workflow.ts --watch
 */

import { workflow, opencode } from "@circuit-breaker/core";

/**
 * Simple workflow that asks OpenCode to analyze the project.
 *
 * The Runner (cb-runner) will:
 * 1. Pick up the enabled transition from NATS
 * 2. Execute the OpenCode task via Dagger
 * 3. Run the container with the prompt
 * 4. Publish the result back to NATS
 */
export default workflow("opencode-hello")
  .namespace("examples")
  .description("Simple OpenCode example: Tell me about this project")
  .labels({
    example: "true",
    complexity: "beginner",
    category: "ai",
  })

  // ============ Places (States) ============

  // Starting place with initial token
  .place("start", { initialTokens: 1 })

  // Final place - workflow complete
  .place("done")

  // ============ Transitions (Actions) ============

  // Ask OpenCode to analyze and describe itself
  .transition("analyze")
  .from("start")
  .to("done")
  .opencode(
    opencode(`
Please analyze this project and tell me about yourself!

Specifically:
1. What is the purpose of this project?
2. What programming languages and frameworks are being used?
3. What is the overall architecture?
4. What are the main components or modules?
5. Are there any interesting patterns or design decisions?

Keep your response concise but informative.
    `)
      .plan() // Read-only mode - won't modify any files
      .model("anthropic", "claude-sonnet-4-5-20250929")
      .timeout(300), // 5 minutes
    // Note: ANTHROPIC_API_KEY is read from the Runner's environment
    // and passed to the container automatically
  )
  .timeout("10m")
  .resources({ cpu: "500m", memory: "1Gi" })
  .done()

  .build();
