import { useTranslation } from "react-i18next";
import type {
  ScheduledTaskHistoryEntry,
  ScheduledTaskSchedule,
  ScheduledTaskWeekday,
} from "@ora/contracts";

export const WEEKDAYS: ReadonlyArray<{
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

export function twoDigits(value: number): string {
  return value.toString().padStart(2, "0");
}

export function scheduleTime(schedule: ScheduledTaskSchedule): string {
  if (schedule.kind === "every_hours") return "09:00";
  return `${twoDigits(schedule.hour)}:${twoDigits(schedule.minute)}`;
}

export function scheduleLabel(
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

export function historyStatusLabel(
  entry: ScheduledTaskHistoryEntry,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  if (entry.kind === "missed") {
    return t(`scheduledTask.historyReason.${entry.reason}`);
  }
  if (entry.failureReason !== null) {
    return t(`scheduledTask.failureReason.${entry.failureReason}`);
  }
  return t(`scheduledTask.historyStatus.${entry.status}`);
}

export function formatTimestamp(timestamp: bigint): string {
  return new Date(Number(timestamp)).toLocaleString();
}
