# Node session ledger

English | [中文](session-ledger.zh.md)

`ora-node-db` persists Agent session input, lifecycle, commands and event delivery in schema v8.
`apps/ora-node/src/session/ledger.rs` implements the existing `SessionLedger` and `CheckoutResolver`
ports for `SessionJournal`. The database does not depend on the Node application.

`NodeDatabase::session_journal` opens a narrow actor connection sharing the original exclusive
Node lease. Clones serialize access through a mutex; independent journals serialize writes through
SQLite immediate transactions. Every write uses FULL synchronous durability. The final journal
keeps the lease alive even when the admission connection closes.

- Admission preserves the complete immutable input and both identity keys. Controlled admission
  records the runtime permit and respects unfinished responsibilities. Start rechecks runtime
  authority and only moves Accepted to Running; it cannot restart a Running session.
- Thread appends require Running. A transaction allocates the next sequence, stores the complete
  event envelope and advances `last_sequence` before returning. Exact ACKs delete only the named
  event. Duplicate ACKs are harmless; future ACKs fail. Deleting every event never resets sequencing.
- Commands retain their full envelope, unique command ID and acceptance order. Identical retries
  do not requeue settled commands; changed input fails. Queue reads include all queued commands.
  Settlement is one-way; repeating the same settlement is harmless, changing it fails.
- Ending an Accepted or Running session commits its result, final event and disposal of every
  queued command together. Later appends fail; later commands report `SessionEnded` without adding
  queue entries. Any transaction failure leaves all three unchanged.
- Checkout lookup returns the exact stored target path only for a completed successful clone.
  Missing, failed, pending and non-clone executions return no checkout. The runtime adapter also
  fails closed on storage errors; it never reconstructs paths from IDs.

`recoverable_sessions` exposes unfinished input for interrupted settlement, not automatic resume.
Production composition in `service/agents.rs` shares this journal and the installer's catalog.
With `agent` deployment configuration, the Node advertises AgentSession and handles controlled
starts, durable commands, a 256-event window and exact-ACK replay. Startup seals unfinished
executions as interrupted before the listener opens; disabling Agent configuration still settles
old sessions without starting a plugin. See [Agent sessions](agent-session.md).

Tests use real SQLite through database APIs and through the runtime traits:
`cargo test -p ora-node-db` and `cargo test -p ora-node --test session_ledger`.
They cover migration, rollback, exact ACKs, restart, concurrent allocation and terminal admission,
command ordering, runtime fencing, checkout evidence, and the shared lease lifetime.
