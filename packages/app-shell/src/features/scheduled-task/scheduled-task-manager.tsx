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
  cn,
} from "@ora/ui";
import {
  IconCalendarClock,
  IconEdit,
  IconPlus,
  IconShieldX,
  IconTrash,
} from "@tabler/icons-react";
import { localizeContractError } from "../../i18n/contract-error";
import { useAvailableAgents } from "../../state/hooks/use-available-agents";
import { useProjects } from "../../state/hooks/use-projects";
import {
  useCreateScheduledTask,
  useDeleteScheduledTask,
  useRevokeScheduledTaskApproval,
  useScheduledTasks,
  useUpdateScheduledTask,
} from "../../state/hooks/use-scheduled-tasks";
import type { AgentEntry } from "../../state/hooks/use-agent-catalog";

const WEEKDAYS: ReadonlyArray<{
  value: ScheduledTaskWeekday;
  key: string;
}> = [
  { value: "monday", key: "scheduledTask.weekday.mon" },
  { value: "tuesday", key: "scheduledTask.weekday.tue" },
  { value: "wednesday", key: "scheduledTask.weekday.wed" },
  { value: "thursday", key: "scheduledTask.weekday.thu" },
  { value: "friday", key: "scheduledTask.weekday.fri" },
  { value: "saturday", key: "scheduledTask.weekday.sat" },
  { value: "sunday", key: "scheduledTask.weekday.sun" },
];

function twoDigits(value: number): string {
  return value.toString().padStart(2, "0");
}

function scheduleTime(schedule: ScheduledTaskSchedule): string {
  if (schedule.kind === "every_hours") return "09:00";
  return `${twoDigits(schedule.hour)}:${twoDigits(schedule.minute)}`;
}

function scheduleLabel(
  schedule: ScheduledTaskSchedule,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  if (schedule.kind === "every_hours") {
    return t("scheduledTask.scheduleEvery", { count: schedule.interval });
  }
  const time = scheduleTime(schedule);
  if (schedule.kind === "daily") {
    return t("scheduledTask.scheduleDaily", { time });
  }
  const labels = new Map(WEEKDAYS.map((day) => [day.value, t(day.key)]));
  return t("scheduledTask.scheduleWeekly", {
    days: schedule.weekdays.map((day) => labels.get(day) ?? day).join(", "),
    time,
  });
}

interface ScheduledTaskFormProps {
  open: boolean;
  task: ScheduledTask | null;
  projects: Project[];
  agents: AgentEntry[];
  onOpenChange(open: boolean): void;
}

