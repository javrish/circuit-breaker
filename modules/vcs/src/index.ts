/**
 * VCS module for unified source code operations
 *
 * Provides a unified interface for working with different version control systems
 * using URL scheme-based routing:
 *
 * - atomic-https://host/project  → Atomic clone via HTTPS
 * - atomic-ssh://host/project    → Atomic clone via SSH
 * - git-https://host/repo        → Git clone via HTTPS
 * - git-ssh://host/repo          → Git clone via SSH
 * - /absolute/path               → Local directory (requires --source)
 * - --source=/path/to/repo       → Local directory (no URL needed)
 *
 * Policy evaluation is handled by the Circuit Breaker runner, not this module.
 */
import { dag, Directory, object, func } from "@dagger.io/dagger";

type VcsType = "git" | "atomic" | "local" | "file" | "unknown";
type Protocol = "https" | "ssh";

interface ParsedUrl {
  vcs: VcsType;
  protocol: Protocol | null;
  url: string;
}

@object()
export class Vcs {
  /**
   * Parse a VCS URL scheme into components
   */
  private parseUrl(url: string): ParsedUrl {
    if (url.startsWith("atomic-https://")) {
      return {
        vcs: "atomic",
        protocol: "https",
        url: url.replace("atomic-https://", "https://"),
      };
    }
    if (url.startsWith("atomic-ssh://")) {
      return {
        vcs: "atomic",
        protocol: "ssh",
        url: url.replace("atomic-ssh://", "ssh://"),
      };
    }
    if (url.startsWith("git-https://")) {
      return {
        vcs: "git",
        protocol: "https",
        url: url.replace("git-https://", "https://"),
      };
    }
    if (url.startsWith("git-ssh://")) {
      return {
        vcs: "git",
        protocol: "ssh",
        url: url.replace("git-ssh://", "ssh://"),
      };
    }
    if (url.startsWith("file://")) {
      return { vcs: "file", protocol: null, url: url };
    }
    if (url.startsWith("/")) {
      return { vcs: "local", protocol: null, url: url };
    }
    return { vcs: "unknown", protocol: null, url: url };
  }

  /**
   * Clone a Git repository
   */
  @func()
  async git(url: string, ref?: string): Promise<Directory> {
    const repo = dag.git(url);
    return ref ? repo.ref(ref).tree() : repo.head().tree();
  }

  /**
   * Clone an Atomic repository
   */
  @func()
  async atomic(url: string, channel?: string): Promise<Directory> {
    return dag
      .container()
      .from("rust:1.75-slim")
      .withExec(["apt-get", "update"])
      .withExec(["apt-get", "install", "-y", "git", "pkg-config", "libssl-dev"])
      .withExec([
        "cargo",
        "install",
        "--git",
        "https://github.com/your-org/atomic",
        "atomic-cli",
      ])
      .withExec([
        "atomic",
        "clone",
        url,
        "/repo",
        "--channel",
        channel ?? "dev",
      ])
      .directory("/repo");
  }

  /**
   * Unified checkout using URL scheme routing
   *
   * URL schemes:
   * - atomic-https://host/project  → Atomic via HTTPS
   * - atomic-ssh://host/project    → Atomic via SSH
   * - git-https://host/repo        → Git via HTTPS
   * - git-ssh://host/repo          → Git via SSH
   *
   * For local directories, just pass --source (no URL needed)
   *
   * Examples:
   *   dagger call checkout --url=git-https://github.com/org/repo
   *   dagger call checkout --url=atomic-https://api.example.com/project --ref=main
   *   dagger call checkout --source=/path/to/local/repo
   */
  @func()
  async checkout(
    url?: string,
    ref?: string,
    source?: Directory,
  ): Promise<Directory> {
    if (source) {
      return source;
    }

    if (!url) {
      throw new Error(
        "Must provide either --url or --source. " +
          "Examples:\n" +
          "  dagger call checkout --url=git-https://github.com/org/repo\n" +
          "  dagger call checkout --source=/path/to/local/repo",
      );
    }

    const parsed = this.parseUrl(url);

    switch (parsed.vcs) {
      case "git":
        return this.git(parsed.url, ref);

      case "atomic":
        return this.atomic(parsed.url, ref);

      case "file":
        throw new Error(
          "file:// URLs are deprecated. Use --source instead:\n" +
            `  dagger call checkout --source=${url.replace("file://", "")}`,
        );

      case "local":
        if (!source) {
          throw new Error(
            `Local path "${url}" requires --source flag.\n` +
              `  dagger call checkout --url=${url} --source=${url}`,
          );
        }
        return source;

      default:
        throw new Error(
          `Unknown URL scheme: ${url}. ` +
            `Supported schemes: atomic-https://, atomic-ssh://, git-https://, git-ssh://, or absolute path`,
        );
    }
  }

