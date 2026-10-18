# Streams and Cancellation

English | [中文](streams.zh.md)

Session load and prompt, history replay, workspace watching and `watchAppEvents` are streams. This
document describes how Desktop runs them over Tauri and what a cancellation actually confirms.

## Stream lifecycle

Streams use the shared `stream_contract` command, which forwards ordered `data`, `error` and `end`
frames over a Tauri Channel. A private call id lets an `AbortSignal` cancel exactly that stream,
while one separate request id correlates the whole stream for logging.

The stream registry (`apps/desktop/src-tauri/src/stream_registry.rs`) claims the call id before
domain startup and keeps it until the owning registration is dropped; cancellation signals the
owner instead of freeing the id. Startup already in progress is allowed to settle, because
abandoning arbitrary domain work could orphan actor side effects; a resource created after
cancellation is dropped at once. The frontend signals cancellation even while startup is pending and
repeats cleanup after it settles. Application exit cancels every starting or running registration
and refuses new ones. Domain modules supply event sources; they never duplicate forwarding or
request completion.

## Three stages of cancellation

1. **Requested**: Desktop cancels the registration token, or a backend stream sends an
   operation-specific actor command. Nothing is confirmed yet.
2. **Triggered**: startup settles or forwarding exits and the resource owner begins cleanup. `Drop`
   remains best effort and confirms nothing.
3. **Confirmed**: the operation owner acknowledged stopping, event publishers released their
   resources and any attached workflow restoration returned successfully. Only then does
   `cancel_contract_stream` succeed.

Backend callers use `SessionEventStream::cancel_and_wait(&mut self)`. Cancellation starts once and
repeated calls return the same result; dropping the waiting future does not abort cleanup. Callers
apply their own deadline with `tokio::time::timeout`.

## Waiting for confirmation

`cancel_contract_stream` waits up to 30 seconds. A timeout ends that waiter only, never startup,
cleanup or another waiter; the registration stays owned while cleanup runs, concurrent waiters
observe the same registration, and a later call can still retrieve the eventual receipt. Completed
receipts are cached within the registry's 256-entry pruning threshold, and active registrations are
never pruned. An unknown or evicted id is an error, not inferred success; use a fresh call id for a
new operation.

During startup, cancellation before the source is polled creates no resource. Once creation began,
Desktop waits for it to settle and then for the resulting stream's cleanup. Startup and cleanup
errors are preserved for both the startup command and cancellation waiters. A running stream defers
its terminal frame and lifecycle completion until cleanup settles.

Native filesystem watchers, in both startup and forwarding, await
`ora_fs::WorkspaceWatcher::close()`, which waits for the native event handler's retirement receipt
after native handles and callback captures are released. Watcher `Drop` and host-worker exit confirm
nothing. The receipt relies on the destructor ordering of the exactly pinned `notify` 8.2.0; the
platform audit lives in `crates/fs/src/watch/retirement.rs` and must be repeated when `notify` is
upgraded.

## Who confirms what

Actor confirmation belongs to one operation generation. A prompt needs a terminal provider
response; an expired cancellation grace period or a provider failure stays unconfirmed even after
local isolation, and a later successful turn cannot validate an earlier failed operation. The actor
keeps at most 256 operation receipts and fails conservatively when evidence is missing.

- Cancelling a load detaches and joins its relay without cancelling the prompt it follows.
- History replay and application-event workers confirm their task ended.
- Workflow cleanup awaits `end_human_turn`, including its persistence result.
- Shared actors, provider connections, other streams and user files stay with their owners.

## Errors

The original business failure takes precedence in the terminal frame and request lifecycle: for
example `session_history_degraded` stays intact even if cleanup also returns
`agent_runtime_unavailable`, and the cancellation receipt reports the cleanup error separately.
Cleanup errors become the terminal error only when there was no business failure.

Cleanup failure, a missing owner receipt, a panicked cleanup worker and a waiting timeout all
return errors with the existing contract shape: confirmation failures use `internal_error`, an
unavailable actor uses `agent_runtime_unavailable`, and restoration errors keep their own code.
Diagnostics name failed restoration, missing or expired evidence, or `stream cleanup wait timed
out; cleanup remains active`. An error never certifies that cleanup completed, and an abort signal
or an abandoned iterator is not a cleanup receipt.
