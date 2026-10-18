# node-protocol

`ThreadEvent` may carry a bounded assistant-text projection and truncation mark beside the original settled record. Old messages without either field remain valid. The projection does not replace the original record, Thread acknowledgement, or terminal result.

[中文](README.md)

Optional Hello/HelloAccepted `thread_previews` negotiates the live path; `ThreadPreview` is never sent without it. Messages are bounded execution/version replacements with no durable event sequence or Ack. Settled `ThreadEvent.history_sequence` links stable positions; older messages remain decodable when the field is absent.

RuntimeControl capability represents user control fencing, independently of a single connection or heartbeat. RuntimeBinding separates user epoch, Controller epoch, runtime generation, Node incarnation, business operation, Node operation and execution. Unsupported cloud peers fail explicitly rather than ignoring new constraints.

## Verification boundary

Module tests cover their real protocol, recovery, Git or TLS boundary. Cloud doubles and the loopback fixture do not prove production multiplayer authorization. Use cluster Compose for acceptance. third_party/cloud pins contract e48cc41; generated output is never edited by hand. See [runtime control](../../docs/controller/runtime-control.md) for dependencies and missing evidence.
