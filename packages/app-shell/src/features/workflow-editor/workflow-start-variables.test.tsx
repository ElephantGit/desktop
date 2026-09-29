import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { WorkflowInputVariable } from "@ora/workflow-mock";
import { appI18n } from "../../i18n/i18n-instance";
import { AppI18nProvider } from "../../i18n/i18n";
import { WorkflowStartVariables } from "./workflow-start-variables";

/** The agent-authored declaration from the reported defect: JSON control, object type, array value. */
const REQUIREMENTS_VALUE = [
  "需求一：为 /health 接口补充单元测试。",
  "需求二：为文件上传接口增加大小上限校验。",
  "需求三：在 README 中补充本地启动步骤。",
];

function renderStartVariables(
  variables: WorkflowInputVariable[],
): ReturnType<typeof render> & { onChange: ReturnType<typeof vi.fn> } {
  const onChange = vi.fn();
  const view = render(
    <AppI18nProvider>
      <WorkflowStartVariables variables={variables} onChange={onChange} />
    </AppI18nProvider>,
  );
  return { ...view, onChange };
}

describe("WorkflowStartVariables", () => {
  it("saves the agent-authored JSON array once the declared pool type matches it", async () => {
    await appI18n.changeLanguage("zh-CN");
    const user = userEvent.setup();
    const { onChange } = renderStartVariables([
      {
        name: "requirements",
        displayName: "需求清单",
        fieldType: "json",
        valueType: "object",
        required: false,
        value: REQUIREMENTS_VALUE,
      },
    ]);

    await user.click(
      screen.getByRole("button", { name: "编辑变量 requirements" }),
    );
    await user.click(screen.getByRole("button", { name: "保存" }));
    // The persisted declaration says `object`, so the array value is rejected until the
    // declaration is repaired to a structured type that matches the value.
    expect(screen.getByText("值与 object 类型不匹配。")).toBeInTheDocument();

    await user.click(screen.getByRole("combobox", { name: "值类型" }));
    await user.click(
      await screen.findByRole("option", { name: "array[string]" }),
    );
    expect(
      screen.queryByText("值与 object 类型不匹配。"),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "保存" }));

    expect(onChange).toHaveBeenCalledWith([
      {
        name: "requirements",
        displayName: "需求清单",
        fieldType: "json",
        valueType: "array[string]",
        required: false,
        value: REQUIREMENTS_VALUE,
      },
    ]);
  });

  it("preserves a legacy array declaration instead of collapsing it to object", async () => {
    await appI18n.changeLanguage("zh-CN");
    const user = userEvent.setup();
    const { onChange } = renderStartVariables([
      {
        name: "prs",
        valueType: "array[object]",
        value: [{ number: 1 }, { number: 2 }],
      },
    ]);

    await user.click(screen.getByRole("button", { name: "编辑变量 prs" }));
    await user.click(screen.getByRole("button", { name: "保存" }));

    expect(onChange).toHaveBeenCalledWith([
      {
        name: "prs",
        fieldType: "json",
        valueType: "array[object]",
        required: false,
        value: [{ number: 1 }, { number: 2 }],
      },
    ]);
  });

  it("accepts a JSON array as the initial value of a new JSON variable", async () => {
    await appI18n.changeLanguage("zh-CN");
    const user = userEvent.setup();
    const { onChange } = renderStartVariables([]);

    await user.click(screen.getByRole("button", { name: "添加变量" }));
    await user.type(screen.getByLabelText("变量名称"), "requirements");
    await user.click(screen.getByRole("combobox", { name: "字段类型" }));
    await user.click(await screen.findByRole("option", { name: /JSON/ }));
    // JSON text contains bracket characters that user-event parses as key descriptors, so
    // the textarea update follows the same fireEvent.change pattern as the schema editor.
    fireEvent.change(screen.getByLabelText(/初始值/), {
      target: { value: '["需求一：补充单元测试。"]' },
    });
    await user.click(screen.getByRole("button", { name: "保存" }));

    expect(onChange).toHaveBeenCalledWith([
      {
        name: "requirements",
        fieldType: "json",
        valueType: "any",
        required: false,
        value: ["需求一：补充单元测试。"],
      },
    ]);
  });
});
