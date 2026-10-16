import { describe, expect, it } from "vitest";
import { scheduledTaskTranslations } from "./translations";

describe("Scheduled Task approval disclosure", () => {
  it.each([
    [
      "zh-CN" as const,
      ["不是沙箱", "任意命令", "工作树外", "本机凭据", "git push", "模型额度"],
    ],
    [
      "en-US" as const,
      [
        "not a sandbox",
        "arbitrary commands",
        "outside the worktree",
        "local credentials",
        "git push",
        "model usage",
      ],
    ],
  ])("lists every unattended execution risk in %s", (locale, risks) => {
    const disclosure =
      scheduledTaskTranslations[locale]["scheduledTask.approvalHelp"];

    for (const risk of risks) {
      expect(disclosure).toContain(risk);
    }
  });
});
