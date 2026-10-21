# ora-controller

[中文](README.md)

Cloud mode only dials authenticated gRPC and has no business SQLite. Every new clone/effect dispatch obtains a fresh PostgreSQL permit. Closed qualification permits historical reconciliation, not new work. Binding acknowledgements, ordinary progress and independent force stops retain stable result identities.

Substrate effects recheck the permit's Controller epoch, expiry and current local lease after journal reads and immediately before PUT. Local dispatch requires more than 2 seconds of permit lifetime remaining, matching Sandbox Server. For lifecycle effects, permit expiry within the same epoch enters RetryWait; the next attempt fetches a fresh permit and continues the original effect. An epoch mismatch ends the round without writing defer state. Force stops deliver every target supplied by Cloud; one target's failure does not block independent targets, while Cloud unavailability or lease loss stops dispatch. Before a new terminate PUT, the worker attempts an existing Node control close with the bounded timeout; it does not create an EndSession without Cloud authority. Only Substrate's `terminated` and `lateEnsureFenced` dual evidence confirms termination. A `sandbox_terminated` tombstone rejection keeps ensure Blocked without retransmission or a replacement generation.

## Verification boundary

Module tests cover their real protocol, recovery, Git or TLS boundary. Cloud doubles and the loopback fixture do not prove production multiplayer authorization. Use cluster Compose for acceptance. third_party/cloud pins contract e48cc41; generated output is never edited by hand. See [runtime control](../../docs/controller/runtime-control.md) for dependencies and missing evidence.

See [Agent session relay](../../docs/controller/agent-relay.md) for session dispatch, Thread takeover and command delivery.
