# minicloud local runtime

English | [中文](runtime.zh.md)

The Linux HTTP executable embeds the same Controller runtime as `ora-controller`. Start Node and
host/guardian using their existing [deployment configuration](../node/repository-clone.md), then build
`cargo build -p ora-minicloud-server`. Run `ora-minicloud-server /absolute/path/minicloud.json`:

```json
{
  "listen": "127.0.0.1:4317",
  "node_id": "deployment-node",
  "controller": {
    "home_directory": "/home/node/controller",
    "protected_state_directories": ["/home/node/state", "/home/node/process"],
    "controller_id": "deployment-controller",
    "nodes": [
      {
        "node_id": "deployment-node",
        "endpoint": "/home/node/state/control.sock"
      }
    ],
    "session": { "io_timeout_ms": 10000, "query_interval_ms": 1000 },
    "reconnect_ms": 1000,
    "timezone": "Asia/Shanghai"
  }
}
```

The Node owner must match this ControllerId. Stop any standalone Controller using the same state
directory before starting minicloud. All state paths remain explicitly injected and protected from
overlap. Non-loopback listening or an unconfigured target is rejected before opening Controller state.
This is a non-production application, with no authentication or additional security infrastructure.

## HTTP interface

After `deno install`, run `deno task --filter @ora/minicloud-client dev` from the repository root and
open `http://127.0.0.1:5174`. Vite proxies `/api` to `http://127.0.0.1:4317`; set
`MINICLOUD_SERVER_URL` when using a different server port. The page uses the shared shadcn components
with React 19 and polls Controller operations. An unresolved submission is saved in tab session storage
before dispatch; after response loss or reload, “retry original request” reuses its identity and input.
This storage is not operation history. Closing the page aborts HTTP and polling, not the Node execution.

- `POST /api/clones`: `{ "requestId": "stable-client-id", "repository": "https://host/repo.git", "branch": "main" }`.
  Returns HTTP 202 with `requestId`, `operationId`, and `executionId` after durable acceptance.
- `GET /api/clones`: newest accepted operations first, including pending records while Node is offline.
- `GET /api/clones/{executionId}`: one operation; 404 means no accepted operation with that identity.

State is `pending`, `succeeded` (Node path and commit), or `failed` (known reason and retained path).
Pending makes no claim about whether Git is currently running. HTTP 400 means invalid input, 409 an
identity/input conflict and 503 temporary unavailability; HTTP errors are not terminal clone failures.
The browser DTOs are generated from `ora-contracts::minicloud`, separate from the Node wire protocol.

Repeat an uncertain submission with the same request identity and input. Closing a browser or stopping
server does not cancel a Node execution. Restart with the same state directory to recover original facts.
There is no separate minicloud task database, cleanup command or automatic new-execution retry.

Real HTTP tests cover acceptance, conflicts, invalid input, offline listing, missing identities, exclusive
ownership, loopback restriction and normal server restart. Lower-level Controller crash tests remain
separate from minicloud's own end-to-end evidence.

Frontend checks: `deno task --filter @ora/minicloud-client lint`, `test`, and `build`.
`task test:minicloud` additionally runs real HTTP and Vite proxy → independent minicloud server →
Node → HTTPS Git tests, including server SIGKILL/restart and one mutation Run. It requires Linux and
installed frontend dependencies. The Vite case is explicitly opt-in for Rust-only CI runners.
The real chain also holds a SQLite writer lock to verify HTTP 503 without accepted intent, then releases
it and verifies a single Run. DOM tests verify UI behavior and reload identity recovery; a full
browser-engine interaction suite and real HTTP response truncation remain separate acceptance gaps.
