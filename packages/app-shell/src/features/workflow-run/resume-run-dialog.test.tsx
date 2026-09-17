import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createChatStore } from "@ora/chat";
import type { PreviewWorkflowRunResumeResponse } from "@ora/contracts";
import { createMemoryWorkflowRuntime } from "@ora/workflow-runtime/memory";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import {
  createTestClient,
  type TestHandlers,
} from "../../test/contracts-transport";
import { appI18n } from "../../i18n/i18n-instance";
import { ResumeRunDialog } from "./resume-run-dialog";

const PREVIEW: PreviewWorkflowRunResumeResponse = {
  resumable: true,
  failedNodes: [
    {
      nodeId: "c",
      nodeRunId: "nr-c",
      startedAt: 40n,
      checkpoint: "abc",
      checkpointError: null,
      nodeFileChanges: [
        { path: "f1", additions: 1n, deletions: 0n },
        { path: "f2", additions: 1n, deletions: 0n },
      ],
      changedSinceCheckpoint: [
        { path: "f1", additions: 1n, deletions: 0n },
        { path: "f2", additions: 1n, deletions: 0n },
        { path: "f3", additions: 1n, deletions: 0n },
      ],
    },
  ],
  nodeFilesAvailable: true,
  nodeFilesUnavailableReason: null,
  checkpointAvailable: true,
  checkpointUnavailableReason: null,
  currentSnapshotId: "snap-1",
  currentSnapshotVersion: "v1",
  publishedSnapshotId: null,
  publishedSnapshotVersion: null,
  publishedSnapshotSwitchable: false,
  publishedSnapshotIncompatibleReason: null,
};

/** Builds a dialog harness with mocked preview and resume operations. */
function renderDialog(
  preview: PreviewWorkflowRunResumeResponse,
  resumeFromFailure = vi.fn(async (request: { runId: string }) => ({
    run: {
      id: request.runId,
      workspaceId: "workspace-1",
      workflowId: "workflow-1",
      snapshotId: "snap-1",
      name: "run",
      status: "running" as const,
      state: null,
      input: null,
      output: null,
      error: null,
      startedAt: null,
      finishedAt: null,
      createdAt: 1n,
      updatedAt: 1n,
    },
    preRollbackCheckpoint: null,
  })),
) {
  const previewResume = vi.fn(async () => preview);
  const handlers: TestHandlers = {
    previewWorkflowRunResume: previewResume,
    resumeWorkflowRunFromFailure: resumeFromFailure,
  };
  const client = createTestClient(handlers);
  const runtime = createMemoryWorkflowRuntime();
  const Wrapper = createHookWrapper(
    client,
    createTestQueryClient(),
    createChatStore(client.session),
    runtime,
  );
  const onResumed = vi.fn();
  const onOpenChange = vi.fn();
  render(
    <Wrapper>
      <ResumeRunDialog
        open
        runId="run-1"
        onOpenChange={onOpenChange}
        onResumed={onResumed}
      />
    </Wrapper>,
  );
  return { previewResume, resumeFromFailure, onResumed, onOpenChange, runtime };
}

