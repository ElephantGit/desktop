import type {
  WorkflowInputFieldType,
  WorkflowInputVariable,
  WorkflowVariableValueType,
} from "./node-data";

/** Start field types shown by the editor, in the same order as the configuration menu. */
export const WORKFLOW_INPUT_FIELD_TYPES = [
  "text-input",
  "paragraph",
  "select",
  "number",
  "checkbox",
  "file",
  "file-list",
  "json",
] as const satisfies readonly WorkflowInputFieldType[];

/** Pool types a JSON control may declare: arbitrary JSON covers objects, arrays, and mixed values.
 *
 * This is the single source of truth for which pool types resolve back to the JSON control:
 * `resolveWorkflowInputFieldType` derives the control through `isWorkflowJsonFieldValueType`,
 * so a saved (control, type) pair always round-trips without rewriting the declaration, and a
 * new pool type that is not listed here is a compile error there instead of a silent collapse.
 */
export const WORKFLOW_JSON_FIELD_VALUE_TYPES = [
  "object",
  "any",
  "array",
  "array[string]",
  "array[number]",
  "array[object]",
  "array[boolean]",
  "array[any]",
] as const satisfies readonly WorkflowVariableValueType[];

/** The pool types the JSON control can declare, as a union. */
export type WorkflowJsonFieldValueType =
  (typeof WORKFLOW_JSON_FIELD_VALUE_TYPES)[number];

/** Returns the variable-pool type produced by one Start form control. */
export function workflowInputFieldValueType(
  fieldType: WorkflowInputFieldType,
): WorkflowVariableValueType {
  switch (fieldType) {
    case "text-input":
    case "paragraph":
    case "select":
      return "string";
    case "number":
      return "number";
    case "checkbox":
      return "boolean";
    case "file":
      return "file";
    case "file-list":
      return "array[file]";
    case "json":
      // Arbitrary JSON has no single shape, so the default declaration is `any`; the
      // editor can narrow it to any type in WORKFLOW_JSON_FIELD_VALUE_TYPES.
      return "any";
  }
}

/** Returns whether one Start form control can produce the declared pool type.
 *
 * Single-shape controls accept exactly the type they emit; the JSON control accepts every
 * structured type because its textarea can hold any JSON document.
 */
export function workflowInputFieldProducesValueType(
  fieldType: WorkflowInputFieldType,
  valueType: WorkflowVariableValueType,
): boolean {
  return fieldType === "json"
    ? isWorkflowJsonFieldValueType(valueType)
    : workflowInputFieldValueType(fieldType) === valueType;
}

/** Narrows a string to a pool type the JSON control can declare.
 *
 * The single membership check for `WORKFLOW_JSON_FIELD_VALUE_TYPES`; every consumer (this
 * package and the editor) routes through it so the supported set cannot drift per call site.
 */
export function isWorkflowJsonFieldValueType(
  value: string,
): value is WorkflowJsonFieldValueType {
  return (WORKFLOW_JSON_FIELD_VALUE_TYPES as readonly string[]).includes(value);
}

/** Returns the pool type an existing declaration keeps when its control still produces it.
 *
 * Editors call this when reopening a variable so compatible declarations (for example a
 * legacy `array[string]` Start input) survive editing instead of collapsing to the control
 * default.
 */
export function resolveWorkflowInputVariableValueType(
  variable: Pick<WorkflowInputVariable, "fieldType" | "valueType">,
): WorkflowVariableValueType {
  const fieldType = resolveWorkflowInputFieldType(variable);
  return workflowInputFieldProducesValueType(fieldType, variable.valueType)
    ? variable.valueType
    : workflowInputFieldValueType(fieldType);
}

/** Resolves legacy Start declarations that predate explicit form control metadata.
 *
 * Structured types derive the JSON control through the shared membership check instead of a
 * hand-maintained case list, so `WORKFLOW_JSON_FIELD_VALUE_TYPES` stays the only place that
 * enumerates them; a pool type missing from both the explicit cases and the constant is a
 * compile error (the `default` arm only accepts `never`), not a silent control change.
 */
export function resolveWorkflowInputFieldType(
  variable: Pick<WorkflowInputVariable, "fieldType" | "valueType">,
): WorkflowInputFieldType {
  if (variable.fieldType !== undefined) return variable.fieldType;
  switch (variable.valueType) {
    case "number":
    case "integer":
      return "number";
    case "boolean":
      return "checkbox";
    case "file":
      return "file";
    case "array[file]":
      return "file-list";
    case "string":
    case "secret":
      return "text-input";
    default:
      if (isWorkflowJsonFieldValueType(variable.valueType)) return "json";
      // Exhaustiveness guard: with the current vocabulary every remaining pool type is a
      // JSON-control member, so this only compiles while that invariant holds.
      return unreachablePoolType(variable.valueType);
  }
}

/** Fails at compile time for pool types no Start control can produce. */
function unreachablePoolType(value: never): never {
  throw new Error(`unsupported workflow variable value type: ${value}`);
}
