import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { SearchableChoice } from "./searchable-choice";

const longName = `desktop/${"q".repeat(48)}`;
const options = [
  { label: "Ora Desktop", value: "project-1", keywords: "project-1" },
  { label: "Another project", value: "project-2", keywords: "project-2" },
  { label: longName, value: "project-long", keywords: "project-long" },
];

function ChoiceHarness({ initial = "project-1" }: { initial?: string }) {
  const [value, setValue] = useState(initial);
  return (
    <label>
      项目
      <SearchableChoice
        id="scheduled-task-project"
        value={value}
        options={options}
        placeholder="搜索项目"
        emptyLabel="没有匹配的项目"
        onValueChange={setValue}
      />
    </label>
  );
}

describe("SearchableChoice", () => {
  it("filters options by label on the client", async () => {
    const user = userEvent.setup();
    render(<ChoiceHarness />);
    const search = screen.getByRole("combobox", { name: "项目" });
    await user.click(search);
    await user.clear(search);
    await user.type(search, "Another");

    expect(
      await screen.findByRole("option", { name: "Another project" }),
    ).toBeTruthy();
    expect(screen.queryByRole("option", { name: "Ora Desktop" })).toBeNull();
  });

  it("filters by the stored id behind a display name", async () => {
    const user = userEvent.setup();
    render(<ChoiceHarness />);
    const search = screen.getByRole("combobox", { name: "项目" });
    await user.click(search);
    await user.clear(search);
    await user.type(search, "project-2");

    expect(
      await screen.findByRole("option", { name: "Another project" }),
    ).toBeTruthy();
    expect(screen.queryByRole("option", { name: "Ora Desktop" })).toBeNull();
  });

  it("shows an empty state when nothing matches", async () => {
    const user = userEvent.setup();
    render(<ChoiceHarness />);
    const search = screen.getByRole("combobox", { name: "项目" });
    await user.click(search);
    await user.clear(search);
    await user.type(search, "missing-project");

    expect(screen.getByText("没有匹配的项目")).toBeTruthy();
    expect(screen.queryByRole("option", { name: "Ora Desktop" })).toBeNull();
  });

  it("wraps a long option name inside the menu", async () => {
    const user = userEvent.setup();
    render(<ChoiceHarness />);
    await user.click(screen.getByRole("combobox", { name: "项目" }));

    const option = await screen.findByRole("option", { name: longName });
    expect(option.querySelector("span")?.className).toContain(
      "wrap-break-word",
    );
    expect(option.querySelector("span")?.className).toContain(
      "whitespace-normal",
    );
  });
});
