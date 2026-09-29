-- Widen the common identity check without renaming a table referenced by existing records.
CREATE TEMP TABLE saved_session_identities AS SELECT * FROM execution_identities;
DROP TABLE execution_identities;
CREATE TABLE execution_identities (
    operation TEXT PRIMARY KEY,
    execution TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('worktree','clone','plugin','agent_session'))
);
INSERT INTO execution_identities SELECT * FROM saved_session_identities;
DROP TABLE saved_session_identities;
CREATE TABLE node_executions (
    execution TEXT PRIMARY KEY REFERENCES execution_identities(execution),
    kind TEXT NOT NULL CHECK(kind='agent_session'),
    input TEXT NOT NULL CHECK(json_valid(input)),
    state TEXT NOT NULL CHECK(state IN ('accepted','running','completed')),
    result TEXT CHECK(result IS NULL OR json_valid(result)),
    last_sequence INTEGER NOT NULL DEFAULT 0 CHECK(last_sequence>=0),
    CHECK((state='completed') = (result IS NOT NULL))
);
CREATE TABLE execution_events (
    execution TEXT NOT NULL REFERENCES node_executions(execution),
    sequence INTEGER NOT NULL CHECK(sequence>0),
    event TEXT NOT NULL CHECK(json_valid(event)),
    PRIMARY KEY(execution,sequence)
);
CREATE TABLE session_commands (
    acceptance_order INTEGER PRIMARY KEY AUTOINCREMENT,
    execution TEXT NOT NULL REFERENCES node_executions(execution),
    command_id TEXT NOT NULL CHECK(length(command_id)>0),
    input TEXT NOT NULL CHECK(json_valid(input)),
    state TEXT NOT NULL CHECK(state IN ('queued','executed','discarded')),
    UNIQUE(execution,command_id)
);
CREATE INDEX queued_session_commands ON session_commands(execution,state,acceptance_order);
CREATE TRIGGER bind_new_session AFTER INSERT ON node_executions BEGIN
    INSERT INTO execution_controllers SELECT NEW.execution,controller FROM controller_binding;
END;
