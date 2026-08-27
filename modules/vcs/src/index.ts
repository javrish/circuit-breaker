/**
 * VCS module for unified source code operations
 *
 * Provides a unified interface for working with different version control systems
 * using URL scheme-based routing:
 *
 * - /absolute/path               → Local directory (passed via --source flag)
 * - git-https://host/repo        → Git clone via HTTPS
 * - git-ssh://host/repo          → Git clone via SSH
 * - atomic-https://host/project  → Atomic clone via HTTPS
 * - atomic-ssh://host/project    → Atomic clone via SSH
 *
 * Usage with Circuit Breaker:
 *
 *   Local:
 *     ./cb inject <run-id> start --data '{"url": "/path/to/repo"}'
 *     # The path is returned and subsequent steps use --source to mount it
 *
 *   Remote:
 *     ./cb inject <run-id> start --data '{"url": "git-https://github.com/org/repo"}'
 *     # Remote repos are cloned within Dagger, no exportPath needed
 *
 * Note: For local paths, Dagger CLI's --source flag automatically mounts
 * host directories. The workflow passes paths between steps, and each
 * dagger call uses --source=<path> to mount the directory.
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
      return { vcs: "file", protocol: null, url };
    }
    if (url.startsWith("/")) {
      return { vcs: "local", protocol: null, url };
    }
    return { vcs: "unknown", protocol: null, url };
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
   *
   * Returns JSON with VCS-specific metadata:
   * - Git: hash, subject, author, email, date
   * - Atomic: currentStack
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
   * Checkout and return path for multi-step workflows
   *
   * For local directories: Returns the path directly. Subsequent steps
   * use Dagger's --source flag to mount the host directory.
   *
   * For remote URLs: Clones within Dagger and returns info. The checkout
   * is passed via --source to subsequent steps within the same session,
   * or use the lower-level checkout() function for Directory chaining.
   *
   * @param url - VCS URL (git-https://, atomic-https://) or local path (/path/to/repo)
   * @param ref - Optional branch, tag, or channel
   * @param source - Optional local directory (for Dagger Directory input)
   *
   * @returns JSON with { vcs, path, info }
   *
   * @example Local directory:
   *   dagger call checkout-to-path --url=/path/to/local/repo
   *
   * @example With source flag (preferred for local):
   *   dagger call checkout-to-path --source=/path/to/local/repo
   *
   * @example Remote git repo:
   *   dagger call checkout-to-path --url=git-https://github.com/org/repo
   */
  @func()
  async checkoutToPath(
    url?: string,
    ref?: string,
    source?: Directory,
  ): Promise<string> {
    // If source provided directly, use it
    if (source) {
      const vcs = await this.detect(source);
      let info: Record<string, string> = {};
      try {
        const infoJson = await this.info(source);
        info = JSON.parse(infoJson);
      } catch {
        // Ignore info errors
      }
      // Return the url as path if provided, otherwise indicate source was used
      return JSON.stringify({ vcs, path: url ?? "(source)", info });
    }

    if (!url) {
      throw new Error(
        "Must provide either --url or --source.\n" +
          "Examples:\n" +
          "  Local:  --source=/path/to/repo\n" +
          "  Local:  --url=/path/to/repo\n" +
          "  Remote: --url=git-https://github.com/org/repo",
      );
    }

    // Check if this is a local path
    const isLocal = url.startsWith("/");

    if (isLocal) {
      // For local paths, return the path - caller uses --source to mount
      const dir = dag.address(url).directory();
      const vcs = await this.detect(dir);

      let info: Record<string, string> = {};
      try {
        const infoJson = await this.info(dir);
        info = JSON.parse(infoJson);
      } catch {
        // Ignore info errors
      }

      return JSON.stringify({ vcs, path: url, info });
    }

    // Remote URL - checkout via git/atomic
    const dir = await this.checkout(url, ref, source);
    const vcs = await this.detect(dir);

    let info: Record<string, string> = {};
    try {
      const infoJson = await this.info(dir);
      info = JSON.parse(infoJson);
    } catch {
      // Ignore info errors
    }

    // For remote repos, return the URL as identifier
    // The actual Directory is available via checkout() for chaining
    return JSON.stringify({ vcs, path: url, info });
  }
}
