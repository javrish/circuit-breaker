#!/usr/bin/env bun
/**
 * Circuit Breaker Interactive TUI
 *
 * A full interactive terminal interface with slash commands for managing workflows.
 * Run `cb` to start the TUI, then use commands like:
 *   /run <workflow>     - Run a workflow
 *   /status [run-id]    - Check status
 *   /logs [run-id]      - View logs
 *   /inject <place>     - Inject a token
 *   /list               - List workflows/runs
 *   /help               - Show help
 *   /quit               - Exit
 */

import React, { useState, useEffect, useRef } from "react";
import { render, Box, Text, useApp, useInput } from "ink";
import TextInput from "ink-text-input";
import Spinner from "ink-spinner";
import { connect, type NatsConnection, type Subscription } from "nats";
import { CircuitBreakerClient } from "@circuit-breaker/core";

// ============ Types ============

interface LogEntry {
  timestamp: string;
  type: "info" | "success" | "error" | "warn" | "event" | "command" | "system";
  message: string;
  details?: string;
}

interface RunInfo {
  runId: string;
  workflowName: string;
  status: string;
  startedAt: string;
}

interface AppState {
  mode: "command" | "selecting" | "confirming";
  currentRun: RunInfo | null;
  logs: LogEntry[];
  eventCount: number;
  natsConnected: boolean;
  apiConnected: boolean;
  devMode: boolean;
  loading: boolean;
  loadingMessage: string;
}

interface AppProps {
  apiUrl: string;
  apiKey?: string;
  natsUrl: string;
}

// ============ Commands ============

const COMMANDS: Record<string, { desc: string; devMode: boolean }> = {
  "/help": { desc: "Show available commands", devMode: true },
  "/run <workflow> [input]": {
    desc: "Run a workflow file with optional JSON input",
    devMode: false,
  },
  "/validate <workflow>": {
    desc: "Validate a workflow file (works offline)",
    devMode: true,
  },
  "/visualize <workflow>": {
    desc: "Generate workflow visualization",
    devMode: true,
  },
  "/status [run-id]": {
    desc: "Get status of current or specified run",
    devMode: false,
  },
  "/logs [run-id]": {
    desc: "View logs for current or specified run",
    devMode: false,
  },
  "/list": { desc: "List recent workflow runs", devMode: false },
  "/workflows": { desc: "List available workflows", devMode: false },
  "/resume <place> <data>": {
    desc: "Resume workflow by updating token data to satisfy a failed guard",
    devMode: false,
  },
  "/inject <place> [data]": {
    desc: "Inject a new token into a place",
    devMode: false,
  },
  "/describe [run-id]": {
    desc: "Show places, tokens, and how to trigger transitions",
    devMode: false,
  },
  "/cancel [run-id]": { desc: "Cancel a running workflow", devMode: false },
  "/clear": { desc: "Clear the log output", devMode: true },
  "/connect": { desc: "Reconnect to API and NATS", devMode: true },
  "/quit": { desc: "Exit the TUI", devMode: true },
};

// ============ Main App ============

