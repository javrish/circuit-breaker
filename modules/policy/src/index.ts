/**
 * Policy module for OPA/Rego policy evaluation
 *
 * Evaluates policies against JSON input using conftest and OPA.
 * Designed to be composed with other modules in Circuit Breaker workflows.
 *
 * All functions accept either a Directory object or a path string for policies.
 * Use path strings for multi-step workflows where the policies directory
 * is specified as a host path.
 */
import { dag, Directory, object, func } from "@dagger.io/dagger";

@object()
export class Policy {
  /**
   * Get a Directory from either a Directory object or a path string
   */
  private getPoliciesDir(
    policies?: Directory,
    policiesPath?: string,
  ): Directory {
    if (policies) {
      return policies;
    }
    if (policiesPath) {
      return dag.address(policiesPath).directory();
    }
    throw new Error(
      "Must provide either --policies (Directory) or --policies-path (string)",
    );
  }

  /**
   * Evaluate OPA/Rego policy against JSON input using conftest
   *
   * Runs conftest in a container to evaluate policies.
   * Returns JSON with pass/fail status and any violations.
   *
   * @param input - JSON string to evaluate against policies
   * @param policies - Directory containing .rego policy files
   * @param policiesPath - Path to policies directory (alternative to policies)
   * @param namespace - Optional policy namespace to evaluate (default: all)
   */
  @func()
  async evaluate(
    input: string,
    policies?: Directory,
    policiesPath?: string,
    namespace?: string,
  ): Promise<string> {
    const policiesDir = this.getPoliciesDir(policies, policiesPath);
    const args = [
      "/usr/local/bin/conftest",
      "test",
      "/input.json",
      "--policy",
      "/policies",
      "--output",
      "json",
    ];

    if (namespace) {
      args.push("--namespace", namespace);
    }

    const result = await dag
      .container()
      .from("openpolicyagent/conftest:latest")
      .withDirectory("/policies", policiesDir)
      .withNewFile("/input.json", input)
      .withExec(args)
      .stdout();

    return this.parseConftestOutput(result);
  }

  /**
   * Evaluate policy and return a summary using OPA eval
   *
   * Useful for getting computed values from policies, not just pass/fail.
   *
   * @param input - JSON string to evaluate
   * @param policies - Directory containing .rego policy files
   * @param policiesPath - Path to policies directory (alternative to policies)
   * @param query - OPA query (e.g., "data.quality.summary")
   */
  @func()
  async query(
    input: string,
    policies?: Directory,
    policiesPath?: string,
    query?: string,
  ): Promise<string> {
    if (!query) {
      throw new Error("query parameter is required");
    }
    const policiesDir = this.getPoliciesDir(policies, policiesPath);
    const result = await dag
      .container()
      .from("openpolicyagent/opa:latest")
      .withDirectory("/policies", policiesDir)
      .withNewFile("/input.json", input)
      .withExec([
        "/opa",
        "eval",
        "--data",
        "/policies",
        "--input",
        "/input.json",
        "--format",
        "json",
        query,
      ])
      .stdout();

    try {
      const parsed = JSON.parse(result);
      if (parsed.result?.[0]?.expressions?.[0]?.value !== undefined) {
        return JSON.stringify(parsed.result[0].expressions[0].value);
      }
      return JSON.stringify(parsed);
    } catch {
      return result;
    }
  }

  /**
   * Check if input passes all policies (simple boolean result)
   *
   * @param input - JSON string to evaluate
   * @param policies - Directory containing .rego policy files
   * @param policiesPath - Path to policies directory (alternative to policies)
   */
  @func()
  async check(
    input: string,
    policies?: Directory,
    policiesPath?: string,
  ): Promise<boolean> {
    const result = await this.evaluate(input, policies, policiesPath);
    const parsed = JSON.parse(result);
    return parsed.passed === true;
  }

  /**
   * Parse conftest JSON output into a structured result
   */
  private parseConftestOutput(output: string): string {
    try {
      const parsed = JSON.parse(output);
      const failures: string[] = [];
      const warnings: string[] = [];
      const successes: string[] = [];

      if (Array.isArray(parsed)) {
        for (const item of parsed) {
          if (item.failures && Array.isArray(item.failures)) {
            for (const f of item.failures) {
              failures.push(f.msg || JSON.stringify(f));
            }
          }
          if (item.warnings && Array.isArray(item.warnings)) {
            for (const w of item.warnings) {
              warnings.push(w.msg || JSON.stringify(w));
            }
          }
          if (item.successes && Array.isArray(item.successes)) {
            for (const s of item.successes) {
              successes.push(s.msg || JSON.stringify(s));
            }
          }
        }
      }

      return JSON.stringify({
        passed: failures.length === 0,
        failures,
        warnings,
        successes,
        raw: parsed,
      });
    } catch {
      return JSON.stringify({
        passed: false,
        failures: ["Failed to parse conftest output"],
        warnings: [],
        successes: [],
        raw: output,
      });
    }
  }
}
