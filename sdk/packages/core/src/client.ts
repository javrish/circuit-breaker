/**
 * API client for communicating with the Circuit Breaker engine.
 *
 * @module
 */

import type { Workflow } from "./schema";

/**
 * Client configuration options.
 */
export interface ClientOptions {
  /** Base URL of the Circuit Breaker API (default: http://localhost:8080) */
  baseUrl?: string;
  /** API key for authentication */
  apiKey?: string;
  /** Request timeout in milliseconds (default: 30000) */
  timeout?: number;
  /** Custom headers to include in all requests */
  headers?: Record<string, string>;
}

/**
 * Workflow submission response.
 */
export interface SubmitWorkflowResponse {
  workflowId: string;
  name: string;
  namespace: string;
  createdAt: string;
}

/**
 * Workflow run response.
 */
export interface RunWorkflowResponse {
  runId: string;
  workflowId: string;
  status: "pending" | "running" | "completed" | "failed" | "cancelled";
  startedAt: string;
}

/**
 * Workflow run status.
 */
export interface WorkflowRunStatus {
  runId: string;
  workflowId: string;
  workflowName: string;
  status: "pending" | "running" | "completed" | "failed" | "cancelled";
  startedAt: string;
  completedAt?: string;
  currentMarking: Record<string, number>;
  transitions: TransitionStatus[];
  error?: {
    code: string;
    message: string;
    transition?: string;
  };
}

/**
 * Status of a single transition execution.
 */
export interface TransitionStatus {
  transitionId: string;
  status:
    | "pending"
    | "enabled"
    | "firing"
    | "completed"
    | "failed"
    | "retrying";
  attempt: number;
  startedAt?: string;
  completedAt?: string;
  error?: string;
}

/**
 * Workflow definition stored in the engine.
 */
export interface StoredWorkflow {
  workflowId: string;
  name: string;
  namespace: string;
  version: string;
  definition: Workflow;
  createdAt: string;
  updatedAt: string;
}

/**
 * List workflows response.
 */
export interface ListWorkflowsResponse {
  workflows: StoredWorkflow[];
  total: number;
  offset: number;
  limit: number;
}

/**
 * List runs response.
 */
export interface ListRunsResponse {
  runs: WorkflowRunStatus[];
  total: number;
  offset: number;
  limit: number;
}

/**
 * Log entry for a workflow run.
 */
export interface LogEntry {
  timestamp: string;
  level: string;
  message: string;
  transitionId?: string;
  data?: Record<string, unknown>;
}

/**
 * Get logs response.
 */
export interface GetLogsResponse {
  runId: string;
  logs: LogEntry[];
}

/**
 * Place state in a workflow run.
 */
export interface PlaceState {
  id: string;
  name?: string;
  tokens: number;
  tokenData?: unknown[];
  tokenSchema?: Record<string, unknown>;
}

/**
 * Get places response.
 */
export interface GetPlacesResponse {
  runId: string;
  places: PlaceState[];
}

/**
 * API error response.
 */
export class ApiError extends Error {
  constructor(
    message: string,
    public statusCode: number,
    public code?: string,
    public details?: Record<string, unknown>,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/**
 * Client for interacting with the Circuit Breaker API.
 */
export class CircuitBreakerClient {
  private baseUrl: string;
  private apiKey?: string;
  private timeout: number;
  private headers: Record<string, string>;

  constructor(options: ClientOptions = {}) {
    this.baseUrl =
      options.baseUrl ??
      process.env.CIRCUIT_BREAKER_API ??
      "http://localhost:8080";
    this.apiKey = options.apiKey ?? process.env.CIRCUIT_BREAKER_API_KEY;
    this.timeout = options.timeout ?? 30000;
    this.headers = options.headers ?? {};

    // Remove trailing slash from base URL
    this.baseUrl = this.baseUrl.replace(/\/$/, "");
  }

  /**
   * Make an authenticated request to the API.
   */
  private async request<T>(
    method: string,
    path: string,
    body?: unknown,
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      ...this.headers,
    };

    if (this.apiKey) {
      headers["Authorization"] = `Bearer ${this.apiKey}`;
    }

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      let response: Response;
      try {
        response = await fetch(url, {
          method,
          headers,
          body: body ? JSON.stringify(body) : undefined,
          signal: controller.signal,
        });
      } catch (fetchError) {
        clearTimeout(timeoutId);
        // Handle connection errors (server not running, network issues, etc.)
        if (fetchError instanceof Error) {
          if (fetchError.name === "AbortError") {
            throw new ApiError("Request timeout", 408, "TIMEOUT");
          }
          throw new ApiError(
            `Connection failed: ${fetchError.message}. Is the API server running at ${this.baseUrl}?`,
            0,
            "CONNECTION_ERROR",
          );
        }
        throw new ApiError("Connection failed", 0, "CONNECTION_ERROR");
      }

      clearTimeout(timeoutId);

      if (!response.ok) {
        const errorBody = await response.json().catch(() => null);
        throw new ApiError(
          errorBody?.message ??
            `Request failed: ${method} ${url} returned ${response.status}`,
          response.status,
          errorBody?.code,
          errorBody?.details,
        );
      }

      // Handle 204 No Content
      if (response.status === 204) {
        return undefined as T;
      }

      return response.json() as Promise<T>;
    } catch (error) {
      if (error instanceof ApiError) {
        throw error;
      }
      // Shouldn't reach here, but handle unexpected errors
      throw new ApiError(
        error instanceof Error ? error.message : "Unknown error",
        0,
        "UNKNOWN_ERROR",
      );
    }
  }

