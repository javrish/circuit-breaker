import { workflow } from "../../sdk/packages/core/src/index";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Code Quality Workflow
//
// Multi-step pipeline using separate Dagger modules:
//   1. checkout (vcs module) - Validate and return path info
//   2. lint (lint module) - Run ESLint on source
//   3. policy (policy module) - Evaluate OPA policies against lint results
//
// This workflow is designed for LOCAL directories where each step
// mounts the host path via Dagger's --source flag.
//
// For remote repositories (git-https://, atomic-https://), consider:
//   - Using a single-step pipeline that clones and processes in one session
//   - Or clone to a local path first, then run this workflow

const wf = workflow("code-quality");

// Module paths relative to this workflow file
const VCS_MODULE = join(__dirname, "../../modules/vcs");
const LINT_MODULE = join(__dirname, "../../modules/lint");
const POLICY_MODULE = join(__dirname, "../../modules/policy");
const POLICIES_PATH = join(__dirname, "policies");

// Places
wf.place("start", {
  tokenSchema: {
    type: "object",
    properties: {
      url: {
        type: "string",
        description: "Local path to repository (e.g., /path/to/repo)",
      },
    },
    required: ["url"],
  },
});

wf.place("checked-out", {
  tokenSchema: {
    type: "object",
    properties: {
      vcs: {
        type: "string",
        description: "Detected VCS type (git, atomic, unknown)",
      },
      path: { type: "string", description: "Path to source" },
      info: { type: "object", description: "Repository metadata" },
    },
    required: ["vcs", "path"],
  },
});

wf.place("linted", {
  tokenSchema: {
    type: "object",
    properties: {
      vcs: { type: "string" },
      path: { type: "string" },
      lint: { type: "object", description: "ESLint results" },
    },
    required: ["path", "lint"],
  },
});

wf.place("validated", {
  tokenSchema: {
    type: "object",
    properties: {
      vcs: { type: "string" },
      path: { type: "string" },
      lint: { type: "object" },
      policy: {
        type: "object",
        properties: {
          passed: { type: "boolean" },
          failures: { type: "array", items: { type: "string" } },
          warnings: { type: "array", items: { type: "string" } },
        },
      },
    },
  },
});

wf.place("done");

// Transition: start → checked-out
// Validate the path and detect VCS type
wf.transition("checkout")
  .from("start")
  .to("checked-out")
  .dagger(VCS_MODULE, "checkout-to-path", {
    source: "ctx.token.url",
    url: "ctx.token.url",
  })
  .done();

// Transition: checked-out → linted
// Run ESLint - Dagger mounts host directory via --source
wf.transition("lint")
  .from("checked-out")
  .to("linted")
  .dagger(LINT_MODULE, "eslint", {
    source: "ctx.token.path",
  })
  .done();

// Transition: linted → validated
// Evaluate OPA policy against lint results
wf.transition("policy-check")
  .from("linted")
  .to("validated")
  .dagger(POLICY_MODULE, "evaluate", {
    input: "ctx.token.lint",
    policies: POLICIES_PATH,
  })
  .done();

// Transition: validated → done
// Final transition to mark workflow complete
wf.transition("complete").from("validated").to("done").noop().done();

export const codeQuality = wf.build();
export default codeQuality;
