/**
 * Zod schemas for Circuit Breaker workflow definitions.
 * These schemas match the JSON Schema contract in /schemas/workflow.schema.json
 *
 * @module
 */

import { z } from 'zod';

// ============ Metadata ============

export const MetadataSchema = z.object({
  description: z.string().max(1000).optional(),
  labels: z.record(z.string()).optional(),
  annotations: z.record(z.string()).optional(),
});

export type Metadata = z.infer<typeof MetadataSchema>;

// ============ Token Schema (for colored Petri nets) ============

export const TokenSchemaSchema = z.record(z.any()).optional();

export type TokenSchema = z.infer<typeof TokenSchemaSchema>;

// ============ Place ============

export const PlaceSchema = z.object({
  id: z
    .string()
    .regex(/^[a-z][a-z0-9_-]*$/, 'Place ID must be lowercase alphanumeric with underscores/hyphens'),
  initialTokens: z.number().int().min(0).default(0),
  capacity: z.number().int().min(1).nullable().default(null),
  tokenSchema: TokenSchemaSchema,
});

export type Place = z.infer<typeof PlaceSchema>;

// ============ Arc ============

export const ArcSchema = z.object({
  place: z.string(),
  weight: z.number().int().min(1).default(1),
  expression: z.string().optional(), // CEL expression for token selection
});

export type Arc = z.infer<typeof ArcSchema>;

// ============ Resources ============

export const TolerationSchema = z.object({
  key: z.string().optional(),
  operator: z.enum(['Exists', 'Equal']).optional(),
  value: z.string().optional(),
  effect: z.enum(['NoSchedule', 'PreferNoSchedule', 'NoExecute']).optional(),
});

export const ResourcesSchema = z.object({
  cpu: z
    .string()
    .regex(/^[0-9]+(m|)$/)
    .optional(),
  memory: z
    .string()
    .regex(/^[0-9]+(Ki|Mi|Gi)$/)
    .optional(),
  gpu: z.string().optional(),
  ephemeralStorage: z
    .string()
    .regex(/^[0-9]+(Ki|Mi|Gi)$/)
    .optional(),
  nodeSelector: z.record(z.string()).optional(),
  tolerations: z.array(TolerationSchema).optional(),
});

export type Resources = z.infer<typeof ResourcesSchema>;

// ============ Actions ============

export const DaggerActionSchema = z.object({
  type: z.literal('dagger'),
  module: z.string(),
  function: z.string().optional(),
  args: z.record(z.any()).optional(),
  image: z.string().optional(),
  cache: z.boolean().default(true),
});

export type DaggerAction = z.infer<typeof DaggerActionSchema>;

export const HttpActionSchema = z.object({
  type: z.literal('http'),
  url: z.string().url(),
  method: z.enum(['GET', 'POST', 'PUT', 'PATCH', 'DELETE']).default('POST'),
  headers: z.record(z.string()).optional(),
  body: z.string().optional(),
  expectedStatus: z.array(z.number().int()).default([200, 201, 202, 204]),
});

export type HttpAction = z.infer<typeof HttpActionSchema>;

export const ScriptActionSchema = z.object({
  type: z.literal('script'),
  runtime: z.enum(['bun', 'deno', 'node']).default('bun'),
  code: z.string().optional(),
  file: z.string().optional(),
});

export type ScriptAction = z.infer<typeof ScriptActionSchema>;

export const NoopActionSchema = z.object({
  type: z.literal('noop'),
});

export type NoopAction = z.infer<typeof NoopActionSchema>;

export const ActionSchema = z.discriminatedUnion('type', [
  DaggerActionSchema,
  HttpActionSchema,
  ScriptActionSchema,
  NoopActionSchema,
]);

export type Action = z.infer<typeof ActionSchema>;

// ============ Transition ============

export const TransitionSchema = z.object({
  id: z
    .string()
    .regex(
      /^[a-z][a-z0-9_-]*$/,
      'Transition ID must be lowercase alphanumeric with underscores/hyphens'
    ),
  inputs: z.array(ArcSchema),
  outputs: z.array(ArcSchema),
  guard: z.string().optional(), // CEL expression
  action: ActionSchema,
  resources: ResourcesSchema.optional(),
  timeout: z
    .string()
    .regex(/^[0-9]+(s|m|h)$/)
    .default('5m'),
  retries: z.number().int().min(0).max(10).default(0),
  retryBackoff: z.enum(['fixed', 'exponential']).default('exponential'),
  priority: z.number().int().min(0).max(100).default(50),
});

export type Transition = z.infer<typeof TransitionSchema>;

// ============ Workflow ============

const WorkflowNameSchema = z
  .string()
  .min(2)
  .max(63)
  .regex(/^[a-z][a-z0-9-]*[a-z0-9]$/, 'Workflow name must be DNS-compatible (lowercase, hyphens)');

const NamespaceSchema = z
  .string()
  .regex(/^[a-z][a-z0-9-]*[a-z0-9]$/)
  .default('default');

export const WorkflowSchema = z
  .object({
    version: z.literal('1.0'),
    name: WorkflowNameSchema,
    namespace: NamespaceSchema,
    metadata: MetadataSchema.optional(),
    places: z.array(PlaceSchema).min(1),
    transitions: z.array(TransitionSchema).min(1),
  })
  .refine(
    (workflow) => {
      // Validate that all arc references point to valid places
      const placeIds = new Set(workflow.places.map((p) => p.id));

      for (const transition of workflow.transitions) {
        for (const input of transition.inputs) {
          if (!placeIds.has(input.place)) {
            return false;
          }
        }
        for (const output of transition.outputs) {
          if (!placeIds.has(output.place)) {
            return false;
          }
        }
      }
      return true;
    },
    {
      message: 'All arc references must point to valid place IDs',
    }
  )
  .refine(
    (workflow) => {
      // Validate unique place IDs
      const placeIds = workflow.places.map((p) => p.id);
      return new Set(placeIds).size === placeIds.length;
    },
    {
      message: 'Place IDs must be unique',
    }
  )
  .refine(
    (workflow) => {
      // Validate unique transition IDs
      const transitionIds = workflow.transitions.map((t) => t.id);
      return new Set(transitionIds).size === transitionIds.length;
    },
    {
      message: 'Transition IDs must be unique',
    }
  );

export type Workflow = z.infer<typeof WorkflowSchema>;

// ============ Partial schemas for building ============

export const PartialPlaceSchema = PlaceSchema.partial().required({ id: true });
export const PartialTransitionSchema = TransitionSchema.partial().required({
  id: true,
  inputs: true,
  outputs: true,
  action: true,
});

// ============ Validation helpers ============

export function parseWorkflow(data: unknown): Workflow {
  return WorkflowSchema.parse(data);
}

export function safeParseWorkflow(data: unknown): z.SafeParseReturnType<unknown, Workflow> {
  return WorkflowSchema.safeParse(data);
}

export function isValidWorkflow(data: unknown): data is Workflow {
  return WorkflowSchema.safeParse(data).success;
}
