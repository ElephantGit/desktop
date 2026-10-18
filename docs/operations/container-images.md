# Container Images

English | [中文](container-images.zh.md)

`docker/Dockerfile` builds the Node and Controller images from one release build of the workspace, so
building both compiles once. The local stack in `ora-space/cluster` builds and runs both.

```sh
docker build -f docker/Dockerfile --target node -t ora-node:local .
docker build -f docker/Dockerfile --target controller -t ora-controller:local .
```

Both use a Debian bookworm runtime; the build stage uses the compiler from `rust-toolchain.toml`.

## Node image

One container per Workspace sandbox, started by the Sandbox Server (never by Compose).

| Input | Rule |
|---|---|
| `ORA_NODE_CONFIG` | The complete [Node configuration](../node/node.md) as JSON; empty refuses to start |
| `ORA_NODE_CERT`, `ORA_NODE_KEY`, `ORA_NODE_CA`, `ORA_CONTROLLER_CERT` | Base64 PEM for the Node's mutual-TLS control listener, see [Control session](../node/control-session.md) |
| `ORA_MODEL_CLIENT_CERT`, `ORA_MODEL_CLIENT_KEY`, `ORA_MODEL_CA` | Required when the configuration has `agent.model_proxy` |
| `ORA_GIT_CLIENT_CERT`, `ORA_GIT_CLIENT_KEY`, `ORA_GIT_CA` | Required when the configuration has `git_access` |
| `/var/lib/ora` | Mount point of the Workspace volume; every state path in the configuration lives below it |
| `/etc/ora/clone.gitconfig` | Clone Git configuration; the image ships an empty, root-owned file without credentials |

The management processes (the process host and the Node) run as root and own the protected
configuration and journals. Git workloads run as UID/GID 1000 (`node`) through the managed process
boundary, and so do agent plugins and their processes when the configuration names an agent workload
directory; the Node's network control listener requires that separate identity.

The entrypoint runs under `tini` as PID 1, so processes reparented to it (guardians and their
descendants) are reaped. It:

1. sets the volume root to `root:root` with mode `0711` (never its contents), creates the clone root and,
   when configured, the agent workload directory (`0711`), which the Node requires and never creates;
2. writes the configuration to `/run/ora/node.json` and each certificate and key to `/run/ora/*.pem`
   (mode `0600`, in a root-only directory) and unsets the variables;
3. starts the process host with `create` when its directory is missing and `recover` otherwise, so the
   same volume keeps its host journal across containers (a failed recovery never falls back to
   creation), and waits up to 30 seconds until the host socket accepts connections (a leftover socket
   file does not count);
4. starts `ora-node`.

A stop signal goes to the Node first, which closes its managed Scopes through the live host; only after
the Node exits is the host stopped. Guardians are not stopped: their journals stay on the volume and the
next container recovers them. The Sandbox Server's stop timeout must therefore exceed the Node's
`shutdown_grace_ms + cleanup_timeout_ms`. If the Node exits on its own, the host is stopped and the
container exits with the Node's status.

Binaries live under the root-owned `/opt/ora/bin`, as the host requires for the guardian. The image also
ships `/opt/ora/bin/deno`, the runtime every plugin runs on: its version is `.deno-version`, and the
build verifies the archive and executable against `scripts/sidecar-checksums.json` (Linux x64 and arm64),
failing on an architecture without a pin. Agent sessions use it through `agent.deno_path`.

## Controller image

Runs `ora-controller` as the unprivileged `controller` user (UID 10001) with `/var/lib/ora-controller`
as a private home. The deployment mounts the configuration and passes `--config`. In the cloud form the
process serves no listener, so the image exposes no port. See [Controller](../controller/controller.md).
