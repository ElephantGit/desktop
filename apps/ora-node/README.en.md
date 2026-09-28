# ora-node

[中文](README.md)

The Rust Node requires mutual TLS, a fixed platform scope, and root management separated from workload UID 1000 for cloud network operation. Protected credentials and local journals stay outside code. Durable acceptance and first directory/Git mutation check eligibility separately; recovery closes old incarnations. Accepted work settles within its original scope. Plaintext production networking and global personal Git fallback are refused. The loopback transport example is test-only, never shipped, and requires injected test bindings.

## Verification boundary

Module tests cover their real protocol, recovery, Git or TLS boundary. Cloud doubles and the loopback fixture do not prove production multiplayer authorization. Use cluster Compose for acceptance. third_party/cloud pins contract f0b5d9c; generated output is never edited by hand. See [runtime control](../../docs/runtime-control.md) for dependencies and missing evidence.
