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
INSERT INTO session_restores(execution,phase)
SELECT execution,'legacy_unknown' FROM node_executions
WHERE json_extract(input,'$.payload.spec.prior_revision') IS NOT NULL;
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
