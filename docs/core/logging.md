# Logging

English | [中文](logging.zh.md)

Desktop, the Controller and the Node write structured logs through `ora-logging`. Plugin processes are the exception: their own diagnostics go to a per-plugin log the
host persists, see [Plugin logging](../plugins/plugin-logging.md).

## Ownership

- `ora-logging` owns the process-wide subscriber, JSON event formatting, sink selection, file
  rotation, retention and the immutable process timezone.
- Composition roots read configuration, call `ora_logging::init_logging` with an explicit
  `LoggingConfig`, keep the returned `LoggingGuard` for the process lifetime and compose the
  cloneable `LogLevelControl` with preference storage. Dropping the guard early loses buffered
  output. Stdout and file sinks are non-lossy: a full writer channel applies backpressure instead of
  dropping lines.
- Request seams and infrastructure crates emit `tracing` events but never configure sinks or read
  environment variables. Handlers and repositories do not emit generic completion or
  propagation-only failure events.

Initialization is process-wide and the timezone can be set only once, so it happens before any
`ora_logging::clock` access. A file sink that cannot be created fails initialization with a typed
`LoggingInitError` instead of falling back to another sink.

## Desktop configuration

Desktop initializes provisional logging at `info` before opening storage, then restores
`user_config.log_level`; an unset preference also means `info`. A storage read error or a malformed
value aborts startup. `ORA_LOG_LEVEL` is not read, and there is no startup override. The in-app
setting and its persisted preference are the only configuration entry points; the compile-time
logging ceiling is unchanged.

The `ora-runtime-settings` manager serializes live level changes: it reloads the process filter
before persisting, rolls the filter back if persistence fails, and finishes a started commit or
compensation even if the requesting Tauri future is cancelled. See [Settings](../desktop/settings.md).

| Item | Value |
|---|---|
| File | `app_data_dir/logs/ora.log` |
| Rotation | Daily, three files retained (files rotate at UTC boundaries) |
| Stdout | Debug builds only |
| Timezone | The OS IANA timezone read at startup and fixed for the process; UTC with a warning if it cannot be read |

Error toasts that show a request id offer **Download logs**, and the same export is available under
Settings → Developer options while developer mode is on. Desktop copies the current daily log to a
destination chosen in the native save dialog without exposing the application-data path.

## JSON event contract

Every sink writes one JSON object per line with `timestamp`, `level`, `target` and `message`.
`method`, `span`, `trace_id` and `request_id` appear only when attached. Business metadata goes
under `context`, failure details under `error`. Routing is by field prefix: `error.kind` lands in
`error`, `context.operation` in `context`, and any other unrecognized field falls into `context`.
Empty `context` and `error` objects are omitted.

```json
{
  "timestamp": "2026-05-09T20:00:00+08:00",
  "level": "INFO",
  "target": "ora_backend::request_lifecycle",
  "message": "request completed",
  "method": "complete_success",
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "context": { "operation": "create_project", "outcome": "success", "duration_ms": 12 }
}
```

The RFC 3339 timestamp uses the process timezone and includes its offset. When a local date differs
from the file suffix, the event timestamp is authoritative.

## Request completion

Frontend requests receive a canonical UUID v4 at the Tauri or stream entry point; client-provided
ids are never canonical. The same id correlates the request span, the public error payload or
stream error frame, and the completion event.

Each request records exactly one completion event with `operation`, `request_id`, `outcome` and
`duration_ms`. Failures add the public `error.code` plus `error.message`, `error.chain` and
`error.chain_depth` from `ErrorReport::from_error`.

| Classification | Level |
|---|---|
| `Internal` | `ERROR` |
| `Conflict`, `Forbidden`, `HostUnavailable` | `WARN` |
| `InvalidRequest`, `NotFound`, `PayloadTooLarge`, `Unprocessable` | `INFO` |
| Cancellation, successful health and readiness checks | `DEBUG` |

`Forbidden` (the host refused access, such as an unreadable file) sits above `INFO` so it is not
buried in successful traffic, and outside `Conflict` so that bucket keeps meaning contended state.
`HostUnavailable` (the host environment did not carry out a delegated action, such as opening a
missing editor) stays out of `ERROR`. A store or Git failure while routing a task is `Internal`,
never `Conflict`; only a worktree Git confirms absent is a genuine conflict.

Long-lived streams defer completion until the stream ends, reports its error frame, is cancelled or
loses its channel; a dropped channel completes as `cancelled`. A terminal frame claims its
completion before it is sent, so the record describes how the backend stream ended even if the frame
never arrives. `RequestLifecycle` guarantees a closing record structurally: when its last handle is
dropped without explicit completion it emits `outcome = "abandoned"` at `DEBUG`.

`ErrorReport` walks the `Error::source()` chain and keeps the original chain text in debug and
release builds; traversal stops after 1,024 nodes. The sanitizer and redaction patterns exist for
explicit use but `ErrorReport::from_error` does not apply them, so callers must attach only approved
fields: absolute paths, full Git arguments or remotes, SQL values, prompts, environment values,
credentials and unbounded stderr are not valid log fields.

Durable background work logs independently of the request that caused it. Git cleanup, for
example, records `operation = "git_cleanup"` events with its own retry state, so a later cleanup
outcome never changes the primary request's error. Bootstrap, migration and state-transition
events are likewise independent lifecycle facts.

## Emission helpers

Use `ora_trace!`, `ora_debug!`, `ora_info!`, `ora_warn!` and `ora_error!` instead of raw `tracing`
macros; they attach the current function as `method`. `runtime_span`, `span_with_correlation`,
`span_with_trace_id` and `span_with_request_id` create spans whose reserved fields propagate to
nested events (explicit event fields still win). Use `ora_logging::clock::now_local` rather than
`OffsetDateTime::now_local()`.

## Git command logging

`gitlancer` stays framework-neutral: it defines `GitlancerLogger` and a write-once
`logging::register`; with no logger registered, call sites have no side effects. `CliGitRunner::run`
calls `log_command` before spawning and `log_result` with duration, exit code and success after the
process exits (spawn failures report a failure with zero duration).

`ora-logging` supplies the bridge `OraGitlancerLogger`; Desktop installs it with
`register_gitlancer_logger()` right after `init_logging`. It logs the alphabetic Git subcommand as
`operation`, and for `git config` the recognized action, scope and safe `user.*` key, while
dropping the cwd and every other argument (messages, paths, remotes, credentials). Results carry only
`duration_ms` and `exit_code`.

## Deployment boundary

Local JSONL and Logdy (`logdy.config.json`) are local viewing tools. Production log shipping (OTLP,
Loki, Tempo, Grafana) belongs to the deployment, see the cluster repository.

## Testing

`with_trace_logging` and `with_recorded_trace_logging` install a thread-scoped `TRACE` dispatcher.
Use them both for tests that assert on structured output and for ordinary tests that touch the same
callsites, because `tracing` caches callsite interest. Desktop startup logging is tested in isolated
child processes because `init_logging` owns a process-global clock and subscriber.
