use std::str::FromStr;

use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use cron::Schedule;
use ora_domain::{
    ScheduledTask, ScheduledTaskExecutionLocation, ScheduledTaskSchedule, ScheduledTaskWeekday,
};

/// Returns the latest two planned times at or before startup. The offline-recovery handler needs
/// both values: if the newest point is still in its live lateness window, the immediately prior
/// point may nevertheless need an `app_not_running` fact.
pub(super) fn recent_scheduled_occurrences(
    task: &ScheduledTask,
    observed_at: i64,
    timezone: chrono_tz::Tz,
) -> Vec<i64> {
    match &task.schedule {
        ScheduledTaskSchedule::EveryHours { interval } => {
            let Some(anchor_at) = task.schedule_anchor_at else {
                return Vec::new();
            };
            let interval_millis = i64::from(*interval).saturating_mul(60 * 60 * 1_000);
            if interval_millis <= 0 {
                return Vec::new();
            }
            // The enable-time anchor defines alignment; it is not itself an occurrence. This
            // mirrors `next_hourly_occurrence_after`, whose first result is anchor + interval.
            let first = anchor_at.saturating_add(interval_millis);
            if observed_at < first {
                return Vec::new();
            }
            let latest = first.saturating_add(
                observed_at
                    .saturating_sub(first)
                    .div_euclid(interval_millis)
                    .saturating_mul(interval_millis),
            );
            let previous = latest.saturating_sub(interval_millis);
            if previous >= anchor_at {
                vec![previous, latest]
            } else {
                vec![latest]
            }
        }
        ScheduledTaskSchedule::Daily { .. } | ScheduledTaskSchedule::Weekly { .. } => {
            let Some(expression) = calendar_cron_expression(&task.schedule) else {
                return Vec::new();
            };
            let Ok(schedule) = Schedule::from_str(&expression) else {
                return Vec::new();
            };
            let start_at = task.audit_fields.updated_at.min(observed_at);
            let Some(start) = Utc
                .timestamp_millis_opt(start_at)
                .single()
                .map(|instant| instant.with_timezone(&timezone))
            else {
                return Vec::new();
            };
            let Some(observed) = Utc
                .timestamp_millis_opt(observed_at)
                .single()
                .map(|instant| instant.with_timezone(&timezone))
            else {
                return Vec::new();
            };
            let mut recent = Vec::with_capacity(2);
            for occurrence in schedule.after(&start) {
                if occurrence > observed {
                    break;
                }
                let millis = occurrence.timestamp_millis();
                if recent.len() == 2 {
                    recent.remove(0);
                }
                recent.push(millis);
            }
            recent
        }
    }
}

/// Maps a calendar callback to the most recent planned tick rather than the wall-clock callback
/// time. The bounded lookback covers every daily or weekly schedule while keeping admission
/// identity stable across a late callback and startup recovery.
pub(super) fn latest_calendar_occurrence_at_or_before(
    schedule: &ScheduledTaskSchedule,
    observed_at: i64,
    timezone: chrono_tz::Tz,
) -> Option<i64> {
    let expression = calendar_cron_expression(schedule)?;
    let schedule = Schedule::from_str(&expression).ok()?;
    let observed = Utc
        .timestamp_millis_opt(observed_at)
        .single()?
        .with_timezone(&timezone);
    let lookback = observed - ChronoDuration::days(8);
    schedule
        .after(&lookback)
        .take_while(|occurrence| *occurrence <= observed)
        .last()
        .map(|occurrence| occurrence.timestamp_millis())
}

pub(super) fn is_schedulable(task: &ScheduledTask) -> bool {
    task.enabled
        && task.execution_location == ScheduledTaskExecutionLocation::Local
        && task.local_activation_allowed()
        && task
            .worktree_base_branch
            .as_deref()
            .is_some_and(|branch| !branch.trim().is_empty())
        && match task.schedule {
            ScheduledTaskSchedule::EveryHours { .. } => task.schedule_anchor_at.is_some(),
            ScheduledTaskSchedule::Daily { .. } | ScheduledTaskSchedule::Weekly { .. } => true,
        }
}

pub(super) fn calendar_cron_expression(schedule: &ScheduledTaskSchedule) -> Option<String> {
    match schedule {
        ScheduledTaskSchedule::Daily { hour, minute } => Some(format!("0 {minute} {hour} * * *")),
        ScheduledTaskSchedule::Weekly {
            weekdays,
            hour,
            minute,
        } => Some(format!(
            "0 {minute} {hour} * * {}",
            weekdays
                .iter()
                .copied()
                .map(weekday_cron_name)
                .collect::<Vec<_>>()
                .join(",")
        )),
        ScheduledTaskSchedule::EveryHours { .. } => None,
    }
}

fn weekday_cron_name(day: ScheduledTaskWeekday) -> &'static str {
    match day {
        ScheduledTaskWeekday::Monday => "MON",
        ScheduledTaskWeekday::Tuesday => "TUE",
        ScheduledTaskWeekday::Wednesday => "WED",
        ScheduledTaskWeekday::Thursday => "THU",
        ScheduledTaskWeekday::Friday => "FRI",
        ScheduledTaskWeekday::Saturday => "SAT",
        ScheduledTaskWeekday::Sunday => "SUN",
    }
}
