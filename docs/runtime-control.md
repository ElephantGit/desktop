# Cloud runtime control delivery

[中文](runtime-control.zh.md)

The authoritative rules live in the [Cloud ADR](../specs/decisions/cloud/controller-integration/20260927-fenced-runtime-control-delivery.md). Controller and Node do not duplicate tenant role policy. Cloud proto is pinned at f0b5d9c. Controller only dials out and has no cloud business SQLite.

Binding persistence precedes acknowledgement. User epoch, Controller lease, runtime generation, Node incarnation and stable execution ID remain distinct. Acceptance and first real mutation both fence old qualification. Closure is sticky within an epoch. New dispatch requires a fresh Cloud permit; disconnection or expiry permits reconciliation of original responsibility, never inference that unknown work did not execute.

Production cloud composition requires HTTPS gRPC, management certificates, HTTPS Substrate and a directly authenticated Node channel. Images separate root management from workload UID/GID 1000. Management materials remain private under /run/ora-management; code belongs to the workload. Static Git configuration accepts only non-secret CA trust, not personal helpers or SSH fallback.

Old workload-owned management homes have no automatic trust repair; unfinished unbound history blocks new binding. Deployment must stop and reconcile old processes before controlled migration. Current Node certificates last 24 hours; seamless refresh is absent and requires managed restart/rotation. The deferred complete local-host trust system remains deferred.

The loopback transport example exists only for real Git, transport and crash regressions, requires injected test scope and mutual TLS on loopback, and is not shipped. It is not production OS-isolation evidence. Real Node/Docker acceptance belongs to cluster. Cloud file/terminal/Agent product execution, real plugin maintenance and project credential provisioning remain closed. Complete lost-acceptance, restart and upgrade/rollback combinations still require evidence; ADRs remain approved.
