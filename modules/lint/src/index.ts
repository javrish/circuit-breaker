/**
 * Lint module for code quality checks
 *
 * Provides linting operations for various languages and tools:
 * - ESLint for JavaScript/TypeScript
 * - Additional linters can be added (Biome, Prettier, etc.)
 *
 * All functions accept either a Directory object or a path string.
 * Use path strings for multi-step workflows where the source was
 * exported by a previous step (e.g., vcs.checkoutToPath).
 */
import { dag, Directory, object, func } from "@dagger.io/dagger";

@object()
export class Lint {
  /**
   * Get a Directory from either a Directory object or a path string
   */
  private getDirectory(source?: Directory, path?: string): Directory {
    if (source) {
      return source;
    }
    if (path) {
      return dag.address(path).directory();
    }
    throw new Error(
      "Must provide either --source (Directory) or --path (string)",
    );
  }

  /**
   * Run ESLint on source directory
   *
   * Returns JSON lint results. Requires the project to have
   * an eslint.config.js or .eslintrc.* file.
   *
   * @param source - Directory containing the source code
   * @param path - Path to source directory (alternative to source)
   * @param fix - Whether to auto-fix problems (default: false)
   * @param extensions - File extensions to lint (default: .js,.jsx,.ts,.tsx)
   */
  @func()
  async eslint(
    source?: Directory,
    path?: string,
    fix?: boolean,
    extensions?: string,
  ): Promise<string> {
    const dir = this.getDirectory(source, path);
    const args = ["npx", "eslint", ".", "--format", "json"];

    if (fix) {
      args.push("--fix");
    }

    if (extensions) {
      args.push("--ext", extensions);
    }

    const result = await dag
      .container()
      .from("node:20-slim")
      .withDirectory("/src", dir)
      .withWorkdir("/src")
      .withExec(["npm", "install"])
      .withExec(args)
      .stdout();

    // Wrap the result in an object for easier token reference in workflows
    // This allows subsequent steps to use ctx.token.lint
    try {
      const parsed = JSON.parse(result);
      return JSON.stringify({ lint: parsed });
    } catch {
      return JSON.stringify({ lint: result, parseError: true });
    }
  }

  /**
   * Run Biome for linting and formatting
   *
   * @param source - Directory containing the source code
   * @param path - Path to source directory (alternative to source)
   * @param fix - Whether to apply safe fixes (default: false)
   */
  @func()
  async biome(
    source?: Directory,
    path?: string,
    fix?: boolean,
  ): Promise<string> {
    const dir = this.getDirectory(source, path);
    const args = ["npx", "@biomejs/biome", "check", "--reporter", "json"];

    if (fix) {
      args.push("--apply");
    }

    args.push(".");

    return dag
      .container()
      .from("node:20-slim")
      .withDirectory("/src", dir)
      .withWorkdir("/src")
      .withExec(["npm", "install"])
      .withExec(args)
      .stdout();
  }

  /**
   * Run Prettier check
   *
   * @param source - Directory containing the source code
   * @param path - Path to source directory (alternative to source)
   * @param fix - Whether to write fixes (default: false)
   */
  @func()
  async prettier(
    source?: Directory,
    path?: string,
    fix?: boolean,
  ): Promise<string> {
    const dir = this.getDirectory(source, path);
    const action = fix ? "--write" : "--check";

    return dag
      .container()
      .from("node:20-slim")
      .withDirectory("/src", dir)
      .withWorkdir("/src")
      .withExec(["npm", "install"])
      .withExec(["npx", "prettier", action, "."])
      .stdout();
  }

  /**
   * Run TypeScript type checking
   *
   * @param source - Directory containing the source code
   * @param path - Path to source directory (alternative to source)
   */
  @func()
  async typescript(source?: Directory, path?: string): Promise<string> {
    const dir = this.getDirectory(source, path);

    return dag
      .container()
      .from("node:20-slim")
      .withDirectory("/src", dir)
      .withWorkdir("/src")
      .withExec(["npm", "install"])
      .withExec(["npx", "tsc", "--noEmit", "--pretty", "false"])
      .stdout();
  }
}
