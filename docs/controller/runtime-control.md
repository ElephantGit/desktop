# Runtime Control

English | [中文](runtime-control.zh.md)

In Cloud mode, who may act on a runtime Workspace is decided by Cloud; the Controller only delivers
what Cloud authorized, and the Node isolates stale authority at its real entry points. The governing
decision is the
[fenced runtime control delivery](https://github.com/ora-space/specs/blob/main/decisions/cloud/controller-integration/20260927-fenced-runtime-control-delivery.md)
ADR; tenant role policy and exclusive control are Cloud's (see Cloud's `docs/runtime/runtime-control`
document) and are not duplicated by the Controller or the Node.

## Identities and generations

Each identity answers a different question and none stands in for another:

| Identity | Authority |
|---|---|
| User and membership | Cloud authentication and authorization |
| Controller lease epoch | The current coordinator's right to write |
| Workspace runtime generation and sandbox id | One running instance and its durable target |
| Runtime control epoch and session | One user or maintenance qualification |
| Node and host incarnation | One running program instance |
| Execution and effect id | Durable responsibility for fixed input |

A stable execution id is not a new authorization and cannot be reused with an old permit to start a
second task. Changing a generation only refuses new actions; it never proves an old process stopped.

## Authenticated management channels

The Cloud–Controller and Controller–Node management links authenticate mutual TLS service identity
and target binding; declared ids in messages are only consistency checks. The Controller refuses a
cloud configuration without `management_tls`, a non-HTTPS Cloud endpoint, non-HTTPS effects or a
non-WSS router, see [Controller](controller.md#cloud-form). The Controller only dials out and keeps no
Cloud business state in SQLite.

In the Node image, root-owned management processes (the Node and the process host) hold the
configuration, journals and private keys under the root-only `/run/ora`, while Git workloads and
agent plugins run as UID/GID 1000 and cannot read management material or write control journals, see
[Container images](../operations/container-images.md). Static Git configuration may carry only
non-secret CA trust, never personal credential helpers or an SSH fallback.

## Dispatch, closure and reporting

1. Cloud checks permission, control, the running instance and conflicting activity in one
   transaction and stores the fixed execution input.
2. Before every new dispatch the Controller obtains a fresh permit (`GetExecutionPermit`, or
   `GetEffectPermit` for Substrate effects). If Cloud is unavailable or the permit is invalid, no new
   action starts.
3. The Node persists acceptance before executing. Retransmitting the same identity with the same input
   only returns the existing state. Bare, unfenced commands are accepted only on private local IPC in
   a Node home that never enabled runtime control; once a home has enabled it, that fallback is closed
   for good.

Withdrawing a qualification closes new admission: the Controller delivers the input-closed binding to
the Node (`ListBindings`, `AcknowledgeBinding`), unaccepted old actions are refused and accepted work
finishes within its original scope. The Node reports the closure and its unfinished execution ids, and
Cloud hands control over only after reconciling them. Closure is sticky within an epoch. A lost
response stays unknown and never releases anything from a cache; a user losing permission does not
discard the end facts of an execution already running.

The Node persists its runtime target, control epoch and version, closure state and responsibilities.
A lower epoch, a different binding at the same epoch, a wrong incarnation or a wrong target is
refused, both at the acceptance queue and at the first real mutation, and a restart never reopens an
old entry from memory.

## Force stop

Emergency termination uses an independent effect permit lasting at most 10 seconds and never beyond
the Controller's lease. The Controller's force-stop worker (`ListForceStops`, `ConfirmForceStop`)
keeps delivering while an ordinary lifecycle step waits on its Node, and never invents execution
completion. The execution side uses a durable runtime fence so a late ensure cannot recreate the
target; Cloud accepts a termination only when it proves both `terminated` and `lateEnsureFenced`.
Unreachability, a heartbeat timeout or a new generation never permits parallel writes to the same
directory.

## Compatibility

Peers negotiate `runtime_control` explicitly, and a controlled execution never silently falls back to
an unbound request. Retired listeners and the old JSON surface stay closed while historical
responsibilities remain queryable. Upgrades keep historical owners, executions and audit references;
rollback never works by deleting new durable state. Node certificates are short-lived and have no
seamless refresh, so rotation needs a managed restart. Old workload-owned management homes are not
repaired automatically: unfinished unbound history blocks new binding, and deployments must stop and
reconcile old processes before a controlled migration.