describe("ResumeRunDialog", () => {
  beforeEach(async () => {
    await appI18n.changeLanguage("zh-CN");
  });

  it("renders the preview summary with M/N/K computed from the mocked preview", async () => {
    const { runtime } = renderDialog(PREVIEW);
    expect(
      await screen.findByText(
        "节点 c：节点记录改动 2 个文件；自检查点以来共 3 个变化，其中 1 个不在节点记录里（可能是失败后手工改的）",
      ),
    ).toBeInTheDocument();
    runtime.dispose();
  });

  it("disables the checkpoint option and shows the unavailability reason", async () => {
    const { runtime } = renderDialog({
      ...PREVIEW,
      checkpointAvailable: false,
      checkpointUnavailableReason: "siblings_ran_after_checkpoint",
    });
    expect(
      await screen.findByText(
        "检查点之后有其他节点跑过，整体回滚会抹掉它们的成果",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: /整体回滚到检查点/ }),
    ).toHaveAttribute("aria-disabled", "true");
    runtime.dispose();
  });

  it("submits node_files when that option is chosen", async () => {
    const user = userEvent.setup();
    const { resumeFromFailure, onResumed, runtime } = renderDialog(PREVIEW);
    await screen.findByText(/节点 c：/);
    await user.click(
      screen.getByRole("radio", { name: /只回滚失败节点改过的文件/ }),
    );
    const dialog = screen.getByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "从失败处继续" }),
    );
    await waitFor(() => {
      expect(resumeFromFailure).toHaveBeenCalledTimes(1);
    });
    expect(resumeFromFailure.mock.calls[0]?.[0]).toEqual({
      runId: "run-1",
      rollback: "node_files",
    });
    expect(onResumed).toHaveBeenCalledTimes(1);
    runtime.dispose();
  });

  it("submits keep by default", async () => {
    const user = userEvent.setup();
    const { resumeFromFailure, runtime } = renderDialog(PREVIEW);
    await screen.findByText(/节点 c：/);
    const dialog = screen.getByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "从失败处继续" }),
    );
    await waitFor(() => {
      expect(resumeFromFailure).toHaveBeenCalledTimes(1);
    });
    expect(resumeFromFailure.mock.calls[0]?.[0]).toEqual({
      runId: "run-1",
      rollback: "keep",
    });
    runtime.dispose();
  });

  it("shows the version checkbox only when the preview carries a different published snapshot", async () => {
    const { runtime } = renderDialog(PREVIEW);
    await screen.findByText(/节点 c：/);
    expect(
      screen.queryByText(/改用当前发布版本/),
    ).not.toBeInTheDocument();
    runtime.dispose();

    const { runtime: withPublished } = renderDialog({
      ...PREVIEW,
      publishedSnapshotId: "snap-2",
      publishedSnapshotVersion: "v2",
      publishedSnapshotSwitchable: true,
    });
    expect(
      await screen.findByText("改用当前发布版本 v2 续跑（当前运行用的是 v1）"),
    ).toBeInTheDocument();
    withPublished.dispose();
  });

  it("disables the version checkbox and shows the mapped reason when not switchable", async () => {
    const { runtime } = renderDialog({
      ...PREVIEW,
      publishedSnapshotId: "snap-2",
      publishedSnapshotVersion: "v2",
      publishedSnapshotSwitchable: false,
      publishedSnapshotIncompatibleReason: "node_missing:b",
    });
    expect(
      await screen.findByText("新版本删掉了节点 b"),
    ).toBeInTheDocument();
    expect(screen.getByRole("checkbox")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    runtime.dispose();
  });

  it("submits snapshotId when the published-version checkbox is checked", async () => {
    const user = userEvent.setup();
    const { resumeFromFailure, runtime } = renderDialog({
      ...PREVIEW,
      publishedSnapshotId: "snap-2",
      publishedSnapshotVersion: "v2",
      publishedSnapshotSwitchable: true,
    });
    await screen.findByText("改用当前发布版本 v2 续跑（当前运行用的是 v1）");
    await user.click(screen.getByRole("checkbox"));
    const dialog = screen.getByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "从失败处继续" }),
    );
    await waitFor(() => {
      expect(resumeFromFailure).toHaveBeenCalledTimes(1);
    });
    expect(resumeFromFailure.mock.calls[0]?.[0]).toEqual({
      runId: "run-1",
      rollback: "keep",
      snapshotId: "snap-2",
    });
    runtime.dispose();
  });

  it("announces the iteration composite as the restart unit", async () => {
    await appI18n.changeLanguage("zh-CN");
    const user = userEvent.setup();
    const compositePreview: PreviewWorkflowRunResumeResponse = {
      ...PREVIEW,
      failedNodes: [
        {
          nodeId: "fix",
          nodeRunId: "nr-fix-1",
          startedAt: 40n,
          checkpoint: "abc",
          checkpointError: null,
          nodeFileChanges: [],
          changedSinceCheckpoint: [],
          resumeUnitNodeId: "iter",
        },
      ],
      nodeFilesAvailable: false,
      nodeFilesUnavailableReason: "composite_region",
    };
    const { resumeFromFailure, onResumed, runtime } =
      renderDialog(compositePreview);
    expect(
      await screen.findByText("迭代节点「iter」将从第一轮重新开始"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "失败发生在迭代节点内部，只能保留现状或整体回滚到迭代开始前的检查点",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: /只回滚失败节点改过的文件/ }),
    ).toHaveAttribute("aria-disabled", "true");
    const dialog = screen.getByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "从失败处继续" }),
    );
    await waitFor(() => {
      expect(resumeFromFailure).toHaveBeenCalledTimes(1);
    });
    expect(resumeFromFailure.mock.calls[0]?.[0]).toEqual({
      runId: "run-1",
      rollback: "keep",
    });
    expect(onResumed).toHaveBeenCalledTimes(1);
    runtime.dispose();
  });
});

