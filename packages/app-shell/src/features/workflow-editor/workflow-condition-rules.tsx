import { Fragment } from "react";
import { useTranslation } from "react-i18next";
import { IconPlus, IconTrash } from "@tabler/icons-react";
import type {
  WorkflowChoice,
  WorkflowConditionComparison,
  WorkflowConditionLogic,
  WorkflowVariableCatalogEntry,
  WorkflowVariableValueType,
} from "@ora/workflow-mock";
import {
  Button,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  cn,
} from "@ora/ui";
import { WorkflowVariableDisplay } from "./workflow-variable-display";
import { WorkflowVariableSelectGroups } from "./workflow-variable-list";

/** Edits one condition group shared by branch routing and Loop termination. */
export function WorkflowConditionRules({
  conditions,
  logic,
  onChange,
  operators,
  variableCatalog,
  logicLabel,
  minimumConditions = 0,
}: {
  conditions: WorkflowConditionComparison[];
  logic: WorkflowConditionLogic;
  onChange: (group: {
    logic: WorkflowConditionLogic;
    conditions: WorkflowConditionComparison[];
  }) => void;
  operators: WorkflowChoice[];
  variableCatalog: WorkflowVariableCatalogEntry[];
  logicLabel: string;
  minimumConditions?: number;
}) {
  const { t } = useTranslation();
  const logicOptions = [
    { value: "and", label: t("settings.workflow.condition.logicAnd") },
    { value: "or", label: t("settings.workflow.condition.logicOr") },
  ];
  function updateComparison(
    index: number,
    patch: Partial<WorkflowConditionComparison>,
  ): void {
    onChange({
      logic,
      conditions: conditions.map((item, candidate) =>
        candidate === index ? { ...item, ...patch } : item,
      ),
    });
  }
  function addComparison(): void {
    onChange({
      logic,
      conditions: [...conditions, { variableSelector: [], operator: "" }],
    });
  }
  function removeComparison(index: number): void {
    onChange({
      logic,
      conditions: conditions.filter((_, candidate) => candidate !== index),
    });
  }
  return (
    <div
      className={cn(
        "px-1",
        conditions.length > 1 && "relative ml-2 border-l border-border/80 pl-3",
      )}
    >
      {conditions.map((comparison, comparisonIndex) => {
        const variableType = variableCatalog.find(
          (entry) =>
            selectorToText(entry.selector) ===
            selectorToText(comparison.variableSelector),
        )?.valueType;
        const availableOperators = operators.filter((operator) =>
          operatorSupportsType(operator.value, variableType),
        );
        const needsValue = ![
          "empty",
          "not_empty",
          "exists",
          "not_exists",
        ].includes(comparison.operator);
        return (
          <Fragment key={comparisonIndex}>
            {comparisonIndex > 0 && (
              <div className="relative h-7">
                <Select
                  value={logic}
                  onValueChange={(logic) => {
                    if (logic === "and" || logic === "or") {
                      onChange({ logic, conditions });
                    }
                  }}
                >
                  <SelectTrigger
                    aria-label={`${logicLabel} · ${comparisonIndex}`}
                    className="absolute -left-6 top-1/2 h-6 w-auto min-w-10 -translate-y-1/2 justify-center gap-1 rounded-md border-blue-200 bg-background px-1.5 text-[10px] font-semibold text-blue-600 shadow-sm dark:border-blue-800 dark:text-blue-400"
                  >
                    <span>{logic.toUpperCase()}</span>
                  </SelectTrigger>
                  <SelectContent>
                    {logicOptions.map((logic) => (
                      <SelectItem key={logic.value} value={logic.value}>
                        {logic.value.toUpperCase()} · {logic.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
            <div className="flex items-start gap-1.5">
              <div className="min-w-0 flex-1 space-y-1.5 rounded-lg bg-muted/70 p-2">
                <Select
                  value={selectorToText(comparison.variableSelector)}
                  onValueChange={(value) => {
                    if (value !== null) {
                      const nextType = variableCatalog.find(
                        (entry) => selectorToText(entry.selector) === value,
                      )?.valueType;
                      updateComparison(comparisonIndex, {
                        variableSelector: textToSelector(value),
                        ...(nextType !== variableType
                          ? { operator: "", value: undefined }
                          : {}),
                      });
                    }
                  }}
                >
                  <SelectTrigger
                    className="h-8 w-full bg-background"
                    aria-label={t("settings.workflow.field.variable", {
                      index: comparisonIndex + 1,
                    })}
                  >
                    <VariableSelectValue
                      catalog={variableCatalog}
                      selector={comparison.variableSelector}
                      placeholder={t(
                        "settings.workflow.condition.variablePlaceholder",
                      )}
                    />
                  </SelectTrigger>
                  <SelectContent
                    alignItemWithTrigger={false}
                    align="start"
                    className="w-70 min-w-70 max-w-70"
                  >
                    <WorkflowVariableSelectGroups
                      variables={variableCatalog}
                      globalVariablesLabel={t(
                        "settings.workflow.globalVariables",
                      )}
                    />
                  </SelectContent>
                </Select>
                <div className="flex min-w-0 gap-1.5">
                  <Select
                    key={selectorToText(comparison.variableSelector)}
                    value={comparison.operator}
                    onValueChange={(operator) => {
                      if (operator !== null) {
                        updateComparison(comparisonIndex, {
                          operator,
                          value: undefined,
                        });
                      }
                    }}
                  >
                    <SelectTrigger
                      aria-label={t("settings.workflow.field.operator", {
                        index: comparisonIndex + 1,
                      })}
                      className="h-8 w-20 shrink-0 bg-background"
                    >
                      <LocalizedSelectValue
                        options={operators}
                        value={comparison.operator}
                        placeholder={t(
                          "settings.workflow.condition.operatorPlaceholder",
                        )}
                      />
                    </SelectTrigger>
                    <SelectContent>
                      {availableOperators.map((operator) => (
                        <SelectItem key={operator.value} value={operator.value}>
                          {operator.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {needsValue &&
                    (variableType === "boolean" ? (
                      <Select
                        value={
                          comparison.value === undefined
                            ? ""
                            : String(comparison.value)
                        }
                        onValueChange={(value) => {
                          if (value === "true" || value === "false")
                            updateComparison(comparisonIndex, {
                              value: value === "true",
                            });
                        }}
                      >
                        <SelectTrigger
                          className="h-8 min-w-0 flex-1 bg-background"
                          aria-label={t("settings.workflow.field.value", {
                            index: comparisonIndex + 1,
                          })}
                        >
                          <SelectValue
                            placeholder={t(
                              "settings.workflow.condition.valuePlaceholder",
                            )}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">true</SelectItem>
                          <SelectItem value="false">false</SelectItem>
                        </SelectContent>
                      </Select>
                    ) : (
                      <Input
                        value={comparisonValueToText(comparison.value)}
                        aria-label={t("settings.workflow.field.value", {
                          index: comparisonIndex + 1,
                        })}
                        placeholder={t(
                          "settings.workflow.condition.valuePlaceholder",
                        )}
                        className="h-8 min-w-0 flex-1 bg-background"
                        onChange={(event) =>
                          updateComparison(comparisonIndex, {
                            value:
                              variableType === "string" ||
                              variableType === "secret"
                                ? event.target.value
                                : parseComparisonValue(event.target.value),
                          })
                        }
                      />
                    ))}
                </div>
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="mt-1 shrink-0 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                disabled={conditions.length <= minimumConditions}
                aria-label={t("settings.workflow.condition.removeRule")}
                onClick={() => removeComparison(comparisonIndex)}
              >
                <IconTrash className="size-3.5" />
              </Button>
            </div>
          </Fragment>
        );
      })}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="mt-3 w-fit justify-start border border-border bg-background shadow-sm"
        onClick={addComparison}
      >
        <IconPlus />
        {t("settings.workflow.condition.addRule")}
      </Button>
    </div>
  );
}

/** Renders the chosen variable with its node identity, or the selector text as a fallback. */
function VariableSelectValue({
  catalog,
  selector,
  placeholder,
}: {
  catalog: WorkflowVariableCatalogEntry[];
  selector: string[];
  placeholder: string;
}) {
  const { t } = useTranslation();
  const selectorText = selectorToText(selector);
  if (selectorText === "") {
    return <SelectValue placeholder={placeholder} />;
  }
  const variable = catalog.find(
    (candidate) => selectorToText(candidate.selector) === selectorText,
  );
  return (
    <SelectValue placeholder={placeholder}>
      {variable === undefined ? (
        selectorText
      ) : (
        <WorkflowVariableDisplay
          variable={variable}
          nodeName={
            variable.sourceNodeTitle ??
            (variable.scope === "global"
              ? t("settings.workflow.globalVariables")
              : variable.sourceNodeId)
          }
        />
      )}
    </SelectValue>
  );
}

/**
 * Renders the selected choice's localized label. Base UI's value element shows
 * the raw value, which equals the label for simple catalogs but not for
 * operator/operation choices, so the label must be resolved explicitly.
 */
export function LocalizedSelectValue({
  options,
  value,
  placeholder,
}: {
  options: WorkflowChoice[];
  value: string;
  placeholder?: string;
}) {
  if (value === "" && placeholder !== undefined) {
    return <SelectValue placeholder={placeholder} />;
  }
  return (
    <SelectValue placeholder={placeholder}>
      {(selected) =>
        options.find((option) => option.value === (selected ?? value))?.label ??
        String(selected ?? value)
      }
    </SelectValue>
  );
}

/** Joins a selector array into its dotted text form for the editor input. */
function selectorToText(selector: string[]): string {
  return selector.join(".");
}

/** Splits the dotted selector text into `[nodeId, root, ...nested]` parts. */
function textToSelector(text: string): string[] {
  return text
    .split(".")
    .map((part) => part.trim())
    .filter((part) => part !== "");
}

/** Coerces the comparison value's text form into a JSON-ish value for the backend. */
function parseComparisonValue(text: string): unknown {
  const trimmed = text.trim();
  if (trimmed === "true") {
    return true;
  }
  if (trimmed === "false") {
    return false;
  }
  if (trimmed !== "" && Number.isFinite(Number(trimmed))) {
    return Number(trimmed);
  }
  return text;
}

/** Renders a comparison value back into its editable text form. */
function comparisonValueToText(value: unknown): string {
  if (value === undefined || value === null) {
    return "";
  }
  return typeof value === "string" ? value : JSON.stringify(value);
}

/** Offers comparisons appropriate for the selected value, preserving unknown-type editing. */
function operatorSupportsType(
  operator: string,
  valueType: WorkflowVariableValueType | undefined,
): boolean {
  if (valueType === undefined || valueType === "any") return true;
  switch (operator) {
    case "greater_than":
    case "greater_than_or_equal":
    case "less_than":
    case "less_than_or_equal":
      return valueType === "number" || valueType === "integer";
    case "contains":
    case "not_contains":
      return (
        valueType === "string" ||
        valueType === "secret" ||
        valueType.startsWith("array")
      );
    case "starts_with":
    case "ends_with":
      return valueType === "string" || valueType === "secret";
    default:
      return true;
  }
}