/** Creates or edits the bounded local Scheduled Task definition shape. */
function ScheduledTaskForm({
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
  const [approved, setApproved] = useState(task?.approval !== null);
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
        approvalPolicy: approved ? ("full_auto_in_worktree" as const) : null,
      };
      if (task === null) {
        await createTask.mutateAsync({
          ...common,
          executionLocation: "local",
        });
      } else {
        await updateTask.mutateAsync({ ...common, taskId: task.id });
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
                  onValueChange={(value) => setProjectId(value ?? "")}
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

/** First-release Scheduled Task definition list and editor. */
export function ScheduledTaskManager() {
  const { t } = useTranslation();
  const tasksQuery = useScheduledTasks();
  const projectsQuery = useProjects();
  const agents = useAvailableAgents();
  const updateTask = useUpdateScheduledTask();
  const deleteTask = useDeleteScheduledTask();
  const revokeApproval = useRevokeScheduledTaskApproval();
  const [editing, setEditing] = useState<ScheduledTask | null | undefined>();
  const projects = useMemo(
    () => projectsQuery.data ?? [],
    [projectsQuery.data],
  );
  const tasks = tasksQuery.data ?? [];
  const projectNames = useMemo(
    () => new Map(projects.map((project) => [project.id, project.name])),
    [projects],
  );
  const agentNames = useMemo(
    () => new Map(agents.map((agent) => [agent.agentRef, agent.label])),
    [agents],
  );
  const error =
    tasksQuery.error ??
    projectsQuery.error ??
    updateTask.error ??
    deleteTask.error ??
    revokeApproval.error;

  const toggleEnabled = (task: ScheduledTask, enabled: boolean) => {
    if (enabled && task.approval === null) {
      setEditing(task);
      return;
    }
    updateTask.mutate({
      taskId: task.id,
      name: task.name,
      enabled,
      schedule: task.schedule,
      projectId: task.projectId,
      agentRef: task.agentRef,
      model: task.model,
      prompt: task.prompt,
      approvalPolicy: task.approval?.policy ?? null,
    });
  };

  return (
    <section className="flex size-full min-w-0 flex-col bg-background">
      <header className="flex items-center gap-4 border-b px-6 py-5">
        <div className="flex size-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <IconCalendarClock className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <h1 className="text-lg font-semibold">{t("scheduledTask.title")}</h1>
          <p className="text-sm text-muted-foreground">
            {t("scheduledTask.description")}
          </p>
        </div>
        <Button
          onClick={() => setEditing(null)}
          disabled={projects.length === 0 || agents.length === 0}
        >
          <IconPlus />
          {t("scheduledTask.new")}
        </Button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto p-6">
        {projects.length === 0 && (
          <p className="mb-4 rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
            {t("scheduledTask.noProjects")}
          </p>
        )}
        {agents.length === 0 && (
          <p className="mb-4 rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
            {t("scheduledTask.noAgents")}
          </p>
        )}
        {tasksQuery.isPending && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner className="size-4" />
            {t("scheduledTask.loading")}
          </div>
        )}
        {error && (
          <p data-selectable className="text-sm text-destructive">
            {localizeContractError(error, t)}
          </p>
        )}
        {!tasksQuery.isPending && !error && tasks.length === 0 && (
          <div className="rounded-xl border border-dashed p-10 text-center text-sm text-muted-foreground">
            {t("scheduledTask.empty")}
          </div>
        )}
        <div className="grid gap-3 xl:grid-cols-2">
          {tasks.map((task) => (
            <article
              key={task.id}
              className="rounded-xl border bg-card p-4 shadow-sm"
            >
              <div className="flex items-start gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <h2 className="truncate font-medium">{task.name}</h2>
                    <span
                      className={cn(
                        "rounded-full px-2 py-0.5 text-[11px] font-medium",
                        task.enabled
                          ? "bg-emerald-500/10 text-emerald-600"
                          : "bg-muted text-muted-foreground",
                      )}
                    >
                      {task.enabled
                        ? t("scheduledTask.statusEnabled")
                        : t("scheduledTask.statusDisabled")}
                    </span>
                  </div>
                  <p className="mt-1 line-clamp-2 text-sm text-muted-foreground">
                    {task.prompt}
                  </p>
                </div>
                <Switch
                  checked={task.enabled}
                  disabled={updateTask.isPending}
                  onCheckedChange={(enabled) => toggleEnabled(task, enabled)}
                  aria-label={
                    task.enabled
                      ? t("scheduledTask.statusEnabled")
                      : t("scheduledTask.statusDisabled")
                  }
                />
              </div>
              <dl className="mt-4 grid gap-2 text-xs text-muted-foreground sm:grid-cols-2">
                <div>
                  <dt className="sr-only">{t("scheduledTask.schedule")}</dt>
                  <dd>{scheduleLabel(task.schedule, t)}</dd>
                </div>
                <div>
                  <dt className="sr-only">{t("scheduledTask.project")}</dt>
                  <dd className="truncate">
                    {projectNames.get(task.projectId) ?? task.projectId}
                  </dd>
                </div>
                <div>
                  <dt className="sr-only">{t("scheduledTask.agent")}</dt>
                  <dd className="truncate">
                    {agentNames.get(task.agentRef) ?? task.agentRef}
                  </dd>
                </div>
                <div>
                  <dt className="sr-only">{t("scheduledTask.model")}</dt>
                  <dd className="truncate">{task.model ?? "—"}</dd>
                </div>
              </dl>
              <div className="mt-4 flex flex-wrap justify-end gap-2 border-t pt-3">
                {task.approval !== null && (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={revokeApproval.isPending}
                    onClick={() =>
                      window.confirm(
                        t("scheduledTask.revokeConfirm", { name: task.name }),
                      ) && revokeApproval.mutate(task)
                    }
                  >
                    <IconShieldX />
                    {t("scheduledTask.revoke")}
                  </Button>
                )}
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => setEditing(task)}
                >
                  <IconEdit />
                  {t("scheduledTask.edit")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  disabled={deleteTask.isPending}
                  onClick={() =>
                    window.confirm(
                      t("scheduledTask.deleteConfirm", { name: task.name }),
                    ) && deleteTask.mutate(task)
                  }
                >
                  <IconTrash />
                  {t("scheduledTask.delete")}
                </Button>
              </div>
            </article>
          ))}
        </div>
      </div>
      {editing !== undefined && (
        <ScheduledTaskForm
          key={editing?.id ?? "new"}
          open
          task={editing}
          projects={projects}
          agents={agents}
          onOpenChange={(open) => !open && setEditing(undefined)}
        />
      )}
    </section>
  );
}