const App: React.FC<AppProps> = ({ apiUrl, apiKey, natsUrl }) => {
  const { exit } = useApp();
  const [input, setInput] = useState("");
  const [state, setState] = useState<AppState>({
    mode: "command",
    currentRun: null,
    logs: [],
    eventCount: 0,
    natsConnected: false,
    apiConnected: false,
    devMode: true, // Start in dev mode until we confirm API is up
    loading: false,
    loadingMessage: "",
  });
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);

  const clientRef = useRef<CircuitBreakerClient | null>(null);
  const natsConnRef = useRef<NatsConnection | null>(null);
  const subsRef = useRef<Subscription | null>(null);

  // Initialize client
  if (!clientRef.current) {
    clientRef.current = new CircuitBreakerClient({ baseUrl: apiUrl, apiKey });
  }
  const client = clientRef.current;

  // Add log entry
  const log = (type: LogEntry["type"], message: string, details?: string) => {
    setState((prev) => ({
      ...prev,
      logs: [
        ...prev.logs,
        {
          timestamp: new Date().toLocaleTimeString(),
          type,
          message,
          details,
        },
      ].slice(-100), // Keep last 100 entries
    }));
  };

  // Set loading state
  const setLoading = (loading: boolean, message = "") => {
    setState((prev) => ({ ...prev, loading, loadingMessage: message }));
  };

  // Check API health
  const checkApiHealth = async (): Promise<boolean> => {
    try {
      const health = await client.health();
      if (health.status === "healthy" || health.status === "ok") {
        setState((prev) => ({ ...prev, apiConnected: true, devMode: false }));
        return true;
      }
    } catch (err) {
      // API not available
    }
    setState((prev) => ({ ...prev, apiConnected: false, devMode: true }));
    return false;
  };

  // Connect to NATS
  const connectNats = async (): Promise<boolean> => {
    try {
      if (natsConnRef.current) {
        await natsConnRef.current.close();
      }

      const nc = await connect({ servers: natsUrl });
      natsConnRef.current = nc;
      setState((prev) => ({ ...prev, natsConnected: true }));

      // Subscribe to all run events
      const sub = nc.subscribe("cb.runs.>");
      subsRef.current = sub;

      (async () => {
        for await (const msg of sub) {
          handleNatsEvent(msg.subject, msg.string());
        }
      })();

      return true;
    } catch (err) {
      setState((prev) => ({ ...prev, natsConnected: false }));
      return false;
    }
  };

  // Connect to both API and NATS
  const connectAll = async () => {
    setLoading(true, "Connecting...");

    const apiOk = await checkApiHealth();
    const natsOk = await connectNats();

    if (apiOk && natsOk) {
      log("success", "Connected to API and NATS");
    } else if (apiOk) {
      log("success", "Connected to API");
      log("warn", `Failed to connect to NATS at ${natsUrl}`);
    } else if (natsOk) {
      log("warn", `API not available at ${apiUrl}`);
      log("success", "Connected to NATS");
      log("system", "Running in dev mode - some commands unavailable");
    } else {
      log("warn", `API not available at ${apiUrl}`);
      log("warn", `NATS not available at ${natsUrl}`);
      log("system", "Running in offline dev mode");
      log("info", "You can still /validate and /visualize workflows locally");
    }

    setLoading(false);
  };

  // Handle NATS events
  const handleNatsEvent = (subject: string, data: string) => {
    setState((prev) => ({ ...prev, eventCount: prev.eventCount + 1 }));

    try {
      const event = JSON.parse(data);
      const parts = subject.split(".");
      const runId = parts[2];

      // Only show events for current run or if no run selected
      if (state.currentRun && state.currentRun.runId !== runId) {
        return;
      }

      if (subject.includes(".transitions.")) {
        const transitionId = parts[4];
        const eventType = parts[5];

        switch (eventType) {
          case "enabled":
            log("event", `Transition enabled: ${event.name || transitionId}`);
            break;
          case "fired":
            log("event", `Transition started: ${event.name || transitionId}`);
            break;
          case "completed":
            log(
              "success",
              `Transition completed: ${event.name || transitionId}`,
              event.resource_usage?.duration_ms
                ? `${event.resource_usage.duration_ms}ms`
                : undefined,
            );
            // Show script output if present
            if (event.outputs?.output) {
              const output = event.outputs.output.trim();
              if (output) {
                output.split("\n").forEach((line: string) => {
                  log("info", `  > ${line}`);
                });
              }
            }
            break;
          case "failed":
            log(
              "error",
              `Transition failed: ${event.name || transitionId}`,
              event.error?.message,
            );
            break;
        }
      } else if (subject.includes(".status")) {
        log("info", `Run status: ${event.status}`);

        if (state.currentRun) {
          setState((prev) => ({
            ...prev,
            currentRun: prev.currentRun
              ? { ...prev.currentRun, status: event.status }
              : null,
          }));
        }
      } else if (subject.includes(".tokens.")) {
        const placeId = parts[4];
        log("event", `Token injected into: ${placeId}`);
      } else if (subject.includes(".logs.")) {
        // Script publish() output - cb.runs.{run_id}.logs.{transition_id}
        const transitionId = parts[4];
        const level = event.level || "info";
        const message = event.message || "";

        // Map level to log type
        const logType =
          level === "error" ? "error" : level === "warn" ? "warn" : "info";

        // Format: == <run_id> == [transition] : message
        const shortRunId = runId.slice(0, 8);
        log(
          logType as any,
          `== ${shortRunId} == [${transitionId}] : ${message}`,
        );
      }
    } catch (err) {
      // Ignore parse errors
    }
  };

  // Initialize
  useEffect(() => {
    log("info", "Welcome to Circuit Breaker TUI");
    log("info", "Type /help for available commands");
    connectAll();

    return () => {
      if (subsRef.current) subsRef.current.unsubscribe();
      if (natsConnRef.current) natsConnRef.current.close();
    };
  }, []);

  // Handle keyboard input for history navigation
  useInput((char, key) => {
    if (key.upArrow && history.length > 0) {
      const newIndex =
        historyIndex < history.length - 1 ? historyIndex + 1 : historyIndex;
      setHistoryIndex(newIndex);
      setInput(history[history.length - 1 - newIndex] || "");
    }
    if (key.downArrow && historyIndex > 0) {
      const newIndex = historyIndex - 1;
      setHistoryIndex(newIndex);
      setInput(history[history.length - 1 - newIndex] || "");
    }
    if (key.downArrow && historyIndex === 0) {
      setHistoryIndex(-1);
      setInput("");
    }
  });

  // Check if command requires API
  const requiresApi = (cmd: string): boolean => {
    const cmdDef = Object.entries(COMMANDS).find(
      ([key]) => key.split(" ")[0] === cmd.toLowerCase(),
    );
    return cmdDef ? !cmdDef[1].devMode : true;
  };

  // Execute command
  const executeCommand = async (cmd: string) => {
    const trimmed = cmd.trim();
    if (!trimmed) return;

    // Add to history
    setHistory((prev) => [...prev, trimmed]);
    setHistoryIndex(-1);
    setInput("");

    log("command", trimmed);

    // Parse command and args, preserving quoted strings
    const parts: string[] = [];
    let current = "";
    let inQuote = false;
    let quoteChar = "";

    for (const char of trimmed) {
      if ((char === '"' || char === "'") && !inQuote) {
        inQuote = true;
        quoteChar = char;
      } else if (char === quoteChar && inQuote) {
        inQuote = false;
        quoteChar = "";
      } else if (char === " " && !inQuote) {
        if (current) {
          parts.push(current);
          current = "";
        }
      } else {
        current += char;
      }
    }
    if (current) {
      parts.push(current);
    }

    const [command, ...args] = parts;

    // Check if command requires API and we're in dev mode
    if (state.devMode && requiresApi(command)) {
      log("error", `Command ${command} requires API connection`);
      log(
        "info",
        "Use /connect to retry connection, or use offline commands like /validate",
      );
      setLoading(false);
      return;
    }

    try {
      switch (command.toLowerCase()) {
        case "/help":
        case "/?":
          log("info", "─── Available Commands ───");
          Object.entries(COMMANDS).forEach(([cmd, { desc, devMode }]) => {
            const available = !state.devMode || devMode;
            const prefix = available ? "  " : "  [offline] ";
            log(
              available ? "info" : "warn",
              `${prefix}${cmd.padEnd(25)} ${desc}`,
            );
          });
          if (state.devMode) {
            log("system", "Commands marked [offline] require API connection");
          }
          break;

        case "/quit":
        case "/exit":
        case "/q":
          log("info", "Goodbye!");
          setTimeout(() => exit(), 500);
          break;

        case "/clear":
          setState((prev) => ({ ...prev, logs: [] }));
          break;

        case "/connect":
          await connectAll();
          break;

        case "/validate":
          if (!args[0]) {
            log("error", "Usage: /validate <workflow-file>");
            break;
          }
          await validateWorkflow(args[0]);
          break;

        case "/visualize":
        case "/viz":
          if (!args[0]) {
            log("error", "Usage: /visualize <workflow-file>");
            break;
          }
          await visualizeWorkflow(args[0]);
          break;

        case "/run":
          if (!args[0]) {
            log("error", "Usage: /run <workflow-file> [json-input]");
            log(
              "info",
              '  Example: /run workflow.ts \'{"repository": "file:///path/to/repo"}\'',
            );
            break;
          }
          // Join remaining args in case JSON has spaces
          const inputJson = args.slice(1).join(" ") || undefined;
          await runWorkflow(args[0], inputJson);
          break;

        case "/status":
          await showStatus(args[0]);
          break;

        case "/logs":
          await showLogs(args[0]);
          break;

        case "/list":
          await listRuns();
          break;

        case "/workflows":
          await listWorkflows();
          break;

        case "/resume":
          if (!args[0] || !args[1]) {
            log("error", "Usage: /resume <place-id> <json-data>");
            log(
              "info",
              "  Updates token data to satisfy a failed guard, then re-evaluates",
            );
            break;
          }
          await resumeWorkflow(args[0], args.slice(1).join(" "));
          break;

        case "/inject":
          if (!args[0]) {
            log("error", "Usage: /inject <place-id> [json-data]");
            log("info", "  Tip: Use /set for smart inject/update behavior");
            break;
          }
          await injectToken(
            args[0],
            args[1],
            args.slice(2).join(" ") || undefined,
          );
          break;

        case "/describe":
          await describeRun(args[0]);
          break;

        case "/cancel":
          await cancelRun(args[0]);
          break;

        default:
          if (command.startsWith("/")) {
            log(
              "error",
              `Unknown command: ${command}. Type /help for available commands.`,
            );
          } else {
            log("warn", `Commands start with /. Did you mean /${command}?`);
          }
      }
    } catch (err) {
      log(
        "error",
        `Command failed: ${err instanceof Error ? err.message : "Unknown error"}`,
      );
    }

    setLoading(false);
  };

  // ============ Command Implementations ============

  // Validate workflow (works offline)
  const validateWorkflow = async (workflowPath: string) => {
    setLoading(true, `Validating ${workflowPath}...`);

    try {
      const { resolve, extname } = await import("path");
      const absolutePath = resolve(process.cwd(), workflowPath);
      const { WorkflowSchema, validateWorkflow: validate } =
        await import("@circuit-breaker/core");

      let workflow;
      const ext = extname(workflowPath).toLowerCase();

      if (ext === ".json") {
        const file = Bun.file(absolutePath);
        workflow = WorkflowSchema.parse(await file.json());
      } else {
        const module = await import(absolutePath);
        const wf = module.default ?? module.workflow;
        if (typeof wf?.build === "function") {
          workflow = wf.build();
        } else {
          workflow = WorkflowSchema.parse(wf);
        }
      }

      log("info", `Loaded workflow: ${workflow.name}`);

      const validation = validate(workflow);
      if (validation.valid) {
        log("success", "Workflow is valid");
        log("info", `  Places: ${workflow.places.length}`);
        log("info", `  Transitions: ${workflow.transitions.length}`);
        const initialPlaces = workflow.places.filter(
          (p: any) => p.initialTokens > 0,
        );
        if (initialPlaces.length > 0) {
          log(
            "info",
            `  Initial tokens: ${initialPlaces.map((p: any) => `${p.id}(${p.initialTokens})`).join(", ")}`,
          );
        }
      } else {
        log("error", "Workflow validation failed");
        validation.errors.forEach((err: any) =>
          log("error", `  ${err.message}`),
        );
      }

      if (validation.warnings?.length > 0) {
        validation.warnings.forEach((warn: any) =>
          log("warn", `  ${warn.message}`),
        );
      }
    } catch (err) {
      log(
        "error",
        `Failed to validate: ${err instanceof Error ? err.message : "Unknown error"}`,
      );
    }
  };

  // Visualize workflow (works offline)
  const visualizeWorkflow = async (workflowPath: string) => {
    setLoading(true, `Generating visualization for ${workflowPath}...`);

    try {
      const { resolve, extname } = await import("path");
      const absolutePath = resolve(process.cwd(), workflowPath);
      const { WorkflowSchema, visualize, getMermaidUrl } =
        await import("@circuit-breaker/core");

      let workflow;
      const ext = extname(workflowPath).toLowerCase();

      if (ext === ".json") {
        const file = Bun.file(absolutePath);
        workflow = WorkflowSchema.parse(await file.json());
      } else {
        const module = await import(absolutePath);
        const wf = module.default ?? module.workflow;
        if (typeof wf?.build === "function") {
          workflow = wf.build();
        } else {
          workflow = WorkflowSchema.parse(wf);
        }
      }

      log("info", `Workflow: ${workflow.name}`);

      // Generate mermaid diagram
      const mermaid = visualize(workflow, {
        format: "mermaid",
        showTokens: true,
      });
      log("info", "─── Mermaid Diagram ───");
      mermaid
        .split("\n")
        .slice(0, 15)
        .forEach((line: string) => {
          log("info", `  ${line}`);
        });
      if (mermaid.split("\n").length > 15) {
        log("info", "  ...(truncated)");
      }

      // Generate URL
      const url = getMermaidUrl(workflow);
      log("success", `View online: ${url}`);
    } catch (err) {
      log(
        "error",
        `Failed to visualize: ${err instanceof Error ? err.message : "Unknown error"}`,
      );
    }
  };

  // Run workflow (requires API)
  const runWorkflow = async (workflowPath: string, inputJson?: string) => {
    setLoading(true, `Running ${workflowPath}...`);

    try {
      const { resolve, extname } = await import("path");
      const absolutePath = resolve(process.cwd(), workflowPath);
      const { WorkflowSchema, validateWorkflow: validate } =
        await import("@circuit-breaker/core");

      let workflow;
      const ext = extname(workflowPath).toLowerCase();

      if (ext === ".json") {
        const file = Bun.file(absolutePath);
        workflow = WorkflowSchema.parse(await file.json());
      } else {
        const module = await import(absolutePath);
        const wf = module.default ?? module.workflow;
        if (typeof wf?.build === "function") {
          workflow = wf.build();
        } else {
          workflow = WorkflowSchema.parse(wf);
        }
      }

      log("info", `Loaded workflow: ${workflow.name}`);

      // Validate
      const validation = validate(workflow);
      if (!validation.valid) {
        validation.errors.forEach((err: any) => log("error", err.message));
        return;
      }
      log("success", "Validation passed");

      // Parse input if provided
      let inputs: Record<string, unknown> | undefined;
      if (inputJson) {
        try {
          inputs = JSON.parse(inputJson);
          log("info", `Input: ${JSON.stringify(inputs)}`);
        } catch {
          log("error", "Failed to parse input JSON");
          return;
        }
      }

      // Submit
      const submitResult = await client.submitWorkflow(workflow);
      log("success", `Submitted: ${submitResult.workflowId}`);

      // Run with inputs
      const runResult = await client.runWorkflow(submitResult.workflowId, {
        inputs,
      });
      log("success", `Started run: ${runResult.runId}`);

      // Set as current run
      setState((prev) => ({
        ...prev,
        currentRun: {
          runId: runResult.runId,
          workflowName: workflow.name,
          status: "running",
          startedAt: new Date().toISOString(),
        },
      }));

      log(
        "info",
        `Monitoring run ${runResult.runId} - events will appear below`,
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      const stack = err instanceof Error ? err.stack : "";
      log("error", `Failed to run workflow: ${msg}`);
      if (stack) {
        log("error", stack);
      }
    }
  };

  const showStatus = async (runId?: string) => {
    const id = runId || state.currentRun?.runId;
    if (!id) {
      log(
        "error",
        "No run ID specified and no current run. Usage: /status <run-id>",
      );
      return;
    }

    setLoading(true, "Fetching status...");

    try {
      const status = await client.getRunStatus(id);
      log("info", `─── Run ${id} ───`);
      log("info", `  Workflow: ${status.workflowName}`);
      log("info", `  Status:   ${status.status}`);
      log("info", `  Started:  ${status.startedAt}`);
      if (status.completedAt) {
        log("info", `  Completed: ${status.completedAt}`);
      }
      if (status.error) {
        log("error", `  Error: ${status.error.message}`);
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : "Unknown error";
      if (errMsg.includes("400")) {
        log("error", "Invalid run ID format - must be a full UUID");
        log(
          "info",
          "Use the full run ID, e.g., 0a561d5a-1234-5678-9abc-def012345678",
        );
      } else if (errMsg.includes("404")) {
        log("error", `Run not found: ${id}`);
      } else {
        log("error", `Failed to get status: ${errMsg}`);
      }
    }
  };

  const showLogs = async (runId?: string) => {
    const id = runId || state.currentRun?.runId;
    if (!id) {
      log(
        "error",
        "No run ID specified and no current run. Usage: /logs <run-id>",
      );
      return;
    }

    setLoading(true, "Fetching logs...");

    try {
      const logsData = await client.getLogs(id);
      log("info", `─── Logs for ${id} ───`);
      (logsData.logs || []).slice(-20).forEach((entry: any) => {
        log("info", `  [${entry.timestamp}] ${entry.message}`);
      });
    } catch (err) {
      log(
        "error",
        `Failed to get logs: ${err instanceof Error ? err.message : "Unknown error"}`,
      );
    }
  };

  const listRuns = async () => {
    setLoading(true, "Fetching runs...");

    try {
      const runs = await client.listRuns();
      log("info", "─── Recent Runs ───");
      if (!runs.runs || runs.runs.length === 0) {
        log("info", "  No runs found");
      } else {
        runs.runs.slice(0, 10).forEach((run: any) => {
          const status =
            run.status === "completed"
              ? "✓"
              : run.status === "failed"
                ? "✗"
                : run.status === "running"
                  ? "⟳"
                  : "◷";
          log(
            "info",
            `  ${status} ${run.runId} - ${run.workflowName} (${run.status})`,
          );
        });
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : "Unknown error";
      if (errMsg.includes("404")) {
        log("error", "API endpoint not available (404)");
        log("info", "The /list endpoint may not be implemented yet");
        log("info", "Try /run to start a new workflow instead");
      } else {
        log("error", `Failed to list runs: ${errMsg}`);
      }
    }
  };

  const listWorkflows = async () => {
    setLoading(true, "Fetching workflows...");

    try {
      const workflows = await client.listWorkflows();
      log("info", "─── Workflows ───");
      if (!workflows.workflows || workflows.workflows.length === 0) {
        log("info", "  No workflows submitted yet");
        log("info", "  Use /run <workflow-file> to submit and run a workflow");
      } else {
        workflows.workflows.forEach((wf: any) => {
          log("info", `  ${wf.name} (${wf.namespace})`);
        });
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : "Unknown error";
      if (errMsg.includes("404")) {
        log("error", "API endpoint not available (404)");
        log("info", "Use /run <workflow-file> to submit and run a workflow");
      } else {
        log("error", `Failed to list workflows: ${errMsg}`);
      }
    }
  };

  const resumeWorkflow = async (placeId: string, dataStr: string) => {
    const runId = state.currentRun?.runId;
    if (!runId) {
      log("error", "No current run. Start a workflow first with /run");
      return;
    }

    let data: unknown;
    try {
      data = JSON.parse(dataStr);
    } catch {
      log(
        "error",
        "Invalid JSON data. Example: /resume evaluated '{\"score\": 95}'",
      );
      return;
    }

    setLoading(true, `Resuming with updated token data in ${placeId}...`);

    try {
      const result = await client.resume(runId, placeId, data, {
        reason: "TUI resume",
      });

      const action = result.injected ? "injected token" : "updated token";
      log("success", `Resumed: ${action} in ${placeId}`);

      if (result.enabledTransitions.length > 0) {
        log(
          "success",
          `Guards passed! Enabled: ${result.enabledTransitions.join(", ")}`,
        );
      }
      if (result.waitingTransitions.length > 0) {
        log(
          "warn",
          `Guards still failing: ${result.waitingTransitions.join(", ")}`,
        );
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : "Unknown error";
      if (errMsg.includes("400")) {
        log("error", `Place '${placeId}' not found in workflow`);
      } else if (errMsg.includes("404")) {
        log("error", `Run not found`);
      } else {
        log("error", `Failed to resume: ${errMsg}`);
      }
    }
  };

  const injectToken = async (
    placeId: string,
    dataStr?: string,
    _unused?: string,
  ) => {
    const runId = state.currentRun?.runId;
    if (!runId) {
      log("error", "No current run. Start a workflow first with /run");
      return;
    }

    let data: unknown = undefined;
    if (dataStr) {
      try {
        data = JSON.parse(dataStr);
      } catch {
        log("error", "Invalid JSON data");
        return;
      }
    }

    setLoading(true, `Injecting token into ${placeId}...`);

    try {
      await client.injectToken(runId, placeId, {
        data,
        reason: "TUI injection",
      });
      log("success", `Token injected into ${placeId}`);
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : "Unknown error";
      if (errMsg.includes("400")) {
        log("error", `Place '${placeId}' not found in workflow`);
      } else if (errMsg.includes("404")) {
        log("error", `Run not found`);
      } else {
        log("error", `Failed to inject token: ${errMsg}`);
      }
    }
  };

  const describeRun = async (runId?: string) => {
    const id = runId || state.currentRun?.runId;
    if (!id) {
      log(
        "error",
        "No run ID specified and no current run. Usage: /describe <run-id>",
      );
      return;
    }

    setLoading(true, "Fetching description...");

    try {
      // Get places with token counts
      const placesResp = await client.getPlaces(id);

      // Also get run status to find workflow_id
      const runStatus = await client.getRunStatus(id);

      log("info", `─── Run: ${runStatus.workflowName || id} ───`);
      log("info", `Run ID: ${id}`);
      log("info", "");
      log("info", "Places (current tokens):");
      (placesResp.places || []).forEach((p: any) => {
        const tokens = p.tokenCount > 0 ? `● ${p.tokenCount}` : "○ 0";
        log("info", `  ${tokens}  ${p.placeId}`);
      });

      log("info", "");
      log(
        "info",
        "To trigger a transition, inject a token into its input place:",
      );
      log("info", `  /inject ${id} <place-id> [json-data]`);
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : "Unknown error";
      if (errMsg.includes("400")) {
        log("error", "Invalid run ID format - must be a full UUID");
      } else {
        log("error", `Failed to describe: ${errMsg}`);
      }
    }
  };

  const cancelRun = async (runId?: string) => {
    const id = runId || state.currentRun?.runId;
    if (!id) {
      log(
        "error",
        "No run ID specified and no current run. Usage: /cancel <run-id>",
      );
      return;
    }

    setLoading(true, `Cancelling ${id}...`);

    try {
      await client.cancelRun(id);
      log("success", `Cancelled run ${id}`);

      if (state.currentRun?.runId === id) {
        setState((prev) => ({
          ...prev,
          currentRun: prev.currentRun
            ? { ...prev.currentRun, status: "cancelled" }
            : null,
        }));
      }
    } catch (err) {
      log(
        "error",
        `Failed to cancel: ${err instanceof Error ? err.message : "Unknown error"}`,
      );
    }
  };

  // Get log color
  const getLogColor = (type: LogEntry["type"]) => {
    switch (type) {
      case "success":
        return "green";
      case "error":
        return "red";
      case "warn":
        return "yellow";
      case "event":
        return "cyan";
      case "command":
        return "magenta";
      case "system":
        return "blue";
      default:
        return "white";
    }
  };

  // Get connection status color
  const getConnectionColor = () => {
    if (state.apiConnected && state.natsConnected) return "green";
    if (state.apiConnected || state.natsConnected) return "yellow";
    return "red";
  };

  // Get connection status text
  const getConnectionStatus = () => {
    if (state.apiConnected && state.natsConnected) return "● online";
    if (state.devMode) return "○ dev mode";
    if (state.apiConnected) return "◐ API only";
    if (state.natsConnected) return "◐ NATS only";
    return "○ offline";
  };

  return (
    <Box flexDirection="column" padding={1}>
      {/* Header */}
      <Box borderStyle="round" borderColor="cyan" paddingX={2} marginBottom={1}>
        <Box width="100%" justifyContent="space-between">
          <Text bold color="cyan">
            Circuit Breaker
          </Text>
          <Box>
            <Text color={getConnectionColor()}>{getConnectionStatus()}</Text>
            {state.eventCount > 0 && (
              <Text dimColor> ({state.eventCount} events)</Text>
            )}
          </Box>
        </Box>
      </Box>

      {/* Dev Mode Banner */}
      {state.devMode && (
        <Box marginBottom={1} paddingX={1}>
          <Text color="yellow">
            ⚠ Dev Mode - API not connected. Use /connect to retry or /validate
            for offline usage.
          </Text>
        </Box>
      )}

      {/* Current Run Info */}
      {state.currentRun && (
        <Box marginBottom={1} paddingX={1}>
          <Text dimColor>Run: </Text>
          <Text>{state.currentRun.workflowName}</Text>
          <Text dimColor> ({state.currentRun.runId.slice(0, 8)})</Text>
          <Text> </Text>
          <Text
            color={
              state.currentRun.status === "completed"
                ? "green"
                : state.currentRun.status === "failed"
                  ? "red"
                  : state.currentRun.status === "running"
                    ? "yellow"
                    : "gray"
            }
          >
            [{state.currentRun.status}]
          </Text>
        </Box>
      )}

      {/* Log Output */}
      <Box
        flexDirection="column"
        borderStyle="single"
        borderColor="gray"
        height={20}
        paddingX={1}
        overflow="hidden"
      >
        {state.logs.slice(-15).map((entry, i) => (
          <Box key={i}>
            <Text dimColor>[{entry.timestamp}] </Text>
            <Text color={getLogColor(entry.type)}>{entry.message}</Text>
            {entry.details && <Text dimColor> - {entry.details}</Text>}
          </Box>
        ))}
        {state.logs.length === 0 && (
          <Text dimColor>No output yet. Type /help to get started.</Text>
        )}
      </Box>

      {/* Loading Indicator */}
      {state.loading && (
        <Box marginY={1}>
          <Text color="yellow">
            <Spinner type="dots" /> {state.loadingMessage || "Loading..."}
          </Text>
        </Box>
      )}

      {/* Command Input */}
      <Box marginTop={1}>
        <Text color="cyan">❯ </Text>
        <TextInput
          value={input}
          onChange={setInput}
          onSubmit={executeCommand}
          placeholder="Type a command (e.g., /help)"
        />
      </Box>

      {/* Footer */}
      <Box marginTop={1}>
        <Text dimColor>
          {state.devMode
            ? "/help for commands | /validate to check workflows | /connect to retry"
            : "/help for commands | /run to start a workflow | /quit to exit"}
        </Text>
      </Box>
    </Box>
  );
};

export default App;

export function startTUI(options: AppProps) {
  render(<App {...options} />);
}
