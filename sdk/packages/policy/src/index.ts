/**
 * @circuit-breaker/policy - OPA policy evaluation for Circuit Breaker workflows.
 */

export interface PolicyAction {
  type: "policy";
  engine: "opa";
  path: string;
  input?: Record<string, unknown>;
}

export const opa = {
  evaluate: (path: string, options: { input?: Record<string, unknown> } = {}): PolicyAction => ({
    type: "policy",
    engine: "opa",
    path,
    input: options.input,
  }),
};
