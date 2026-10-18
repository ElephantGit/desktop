# Agent Relay

English | [中文](agent-relay.zh.md)

In the cloud form the Controller carries Agent session work, session commands, Revision deliveries,
their upload grants and the download grants of resumed sessions between Cloud and the Workspace's
Node. Cloud owns the IssueRun state and every durable receipt; the Controller keeps only
connection-local queues and creates no durable copy of Cloud state. The SQLite form is clone-only.
The decision is the
[Controller relays agent sessions](https://github.com/ora-space/specs/blob/main/decisions/controller/session/0-controller-relays-agent-sessions.md)
ADR; the Node side is in [Node agent sessions](../node/agent-sessions.md) and [Revisions](../node/revisions.md).

## Sessions

`ClaimWork` session items name their Workspace sandbox and Node. The Controller waits for that
sandbox's verified handshake and the `agent_session` capability, then records the exact input in Cloud
while holding the sandbox admission gate; a reconnect wakes queued work. The Node operation id is
read back from the registered record rather than assumed to equal the IssueRun id. A model-bound start
also requires the Node's `model_proxy` capability.

Pending executions are queried. An `Unknown` status permits at most one retransmission of the
original command per connection, using a fresh execution permit and `ControlledStartAgentSession`; a
missing or closed permission can never authorize a start. Unfinished Agent executions count as work
when quiesce decides whether a sandbox is idle.

## Thread events

Each execution has its own ordered takeover worker. Batches hold at most 64 records, wait at most
100 ms from the first record and flush at about 1 MiB of encoded payload to stay within gRPC limits.
Cloud must confirm the whole batch (`TakeOverThreadEvents`) before any exact event ACK is sent to
the Node. The terminal envelope follows every earlier batch; a `Completed` status reply does not settle a
session, because it carries no sequence proving the earlier records arrived.

A definitely unavailable write retries after 250 ms without ACK. A lost-response write uses the RPC
adapter's bounded retries with one submission id. A conflict or exhausted uncertain write closes the
connection without ACK, and the reconnect replays from the Node's durable outbox; merely clearing a
queue would not trigger replay and could leave a full window stuck. Thread workers, command delivery,
reconciliation and polling run outside the transport frame pump, so a slow execution never stops
another or the heartbeats. Connection teardown cancels its workers.

## Commands

`ThreadCommandAvailable` wakes delivery, with periodic polling as a backstop. A command must match the
registered run and execution and the current sandbox and runtime scope; only the first undelivered
command of a run is eligible, and the Node must first report that it knows the execution. A matching
`SessionCommandAccepted`, or `SessionCommandRejected(session_ended)`, permits
`RecordThreadCommandDelivered`; a send alone never does. Retries keep the command id and payload, and
after a restart Cloud supplies the unconfirmed head again while the Node deduplicates.

## Revision delivery

`DeliverRevision` items take the same claim, register and dispatch path. Registration requires the
target Node's verified handshake with `revision_delivery`; otherwise the item stays queued in Cloud.
The registered input is rebuilt with the target Node added (Cloud's spec carries none) and sent only as
`ControlledDeliverRevision` under a fresh permit. Deliveries have their own pending list, so they never
reach clone polling, and an unfinished delivery keeps quiesce from reporting idle.

The terminal `RevisionResult` is taken over with `TakeOverNodeEvent` and acknowledged only after Cloud
commits it; Cloud verifies the declared objects inside that takeover. A `Completed` status reply settles
nothing, because Cloud accepts a delivery result only with the Node's sequenced receipt. The Controller
never writes `RevisionFailed` itself and never reports `VERIFICATION_FAILED`, which only Cloud records.
Shutdown, lease loss and disconnection leave the delivery without a result; a new process resumes it
through status queries and, for `Unknown`, the original command.

### Upload grants

Grants are requested from `GrantRevisionUpload` only with the SHA-256 digests the Node froze, so Cloud
signs `x-amz-checksum-sha256` together with `If-None-Match: *`:

- When the Node sends `UploadGrantNeeded`, its checksum map is forwarded unchanged.
- When a new connection first sees the delivery `Accepted` or `Running`, it requests again with the
  digests that Node last reported to this process.
- Nothing is requested right after dispatch, because the Node has not frozen its objects; a restarted
  Controller, which knows no digests, waits for the Node to ask.

Grants are relayed as `UploadGrant` with headers and expiry verbatim, only for requested keys. An
unavailable Cloud or lost lease keeps the request for a later retry; `CONFLICT` (the delivery has a
result or its run stopped delivering) drops it without failing anything. Grants live only in memory on
their way to the transport and are never persisted, logged or formatted into diagnostics.

## Resumed sessions and download grants

A session input with `prior_revision` resumes the Issue's latest Revision (the
[revision restore contract](https://github.com/ora-space/specs/blob/main/decisions/cloud/controller-integration/20261010-revision-restore-contract.md)).
Its prior Revision maps into the Node session spec with its bundle object (an input without one is
refused) and into the delivery spec without it. Such a session is registered only when the Node's live
handshake also advertised `revision_restore`; otherwise it stays queued in Cloud. An unchanged delivery
result is accepted when its final commit is the base or the prior final commit.

The Node sends `DownloadGrantNeeded` while its restore waits and again on every new connection; the
Controller asks `GrantRevisionDownload` only then, coalesces repeats and keeps nothing across
connections. The grant is relayed as `DownloadGrant { granted }` with headers and expiry verbatim, only
as a `GET` of the session's own bundle key. `NOT_FOUND` and `ABORTED` (the session has a result, its run
stopped, or the input names no bundle) reach the Node as `DownloadGrant { refused }`, which fails the
restore; an unavailable Cloud, a lost reply or a stale lease keeps the request and retries, so an outage
never fails a restore. Download grants are never persisted or logged either.
