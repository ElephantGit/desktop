import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  CreateScheduledTaskRequest,
  ScheduledTask,
  UpdateScheduledTaskRequest,
} from "@ora/contracts";
import { useContractsClient } from "../../contracts-client-context";

export const scheduledTaskKeys = {
  all: ["scheduledTasks"] as const,
  history: (taskId: string) => ["scheduledTasks", taskId, "history"] as const,
  runArtifacts: (runId: string) =>
    ["scheduledTasks", "runArtifacts", runId] as const,
  artifactPreview: (runId: string, relativePath: string) =>
    ["scheduledTasks", "runArtifacts", runId, "preview", relativePath] as const,
};

/** Loads every visible Scheduled Task definition. */
export function useScheduledTasks() {
  const client = useContractsClient();
  return useQuery({
    queryKey: scheduledTaskKeys.all,
    queryFn: () =>
      client.scheduledTask.list({}).then((response) => response.tasks),
  });
}

/** Loads newest-first persisted run and missed facts for one task. */
export function useScheduledTaskHistory(taskId: string) {
  const client = useContractsClient();
  return useQuery({
    queryKey: scheduledTaskKeys.history(taskId),
    queryFn: () =>
      client.scheduledTask
        .listHistory({ taskId })
        .then((response) => response.entries),
  });
}

/** Loads one run's artifact manifest and rejected-candidate log. */
export function useScheduledRunArtifacts(runId: string, enabled: boolean) {
  const client = useContractsClient();
  return useQuery({
    queryKey: scheduledTaskKeys.runArtifacts(runId),
    queryFn: () => client.scheduledTask.listRunArtifacts({ runId }),
    enabled,
  });
}

/** Loads one artifact's bounded text preview while its dialog is open. */
export function useScheduledRunArtifactPreview(
  runId: string,
  relativePath: string | null,
) {
  const client = useContractsClient();
  return useQuery({
    queryKey: scheduledTaskKeys.artifactPreview(runId, relativePath ?? ""),
    queryFn: () => {
      if (relativePath === null) return Promise.resolve(null);
      return client.scheduledTask.readRunArtifact({
        runId,
        relativePath,
      });
    },
    enabled: relativePath !== null,
  });
}

function useRefreshScheduledTasks() {
  const queryClient = useQueryClient();
  return () =>
    queryClient.invalidateQueries({ queryKey: scheduledTaskKeys.all });
}

/** Creates one Scheduled Task definition and refreshes the list. */
export function useCreateScheduledTask() {
  const client = useContractsClient();
  const refresh = useRefreshScheduledTasks();
  return useMutation({
    mutationFn: (request: CreateScheduledTaskRequest) =>
      client.scheduledTask.create(request).then((response) => response.task),
    onSuccess: refresh,
  });
}

/** Replaces mutable Scheduled Task fields and refreshes the list. */
export function useUpdateScheduledTask() {
  const client = useContractsClient();
  const refresh = useRefreshScheduledTasks();
  return useMutation({
    mutationFn: (request: UpdateScheduledTaskRequest) =>
      client.scheduledTask.update(request).then((response) => response.task),
    onSuccess: refresh,
  });
}

/** Soft-deletes one Scheduled Task definition and refreshes the list. */
export function useDeleteScheduledTask() {
  const client = useContractsClient();
  const refresh = useRefreshScheduledTasks();
  return useMutation({
    mutationFn: (task: ScheduledTask) =>
      client.scheduledTask.delete({ taskId: task.id }),
    onSuccess: refresh,
  });
}

/** Revokes unattended consent, disables the task, and refreshes the list. */
export function useRevokeScheduledTaskApproval() {
  const client = useContractsClient();
  const refresh = useRefreshScheduledTasks();
  return useMutation({
    mutationFn: (task: ScheduledTask) =>
      client.scheduledTask
        .revokeApproval({ taskId: task.id })
        .then((response) => response.task),
    onSuccess: refresh,
  });
}
