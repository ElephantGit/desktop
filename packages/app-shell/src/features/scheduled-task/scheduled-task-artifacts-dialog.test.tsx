import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { createChatStore } from "@ora/chat";
import { TooltipProvider } from "@ora/ui";
import type { PlatformAdapter } from "../../platform";
import { AppI18nProvider } from "../../i18n/i18n";
import { PlatformProvider } from "../../platform";
import {
  createTestClient,
  type TestHandlers,
} from "../../test/contracts-transport";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import { createStubPlatform } from "../../test/stub-platform";
import { ScheduledTaskArtifactsDialog } from "./scheduled-task-artifacts-dialog";

const ARTIFACTS: TestHandlers = {
  listScheduledRunArtifacts: async () => ({
    artifacts: [
      {
        relativePath: "nested/report.md",
        kind: "markdown",
        sizeBytes: 8n,
        registeredAt: 10n,
      },
    ],
    rejections: [{ name: "secret.pdf", reason: "unsupported_type" as const }],
  }),
  readScheduledRunArtifact: async ({ relativePath }) => ({
    content: `preview of ${relativePath}`,
    truncated: false,
  }),
};

function renderDialog(platform: PlatformAdapter = createStubPlatform()) {
  const client = createTestClient(ARTIFACTS);
  const Wrapper = createHookWrapper(
    client,
    createTestQueryClient(),
    createChatStore(client.session),
  );
  return render(
    <Wrapper>
      <AppI18nProvider>
        <PlatformProvider adapter={platform}>
          <TooltipProvider>
            <ScheduledTaskArtifactsDialog
              runId="run-1"
              onOpenChange={() => undefined}
            />
          </TooltipProvider>
        </PlatformProvider>
      </AppI18nProvider>
    </Wrapper>,
  );
}

describe("ScheduledTaskArtifactsDialog", () => {
  it("shows the empty state for a run that produced no artifacts", async () => {
    const client = createTestClient({
      listScheduledRunArtifacts: async () => ({
        artifacts: [],
        rejections: [],
      }),
    });
    const Wrapper = createHookWrapper(
      client,
      createTestQueryClient(),
      createChatStore(client.session),
    );
    render(
      <Wrapper>
        <AppI18nProvider>
          <PlatformProvider adapter={createStubPlatform()}>
            <ScheduledTaskArtifactsDialog
              runId="run-1"
              onOpenChange={() => undefined}
            />
          </PlatformProvider>
        </AppI18nProvider>
      </Wrapper>,
    );

    expect(await screen.findByText("本次运行没有产物。")).toBeTruthy();
  });

  it("lists collected artifacts and rejected candidates", async () => {
    renderDialog();

    expect(await screen.findByText("nested/report.md")).toBeTruthy();
    expect(screen.getByText("8 B")).toBeTruthy();
    expect(screen.getByText("secret.pdf")).toBeTruthy();
    expect(screen.getByText("类型不在白名单")).toBeTruthy();
  });

  it("previews a selected artifact through the contracts client", async () => {
    renderDialog();
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: "预览" }));

    expect(await screen.findByText("preview of nested/report.md")).toBeTruthy();
  });

  it("exports through the host save dialog when the platform supports it", async () => {
    const downloads: Array<{
      runId: string;
      relativePath: string;
      destination: string;
    }> = [];
    const platform: PlatformAdapter = {
      ...createStubPlatform(),
      selectSavePath: async () => "/tmp/report.md",
      scheduledArtifacts: {
        download: async (runId, relativePath, destination) => {
          downloads.push({ runId, relativePath, destination });
        },
      },
    };
    renderDialog(platform);
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: "下载" }));

    expect(downloads).toEqual([
      {
        runId: "run-1",
        relativePath: "nested/report.md",
        destination: "/tmp/report.md",
      },
    ]);
  });
});
