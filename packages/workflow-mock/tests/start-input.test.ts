import { describe, expect, it } from "vitest";
import {
  resolveWorkflowInputFieldType,
  resolveWorkflowInputVariableValueType,
  workflowInputFieldProducesValueType,
  workflowInputFieldValueType,
  WORKFLOW_INPUT_FIELD_TYPES,
  WORKFLOW_JSON_FIELD_VALUE_TYPES,
} from "../src";

describe("Start input field types", () => {
  it("maps every supported field control to its variable-pool type", () => {
    expect(
      Object.fromEntries(
        WORKFLOW_INPUT_FIELD_TYPES.map((fieldType) => [
          fieldType,
          workflowInputFieldValueType(fieldType),
        ]),
      ),
    ).toEqual({
      "text-input": "string",
      paragraph: "string",
      select: "string",
      number: "number",
      checkbox: "boolean",
      file: "file",
      "file-list": "array[file]",
      json: "any",
    });
  });

  it("derives controls for legacy declarations", () => {
    expect(resolveWorkflowInputFieldType({ valueType: "integer" })).toBe(
      "number",
    );
    expect(resolveWorkflowInputFieldType({ valueType: "array[file]" })).toBe(
      "file-list",
    );
    expect(resolveWorkflowInputFieldType({ valueType: "object" })).toBe("json");
  });

  it("lets the JSON control declare every structured pool type and nothing else", () => {
    for (const valueType of WORKFLOW_JSON_FIELD_VALUE_TYPES) {
      expect(workflowInputFieldProducesValueType("json", valueType)).toBe(true);
      // Every structured declaration must keep resolving back to the JSON control so
      // re-saving an edited variable never rewrites its declared pool type.
      expect(resolveWorkflowInputFieldType({ valueType })).toBe("json");
    }
    // Scalar and file pool types belong to their dedicated controls.
    for (const valueType of [
      "string",
      "number",
      "integer",
      "boolean",
      "secret",
      "file",
      "array[file]",
    ] as const) {
      expect(workflowInputFieldProducesValueType("json", valueType)).toBe(
        false,
      );
    }
  });

  it("keeps single-type controls locked to the pool type they produce", () => {
    expect(workflowInputFieldProducesValueType("text-input", "string")).toBe(
      true,
    );
    expect(workflowInputFieldProducesValueType("paragraph", "secret")).toBe(
      false,
    );
    expect(workflowInputFieldProducesValueType("number", "number")).toBe(true);
    expect(workflowInputFieldProducesValueType("checkbox", "boolean")).toBe(
      true,
    );
    expect(workflowInputFieldProducesValueType("checkbox", "string")).toBe(
      false,
    );
    expect(workflowInputFieldProducesValueType("file", "file")).toBe(true);
    expect(
      workflowInputFieldProducesValueType("file-list", "array[file]"),
    ).toBe(true);
    expect(workflowInputFieldProducesValueType("file-list", "array[any]")).toBe(
      false,
    );
  });

  it("preserves compatible declared pool types when reopening a variable", () => {
    expect(
      resolveWorkflowInputVariableValueType({ valueType: "array[string]" }),
    ).toBe("array[string]");
    expect(
      resolveWorkflowInputVariableValueType({ valueType: "array[object]" }),
    ).toBe("array[object]");
    expect(resolveWorkflowInputVariableValueType({ valueType: "object" })).toBe(
      "object",
    );
    expect(
      resolveWorkflowInputVariableValueType({
        fieldType: "json",
        valueType: "array[string]",
      }),
    ).toBe("array[string]");
    expect(
      resolveWorkflowInputVariableValueType({
        fieldType: "json",
        valueType: "object",
      }),
    ).toBe("object");
  });

  it("falls back to the control's pool type for incompatible declarations", () => {
    expect(
      resolveWorkflowInputVariableValueType({
        fieldType: "json",
        valueType: "string",
      }),
    ).toBe("any");
    expect(
      resolveWorkflowInputVariableValueType({ valueType: "integer" }),
    ).toBe("number");
    expect(resolveWorkflowInputVariableValueType({ valueType: "secret" })).toBe(
      "string",
    );
  });
});
