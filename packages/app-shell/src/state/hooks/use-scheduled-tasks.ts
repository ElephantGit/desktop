import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  CreateScheduledTaskRequest,
  ScheduledTask,
  UpdateScheduledTaskRequest,
} from "@ora/contracts";
import { useContractsClient } from "../../contracts-client-context";

export const scheduledTaskKeys = {
  all: ["scheduledTasks"] as const,
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