  // ============ Workflow Management ============

  /**
   * Submit a new workflow definition.
   */
  async submitWorkflow(workflow: Workflow): Promise<SubmitWorkflowResponse> {
    return this.request<SubmitWorkflowResponse>(
      "POST",
      "/api/v1/workflows",
      workflow,
    );
  }

  /**
   * Get a workflow by ID.
   */
  async getWorkflow(workflowId: string): Promise<StoredWorkflow> {
    return this.request<StoredWorkflow>(
      "GET",
      `/api/v1/workflows/${workflowId}`,
    );
  }

  /**
   * Get a workflow by name and namespace.
   */
  async getWorkflowByName(
    name: string,
    namespace = "default",
  ): Promise<StoredWorkflow> {
    return this.request<StoredWorkflow>(
      "GET",
      `/api/v1/namespaces/${namespace}/workflows/${name}`,
    );
  }

  /**
   * List all workflows.
   */
  async listWorkflows(
    options: {
      namespace?: string;
      labels?: Record<string, string>;
      limit?: number;
      offset?: number;
    } = {},
  ): Promise<ListWorkflowsResponse> {
    const params = new URLSearchParams();
    if (options.namespace) params.set("namespace", options.namespace);
    if (options.limit) params.set("limit", options.limit.toString());
    if (options.offset) params.set("offset", options.offset.toString());
    if (options.labels) {
      for (const [key, value] of Object.entries(options.labels)) {
        params.append("label", `${key}=${value}`);
      }
    }

    const query = params.toString();
    return this.request<ListWorkflowsResponse>(
      "GET",
      `/api/v1/workflows${query ? `?${query}` : ""}`,
    );
  }

  /**
   * Delete a workflow.
   */
  async deleteWorkflow(workflowId: string): Promise<void> {
    return this.request<void>("DELETE", `/api/v1/workflows/${workflowId}`);
  }

  // ============ Workflow Execution ============

  /**
   * Start a new run of a workflow.
   */
  async runWorkflow(
    workflowId: string,
    options: {
      inputs?: Record<string, unknown>;
      labels?: Record<string, string>;
    } = {},
  ): Promise<RunWorkflowResponse> {
    return this.request<RunWorkflowResponse>(
      "POST",
      `/api/v1/workflows/${workflowId}/runs`,
      options,
    );
  }

  /**
   * Get the status of a run.
   */
  async getRunStatus(runId: string): Promise<WorkflowRunStatus> {
    return this.request<WorkflowRunStatus>("GET", `/api/v1/runs/${runId}`);
  }

  /**
   * List runs for a workflow.
   */
  async listRuns(
    options: {
      workflowId?: string;
      status?: WorkflowRunStatus["status"];
      limit?: number;
      offset?: number;
    } = {},
  ): Promise<ListRunsResponse> {
    const params = new URLSearchParams();
    if (options.workflowId) params.set("workflowId", options.workflowId);
    if (options.status) params.set("status", options.status);
    if (options.limit) params.set("limit", options.limit.toString());
    if (options.offset) params.set("offset", options.offset.toString());

    const query = params.toString();
    return this.request<ListRunsResponse>(
      "GET",
      `/api/v1/runs${query ? `?${query}` : ""}`,
    );
  }