  /**
   * Detect VCS type from a checked-out directory
   *
   * Returns "git", "atomic", or "unknown"
   */
  @func()
  async detect(source: Directory): Promise<string> {
    try {
      await source.directory(".git").entries();
      return "git";
    } catch {
      // .git doesn't exist
    }

    try {
      await source.directory(".atomic").entries();
      return "atomic";
    } catch {
      // .atomic doesn't exist
    }

    return "unknown";
  }

  /**
   * Get repository info from a checked-out directory
   */
  @func()
  async info(source: Directory): Promise<string> {
    const vcsType = await this.detect(source);

    if (vcsType === "git") {
      const result = await dag
        .container()
        .from("alpine/git:latest")
        .withDirectory("/repo", source)
        .withWorkdir("/repo")
        .withExec(["git", "log", "-1", "--format=%H|%s|%an|%ae|%ai"])
        .stdout();

      const [hash, subject, author, email, date] = result.trim().split("|");
      return JSON.stringify({ vcs: "git", hash, subject, author, email, date });
    }

    if (vcsType === "atomic") {
      const result = await dag
        .container()
        .from("alpine:latest")
        .withDirectory("/repo", source)
        .withWorkdir("/repo")
        .withExec(["cat", ".atomic/current_stack"])
        .stdout();

      return JSON.stringify({ vcs: "atomic", currentStack: result.trim() });
    }

    return JSON.stringify({ vcs: "unknown" });
  }

  /**
   * Run ESLint on source directory
   *
   * Returns JSON lint results for policy evaluation by the runner.
   * Requires the project to have an eslint.config.js or .eslintrc.* file.
   */
  @func()
  async lint(source: Directory): Promise<string> {
    return dag
      .container()
      .from("node:20-slim")
      .withDirectory("/src", source)
      .withWorkdir("/src")
      .withExec(["npm", "install"])
      .withExec(["npx", "eslint", ".", "--format", "json"])
      .stdout();
  }

  /**
   * Checkout and lint in one step
   *
   * Returns JSON with vcs type and lint results.
   * Policy evaluation should be done by the runner using .policy() on the transition.
   */
  @func()
  async checkoutAndLint(
    url?: string,
    ref?: string,
    source?: Directory,
  ): Promise<string> {
    const dir = await this.checkout(url, ref, source);
    const vcsType = await this.detect(dir);
    const lintResult = await this.lint(dir);

    return JSON.stringify({
      vcs: vcsType,
      lint: JSON.parse(lintResult),
    });
  }

  /**
   * Evaluate OPA/Rego policy against JSON input using conftest
   *
   * Runs conftest in a container to evaluate policies.
   * Returns JSON with pass/fail status and any violations.
   */
  @func()
  async evaluatePolicy(
    input: string,
    policies: Directory,
    query?: string,
  ): Promise<string> {
    // Run conftest to check policy pass/fail
    const container = dag
      .container()
      .from("openpolicyagent/conftest:latest")
      .withDirectory("/policies", policies)
      .withNewFile("/input.json", input);

    const result = await container
      .withExec([
        "/usr/local/bin/conftest",
        "test",
        "/input.json",
        "--policy",
        "/policies",
        "--output",
        "json",
      ])
      .stdout();

    // Also get the summary using OPA directly
    const summaryResult = await dag
      .container()
      .from("openpolicyagent/opa:latest")
      .withDirectory("/policies", policies)
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
        "data.quality.summary",
      ])
      .stdout();

    // Parse summary
    let summary: Record<string, unknown> = {};
    try {
      const summaryParsed = JSON.parse(summaryResult);
      if (summaryParsed.result?.[0]?.expressions?.[0]?.value) {
        summary = summaryParsed.result[0].expressions[0].value;
      }
    } catch {
      // Ignore summary parse errors
    }

    // Parse conftest output to determine pass/fail
    try {
      const parsed = JSON.parse(result);
      const failures: string[] = [];
      const warnings: string[] = [];

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
        }
      }

      return JSON.stringify({
        passed: failures.length === 0,
        failures,
        warnings,
        summary,
        raw: parsed,
      });
    } catch {
      return JSON.stringify({
        passed: false,
        failures: ["Failed to parse conftest output"],
        warnings: [],
        summary,
        raw: result,
      });
    }
  }

  /**
   * Full code quality workflow: checkout + lint + policy evaluation
   *
   * This is the complete pipeline that runs checkout, lint, and policy check
   * all within Dagger containers.
   */
  @func()
  async codeQuality(
    url?: string,
    ref?: string,
    source?: Directory,
    policies?: Directory,
  ): Promise<string> {
    const dir = await this.checkout(url, ref, source);
    const vcsType = await this.detect(dir);
    const lintResult = await this.lint(dir);

    const result: Record<string, unknown> = {
      vcs: vcsType,
      lint: JSON.parse(lintResult),
    };

    // Run policy check if policies directory provided
    if (policies) {
      const policyInput = JSON.stringify(result);
      const policyResult = await this.evaluatePolicy(policyInput, policies);
      result.policy = JSON.parse(policyResult);
    }

    return JSON.stringify(result);
  }
}
