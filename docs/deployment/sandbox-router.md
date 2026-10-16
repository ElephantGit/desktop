# Controller sandbox routing

English | [中文](sandbox-router.zh.md)

Cloud Controllers reach every sandbox Node through `substrate.router_url`, selecting the target
with `Ate-Target-Actor: <atespace>/<sandboxInstanceId>`. They no longer derive a direct Node URL
from its ID. Remove the obsolete `direct_node_port` field; unknown deployment fields are rejected.

The Cloud executable requires HTTPS effects and a WSS router. Deployment management TLS files
authenticate the Controller to the router, whose certificate must match the router URL. The
platform relay separately reaches the Node; this is not end-to-end Controller–Node TLS. Node
identity validation in hello, Cloud permits, target binding and runtime fencing remain in place.

Controller and Node may share a network for integration testing. Shared reachability does not
change the configured route or enable fallback. No Node DNS or IP is required by Controller.
The local cluster adapter and the real sandbox adapter expose the same target-routing behavior;
their production authentication policies are separate deployment concerns.

Upgrade Controller configuration and the cluster Sandbox Server together. New cluster Nodes pin
the relay certificate, so drain old sandboxes through normal lifecycle operations before creating
replacements. Preserve Workspace data; do not rewrite journals or weaken certificate checks to
reuse old instances. See cluster's `docs/sandbox-routing.md` for the coordinated upgrade and tests.