  /**
   * Cancel a running workflow.
   */
  async cancelRun(runId: string, reason?: string): Promise<void> {
    return this.request<void>("POST", `/api/v1/runs/${runId}/cancel`, {
      reason,
    });
  }

  /**
   * Retry a failed workflow run.
   */
  async retryRun(runId: string): Promise<RunWorkflowResponse> {
    return this.request<RunWorkflowResponse>(
      "POST",
      `/api/v1/runs/${runId}/retry`,
    );
  }

  /**
   * Get logs for a workflow run.
   */
  async getLogs(runId: string): Promise<GetLogsResponse> {
    return this.request<GetLogsResponse>("GET", `/api/v1/runs/${runId}/logs`);
  }

  /**
   * Inject a token into a place in a running workflow.
   */
  async injectToken(
    runId: string,
    placeId: string,
    options: { data?: unknown; reason?: string } = {},
  ): Promise<void> {
    return this.request<void>("POST", `/api/v1/runs/${runId}/inject`, {
      placeId,
      data: options.data,
      reason: options.reason,
    });
  }

  /**
   * Resume a workflow by updating token data to satisfy a failed guard.
   * If no tokens exist in the place, injects one. If tokens exist, updates their data.
   * After updating, guards are re-evaluated and transitions may fire.
   */
  async resume(
    runId: string,
    placeId: string,
    data: unknown,
    options: { reason?: string } = {},
  ): Promise<{
    runId: string;
    placeId: string;
    tokenCount: number;
    injected: boolean;
    enabledTransitions: string[];
    waitingTransitions: string[];
  }> {
    return this.request("POST", `/api/v1/runs/${runId}/resume`, {
      placeId,
      data,
      reason: options.reason,
    });
  }

  /**
   * Get places and their current token state for a run.
   */
  async getPlaces(runId: string): Promise<GetPlacesResponse> {
    return this.request<GetPlacesResponse>(
      "GET",
      `/api/v1/runs/${runId}/places`,
    );
  }

  // ============ Streaming & Real-time ============

  /**
   * Watch a workflow run for updates.
   * Returns an async generator that yields status updates.
   */
  async *watchRun(
    runId: string,
    options: { pollInterval?: number } = {},
  ): AsyncGenerator<WorkflowRunStatus> {
    const pollInterval = options.pollInterval ?? 1000;
    let lastStatus: WorkflowRunStatus | null = null;

    while (true) {
      const status = await this.getRunStatus(runId);

      // Yield if status changed
      if (!lastStatus || status.status !== lastStatus.status) {
        yield status;
      }

      // Stop if terminal state
      if (["completed", "failed", "cancelled"].includes(status.status)) {
        return;
      }

      lastStatus = status;
      await new Promise((resolve) => setTimeout(resolve, pollInterval));
    }
  }

  /**
   * Subscribe to events via WebSocket.
   * Note: Requires WebSocket support in the environment.
   */
  subscribeToEvents(
    options: {
      runId?: string;
      workflowId?: string;
      eventTypes?: string[];
    } = {},
  ): WebSocket {
    const wsUrl = this.baseUrl.replace(/^http/, "ws");
    const params = new URLSearchParams();
    if (options.runId) params.set("runId", options.runId);
    if (options.workflowId) params.set("workflowId", options.workflowId);
    if (options.eventTypes) {
      options.eventTypes.forEach((t) => params.append("eventType", t));
    }

    const query = params.toString();
    const ws = new WebSocket(
      `${wsUrl}/api/v1/events/ws${query ? `?${query}` : ""}`,
    );

    if (this.apiKey) {
      // Send auth after connection
      ws.addEventListener("open", () => {
        ws.send(JSON.stringify({ type: "auth", token: this.apiKey }));
      });
    }

    return ws;
  }

  // ============ Health & Info ============

  /**
   * Check API health.
   */
  async health(): Promise<{
    status: "healthy" | "degraded" | "unhealthy";
    version: string;
  }> {
    return this.request<{
      status: "healthy" | "degraded" | "unhealthy";
      version: string;
    }>("GET", "/health");
  }

  /**
   * Get API version info.
   */
  async version(): Promise<{
    version: string;
    gitCommit: string;
    buildTime: string;
  }> {
    return this.request<{
      version: string;
      gitCommit: string;
      buildTime: string;
    }>("GET", "/version");
  }
}

/**
 * Create a new client with default options from environment.
 */
export function createClient(options?: ClientOptions): CircuitBreakerClient {
  return new CircuitBreakerClient(options);
}
