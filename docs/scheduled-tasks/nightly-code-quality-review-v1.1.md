# Nightly Code Quality Review (V1.1)

English | [中文](nightly-code-quality-review-v1.1.zh.md)

This page is the reusable prompt baseline (V1.1) for the "nightly code quality review" scheduled task, adapted to the currently merged Desktop scheduled-task capability. It describes how to schedule an overnight inspection of the code delta merged into the target branch that day, run engineering gates, and produce a reviewable report — while honestly marking the capabilities the product does not support yet.

## Usage

1. Create a scheduled task and pick the "code quality check" template.
2. Choose the `Daily` schedule (00:30 suggested); execution location `local` (Cloud is not open yet).
3. Target a Git project; pick an agent that can both run commands and review code.
4. Paste the "Prompt (V1.1 verbatim)" section below into the prompt field.
5. Enable the `full_auto_in_worktree` unattended consent.
6. Check `nextRunAt` after saving. **`local` only triggers while Desktop is running**: if it is closed at the scheduled hour, that occurrence is recorded `missed` and is not backfilled.

## Current product limitations

- **Timezone**: local tasks have no timezone selection; the schedule is evaluated in the system's current timezone. The prompt records the actual timezone in the report.
- **Timeout**: each run has a fixed 60-minute limit (not configurable); exceeding it ends as `failed(timeout)`.
- **Run terminal state**: `run.status` / `failure_reason` are decided by the execution layer; the prompt **cannot** rewrite them; the business conclusion only goes into the final response. Distinguishing "execution succeeded but quality verdict FAIL" from "execution failed" is [#439](https://github.com/ora-space/circus/issues/439) (future capability, not implemented).
- **Artifact persistence**: there is no artifact storage; the worktree is reclaimed when unmodified and is only a temporary copy when modified, so the final response must be self-contained and reviewable from run history. Durable artifacts are [#440](https://github.com/ora-space/circus/issues/440) (future capability, not implemented).
- **Delta-scope approximation**: a commit's commit time is not its merge time; using a first-parent time window to bound "the previous natural day" is an approximation, and edge moments may be missed or delayed. The prompt uses `QA_RANGE_UNRESOLVED` to mark cases where a trustworthy range cannot be determined.
- **Missed runs are not compensated**: due points while local is not running are recorded `missed` and are not bulk replayed.

## V1 → V1.1 differences

| Item | V1 | V1.1 |
|---|---|---|
| Terminal mapping | Required FAIL→failed, BLOCKED/RANGE marked incomplete | Explicitly states run.status is not writable by the prompt; the business conclusion goes only into the final response (gap filed as #439) |
| Timezone | Explicit Asia/Kuala_Lumpur | System timezone (local has no timezone field); the report records the actual timezone |
| Delta scope | Allowed `git log` semantics | Requires anchoring BASE/TARGET on the remote target branch's first-parent history; forbids a rolling window |
| Report persistence | Assumed worktree files persist | Final response is self-contained; artifact-persistence gap filed as #440 |
| Gates | Full build + tests + integration | Increment-first within the 60-minute budget, reserve report time, SKIPPED≠PASS |

## Prompt (V1.1 verbatim)

### Goal

Perform an unattended overnight quality inspection of the code delta merged into the target branch of the target Git repository on the previous natural day, surface potential defects, run engineering gates, and produce a reviewable quality report.

**Review only by default: do not modify, commit, push, or create PRs.**

### Execution prerequisites (product capability boundary)

- Schedule: `Daily`, 00:30 suggested.
- Timezone: the system's current timezone (local has no timezone selection); record the actual timezone in the report.
- Environment: an isolated worktree (clean checkout of the task's base branch).
- Timeout: a fixed 60 minutes, not configurable.
- Terminal state: `run.status` is decided by the execution layer, independent of the business conclusion — a session that ends normally becomes `succeeded`, and hitting the deadline becomes `failed(timeout)`. This prompt's business conclusion can only go into the final response and must not claim to rewrite `run.status`.

### Step 0 — Environment check

1. Confirm the repository, target branch, remote URL, and current SHA; record the execution date, actual system timezone, and agent/model version.
2. Run `git fetch origin`; on failure record `QA_BLOCKED` and STOP.
3. Do not touch the user's original working directory; do not modify the target branch; reuse the repository's existing QA/CI commands.

### Step 1 — Determine the delta scope (critical)

Anchor the baseline on the remote target branch snapshot; do not replace it with a `git log --since "24h"` rolling window.

On the first-parent history of `origin/<base_branch>`, using commit time:

1. Find the last commit before 00:00 of the previous natural day = `BASE_SHA`.
2. Find the last commit before 00:00 of today = `TARGET_SHA`.
3. Verify `BASE_SHA` is an ancestor of `TARGET_SHA`.
4. Extract the real delta with `git diff BASE_SHA..TARGET_SHA`; output file count, added/removed lines, commit count, authors, and major modules.

No delta → `QA_IDLE` STOP; missing baseline / abnormal history → `QA_RANGE_UNRESOLVED` STOP, never guess the scope.

### Step 2 — Delta review

Dimensions: correctness, security, concurrency, resource management, performance, compatibility, observability.

Each finding must include: Severity (Critical/High/Medium/Low), file + line, related commit, the problem and trigger condition, impact, evidence, fix suggestion, confidence. Never mark a problem as confirmed based on speculation alone.

### Step 3 — Engineering gates (prioritize high value within 60 minutes)

Run in order on the `TARGET_SHA` snapshot, favoring incremental / already-cached results and avoiding full re-runs:

1. Build / compile
2. Key unit tests (incremental first)
3. lint / static checks
4. type check (if applicable)
5. `git diff --check BASE_SHA..TARGET_SHA` — check committed whitespace in the resolved range; a bare `git diff --check` on a clean checkout compares only the empty worktree diff and can falsely pass

Record the command, exit code, pass/fail counts. SKIPPED ≠ PASS; distinguish environment/dependency insufficiency and timeout from genuine code failures. Reserve time for the final report.

### Step 4 — Final response (must be self-contained and reviewable from run history)

Do not rely on temporary worktree files persisting — the conclusion must be written in full into the final response:

- Review date + actual system timezone
- Repository, branch, `BASE_SHA` / `TARGET_SHA`
- Change summary
- Findings (ordered by severity)
- Build/test/lint results
- Items not executed and why
- `QA_VERDICT`
- Blockers and follow-up actions

### Step 5 — QA_VERDICT

- `QA_PASS`: complete, required gates pass, no Critical/High.
- `QA_WARN`: no blockers, but Medium/Low risks or non-critical checks missing.
- `QA_FAIL`: confirmed Critical/High, or a required gate fails.
- `QA_BLOCKED`: environment/dependency/timeout prevented completing the review.
- `QA_RANGE_UNRESOLVED`: the comparison range cannot be determined reliably.
- `QA_IDLE`: no delta.

After emitting any verdict, STOP; do not enter fix/release/merge phases.

## Future capabilities (not implemented)

- [ora-space/circus#439](https://github.com/ora-space/circus/issues/439): Scheduled Run Result Annotation / Custom Verdict — make the QA conclusion a machine-readable, aggregatable business verdict rather than text buried in the response.
- [ora-space/circus#440](https://github.com/ora-space/circus/issues/440): Scheduled Run Artifact Persistence — decouple report/test artifacts from the worktree lifecycle, index them by Run ID, and access them from run history.

Both are currently requirements only, not implemented; this prompt has been adapted to depend on neither.
