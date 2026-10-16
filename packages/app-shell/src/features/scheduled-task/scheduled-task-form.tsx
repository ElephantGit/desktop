import { useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import type {
  Project,
  ScheduledTask,
  ScheduledTaskSchedule,
  ScheduledTaskWeekday,
} from "@ora/contracts";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Spinner,
  Switch,
  Textarea,
} from "@ora/ui";
import { localizeContractError } from "../../i18n/contract-error";
import {
  useCreateScheduledTask,
  useUpdateScheduledTask,
} from "../../state/hooks/use-scheduled-tasks";
import type { AgentEntry } from "../../state/hooks/use-agent-catalog";
import { WEEKDAYS, scheduleTime } from "./scheduled-task-format";

interface ScheduledTaskFormProps {
  open: boolean;
  task: ScheduledTask | null;
  projects: Project[];
  agents: AgentEntry[];
  onOpenChange(open: boolean): void;
}

/** Creates or edits the bounded local Scheduled Task definition shape. */
export function ScheduledTaskForm({
  open,
  task,
  projects,
  agents,
  onOpenChange,
}: ScheduledTaskFormProps) {
  const { t } = useTranslation();
  const createTask = useCreateScheduledTask();
  const updateTask = useUpdateScheduledTask();
  const initialSchedule = task?.schedule ?? {
    kind: "every_hours" as const,
    interval: 24,
  };
  const [name, setName] = useState(task?.name ?? "");
  const [prompt, setPrompt] = useState(task?.prompt ?? "");
  const [projectId, setProjectId] = useState(
    task?.projectId ?? projects[0]?.id ?? "",
  );
  const [agentRef, setAgentRef] = useState(
    task?.agentRef ?? agents[0]?.agentRef ?? "",
  );
  const [model, setModel] = useState(task?.model ?? "");
  const [scheduleKind, setScheduleKind] = useState(initialSchedule.kind);
  const [interval, setInterval] = useState(
    initialSchedule.kind === "every_hours" ? initialSchedule.interval : 24,
  );
  const [time, setTime] = useState(scheduleTime(initialSchedule));
  const [weekdays, setWeekdays] = useState<ScheduledTaskWeekday[]>(
    initialSchedule.kind === "weekly" ? initialSchedule.weekdays : ["monday"],
  );
  const [enabled, setEnabled] = useState(task?.enabled ?? false);
  // A new definition never ships with pre-checked unattended consent: the user must opt in.
  const [approved, setApproved] = useState(
    task !== null && task.approval !== null,
  );
  const [validationError, setValidationError] = useState(false);
  const [submissionError, setSubmissionError] = useState<string | null>(null);
  const pending = createTask.isPending || updateTask.isPending;
  const agentOptions = useMemo(
    () =>
      task !== null && !agents.some((agent) => agent.agentRef === task.agentRef)
        ? [
            ...agents,
            { agentRef: task.agentRef, label: task.agentRef, logo: null },
          ]
        : agents,
    [agents, task],
  );

  const toggleWeekday = (weekday: ScheduledTaskWeekday) => {
    setWeekdays((current) =>
      current.includes(weekday)
        ? current.filter((candidate) => candidate !== weekday)
        : [...current, weekday],
    );
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const [hourText, minuteText] = time.split(":");
    const hour = Number(hourText);
    const minute = Number(minuteText);
    const invalid =
      name.trim() === "" ||
      prompt.trim() === "" ||
      projectId === "" ||
      agentRef === "" ||
      (enabled && !approved) ||
      (scheduleKind === "every_hours" && interval < 1) ||
      (scheduleKind === "weekly" && weekdays.length === 0) ||
      (scheduleKind !== "every_hours" &&
        (!Number.isInteger(hour) || !Number.isInteger(minute)));
    if (invalid) {
      setValidationError(true);
      return;
    }
    const schedule: ScheduledTaskSchedule =
      scheduleKind === "every_hours"
        ? { kind: "every_hours", interval }
        : scheduleKind === "daily"
          ? { kind: "daily", hour, minute }
          : { kind: "weekly", weekdays, hour, minute };
    setValidationError(false);
    setSubmissionError(null);
    try {
      const common = {
        name: name.trim(),
        enabled,
        schedule,
        projectId,
        agentRef,
        model: model.trim() || null,
        prompt: prompt.trim(),
      };
      if (task === null) {
        await createTask.mutateAsync({
          ...common,
          executionLocation: "local",
          approvalPolicy: approved ? ("full_auto_in_worktree" as const) : null,
        });
      } else {
        await updateTask.mutateAsync({
          ...common,
          taskId: task.id,
          expectedUpdatedAt: task.updatedAt,
          // Only an active re-check is a new consent fact. Echoing the stored
          // approval must neither refresh its consentedAt nor resurrect a
          // revoked one, and unchecking revokes it explicitly.
          approvalChange: approved
            ? task.approval !== null && projectId === task.projectId
              ? ({ kind: "keep" } as const)
              : ({ kind: "grant", policy: "full_auto_in_worktree" } as const)
            : ({ kind: "revoke" } as const),
        });
      }
      onOpenChange(false);
    } catch (error) {
      setSubmissionError(localizeContractError(error, t));
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-h-[86vh] overflow-y-auto sm:max-w-xl">
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>
              {task === null ? t("scheduledTask.new") : t("scheduledTask.edit")}
            </DialogTitle>
            <DialogDescription>
              {t("scheduledTask.description")}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4">
            <div className="grid gap-1.5">
              <Label htmlFor="scheduled-task-name">
                {t("scheduledTask.name")}
              </Label>
              <Input
                id="scheduled-task-name"
                value={name}
                disabled={pending}
                onChange={(event) => setName(event.target.value)}
                autoFocus
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="scheduled-task-prompt">
                {t("scheduledTask.prompt")}
              </Label>
              <Textarea
                id="scheduled-task-prompt"
                value={prompt}
                disabled={pending}
                rows={5}
                onChange={(event) => setPrompt(event.target.value)}
              />
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label>{t("scheduledTask.project")}</Label>
                <Select
                  value={projectId}
                  disabled={pending}
                  onValueChange={(value) => {
                    const nextProjectId = value ?? "";
                    setProjectId(nextProjectId);
                    // Consent is scoped to the selected project. Switching a
                    // project therefore requires an intentional new opt-in.
                    if (task !== null && nextProjectId !== task.projectId) {
                      setApproved(false);
                    }
                  }}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {projects.map((project) => (
                      <SelectItem key={project.id} value={project.id}>
                        {project.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label>{t("scheduledTask.agent")}</Label>
                <Select
                  value={agentRef}
                  disabled={pending}
                  onValueChange={(value) => setAgentRef(value ?? "")}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {agentOptions.map((agent) => (
                      <SelectItem key={agent.agentRef} value={agent.agentRef}>
                        {agent.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="scheduled-task-model">
                {t("scheduledTask.model")}
              </Label>
              <Input
                id="scheduled-task-model"
                value={model}
                disabled={pending}
                onChange={(event) => setModel(event.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label>{t("scheduledTask.schedule")}</Label>
              <Select
                value={scheduleKind}
                disabled={pending}
                onValueChange={(value) =>
                  value &&
                  setScheduleKind(value as ScheduledTaskSchedule["kind"])
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="every_hours">
                    {t("scheduledTask.everyHours")}
                  </SelectItem>
                  <SelectItem value="daily">
                    {t("scheduledTask.daily")}
                  </SelectItem>
                  <SelectItem value="weekly">
                    {t("scheduledTask.weekly")}
                  </SelectItem>
                </SelectContent>
              </Select>
              {scheduleKind === "every_hours" ? (
                <div className="grid gap-1.5">
                  <Label htmlFor="scheduled-task-interval">
                    {t("scheduledTask.interval")}
                  </Label>
                  <Input
                    id="scheduled-task-interval"
                    type="number"
                    min={1}
                    value={interval}
                    disabled={pending}
                    onChange={(event) =>
                      setInterval(Number(event.target.value))
                    }
                  />
                </div>
              ) : (
                <div className="grid gap-1.5">
                  <Label htmlFor="scheduled-task-time">
                    {t("scheduledTask.time")}
                  </Label>
                  <Input
                    id="scheduled-task-time"
                    type="time"
                    value={time}
                    disabled={pending}
                    onChange={(event) => setTime(event.target.value)}
                  />
                </div>
              )}
              {scheduleKind === "weekly" && (
                <div className="grid gap-1.5">
                  <Label>{t("scheduledTask.weekdays")}</Label>
                  <div className="flex flex-wrap gap-1.5">
                    {WEEKDAYS.map((day) => {
                      const selected = weekdays.includes(day.value);
                      return (
                        <Button
                          key={day.value}
                          type="button"
                          size="sm"
                          variant={selected ? "secondary" : "outline"}
                          disabled={pending}
                          aria-pressed={selected}
                          onClick={() => toggleWeekday(day.value)}
                        >
                          {t(day.key)}
                        </Button>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
            <div className="grid gap-2 rounded-lg border p-3">
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium">
                  {t("scheduledTask.local")}
                </span>
                <span className="text-xs text-muted-foreground">
                  {t("scheduledTask.cloud")}
                </span>
              </div>
              <div className="flex items-center justify-between gap-3">
                <Label
                  htmlFor="scheduled-task-approval"
                  className="leading-snug"
                >
                  {t("scheduledTask.approval")}
                </Label>
                <Switch
                  id="scheduled-task-approval"
                  checked={approved}
                  disabled={pending}
                  onCheckedChange={setApproved}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {t("scheduledTask.approvalHelp")}
              </p>
              <div className="flex items-center justify-between gap-3">
                <Label htmlFor="scheduled-task-enabled">
                  {t("scheduledTask.enabled")}
                </Label>
                <Switch
                  id="scheduled-task-enabled"
                  checked={enabled}
                  disabled={pending}
                  onCheckedChange={setEnabled}
                />
              </div>
            </div>
            {validationError && (
              <p className="text-sm text-destructive">
                {t("scheduledTask.validation")}
              </p>
            )}
            {submissionError && (
              <p data-selectable className="text-sm text-destructive">
                {submissionError}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              type="submit"
              disabled={
                pending ||
                projects.length === 0 ||
                (task === null && agents.length === 0)
              }
            >
              {pending && <Spinner className="size-4" />}
              {task === null
                ? t("scheduledTask.create")
                : t("scheduledTask.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
