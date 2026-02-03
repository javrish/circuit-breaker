import { workflow } from "../../sdk/packages/core/src/index";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

// Get the directory of this workflow file
const __dirname = dirname(fileURLToPath(import.meta.url));

// Code Quality Workflow
//
// Runs a Dagger mini-pipeline: checkout → lint → policy
// All steps run inside Dagger containers for portability.
//
// Token schema:
//   url: VCS URL (git-https://, atomic-https://, or absolute local path)
//   ref: Optional branch, tag, or channel
//   source: For local paths, the directory (passed via --source flag)

const wf = workflow("code-quality");

// Paths relative to this workflow file
const POLICIES_PATH = join(__dirname, "policies");
const MODULE_PATH = join(__dirname, "../../modules/vcs");

// Places
wf.place("start", {
  // No initialTokens - require explicit injection with data:
  // cb inject <runId> start --data '{"url": "/path/to/repo", "source": "/path/to/repo"}'
  tokenSchema: {
    type: "object",
    properties: {
      url: {
        type: "string",
        description:
          "VCS URL (git-https://, atomic-https://, or absolute local path)",
      },
      ref: { type: "string", description: "Branch, tag, or channel" },
      source: {
        type: "string",
        description: "Local directory path (for local checkouts)",
      },
    },
    required: ["url"],
  },
});
wf.place("validated");
wf.place("done");

// Transition: start → validated
// Runs the full code quality pipeline via Dagger:
// 1. Checkout source (git or atomic)
// 2. Run ESLint
// 3. Evaluate OPA policy against lint results
wf.transition("validate")
  .from("start")
  .to("validated")
  .dagger(MODULE_PATH, "code-quality", {
    url: "ctx.token.url",
    ref: "ctx.token.ref",
    source: "ctx.token.source",
    policies: POLICIES_PATH,
  })
  .done();

// Transition: validated → done
wf.transition("complete").from("validated").to("done").noop().done();

export const codeQuality = wf.build();
export default codeQuality;
