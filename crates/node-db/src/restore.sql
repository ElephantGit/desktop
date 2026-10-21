-- Recovery evidence belongs to the original session, never to a replacement execution.
CREATE TABLE session_restores (
    execution TEXT PRIMARY KEY REFERENCES node_executions(execution),
    phase TEXT NOT NULL CHECK(phase IN ('pending','git_admitted','restored','agent_admitted','legacy_unknown')),
    first_stop_at_ms INTEGER,
    stop_bound_at_ms INTEGER,
    deferred_end TEXT CHECK(deferred_end IS NULL OR json_valid(deferred_end))
);
-- Older in-flight restores had no durable process association or agent-start fact.
-- Preserve that uncertainty instead of manufacturing interruption or safe delivery.
-- Completed restores keep their old terminal as history: no live process or checkout
-- responsibility remains, and `legacy_unknown` has no exit, so fencing them here would
-- permanently refuse later runtime bindings and checkout reuse on the same Node.
-- An Accepted session never started an actor, so no restore process can exist for it; it keeps
-- the pending phase its own admission writes for a prior Revision.
INSERT INTO session_restores(execution,phase)
SELECT execution,'pending' FROM node_executions
WHERE json_extract(input,'$.payload.spec.prior_revision') IS NOT NULL
  AND state='accepted';
-- v9 ran restore synchronously before the Agent started, and only thread events advance
-- `last_sequence`, so a recorded sequence proves the restore had completed. `execution_events`
-- is the outbound queue, not history: acknowledging an event deletes its row, so the evidence
-- has to be a fact the Controller cannot remove.
INSERT INTO session_restores(execution,phase)
SELECT execution,'legacy_unknown' FROM node_executions
WHERE json_extract(input,'$.payload.spec.prior_revision') IS NOT NULL
  AND state='running'
  AND last_sequence=0;
CREATE TABLE restore_process_attempts (
    run TEXT PRIMARY KEY,
    execution TEXT NOT NULL REFERENCES session_restores(execution),
    data BLOB NOT NULL,
    guarantee TEXT NOT NULL CHECK(guarantee IN ('best_effort','strong')),
    cleaned INTEGER NOT NULL DEFAULT 0 CHECK(cleaned IN (0,1)),
    first_stop_at_ms INTEGER,
    stop_bound_at_ms INTEGER,
    stop_unconfirmed INTEGER NOT NULL DEFAULT 0 CHECK(stop_unconfirmed IN (0,1)),
    CHECK((first_stop_at_ms IS NULL) = (stop_bound_at_ms IS NULL)),
    CHECK(stop_bound_at_ms IS NULL OR stop_bound_at_ms>first_stop_at_ms)
);
