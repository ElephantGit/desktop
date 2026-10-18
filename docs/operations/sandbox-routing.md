# Sandbox Routing

English | [中文](sandbox-routing.zh.md)

A cloud Controller reaches every sandbox Node through `substrate.router_url`, selecting the target with
the header `ate-target-actor: <atespace>/<sandboxInstanceId>`; it never derives a direct Node address
from an id, and needs no Node DNS name or IP. Unknown deployment fields are rejected, so obsolete fields
must be removed from configuration.

- The executable requires HTTPS effects and a WSS router. The Controller's management TLS files
  authenticate it to the router, whose certificate must match the router URL.
- The platform relay separately reaches the Node's mutual-TLS listener, and Nodes pin the relay's
  certificate. This is not end-to-end Controller–Node TLS: Node identity validation in the handshake,
  Cloud permits, target binding and runtime fencing remain in force, see
  [Runtime control](../controller/runtime-control.md).
- Controller and Node may share a network in integration tests; shared reachability changes neither the
  configured route nor enables a fallback.
- The local cluster adapter and the real sandbox adapter expose the same target routing; their
  production authentication policies are deployment concerns.

Upgrade the Controller configuration and the Sandbox Server together. Because Nodes pin the relay
certificate, drain old sandboxes through normal lifecycle operations before creating replacements;
preserve Workspace data and never rewrite journals or weaken certificate checks to reuse old instances.
The coordinated procedure is in the cluster repository's [Sandbox routing](https://github.com/ora-space/cluster/blob/main/docs/sandbox/routing.en.md).
