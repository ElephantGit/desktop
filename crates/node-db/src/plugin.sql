-- Rebuild the identity CHECK without renaming its referenced table. initialize runs before
-- foreign_keys is enabled, under the Node's exclusive database lease.
CREATE TEMP TABLE saved_execution_identities AS SELECT * FROM execution_identities;
DROP TABLE execution_identities;
CREATE TABLE execution_identities (
    operation TEXT PRIMARY KEY,
    execution TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('worktree','clone','plugin'))
);
INSERT INTO execution_identities SELECT * FROM saved_execution_identities;
DROP TABLE saved_execution_identities;
CREATE TABLE plugin_executions (
    execution TEXT PRIMARY KEY REFERENCES execution_identities(execution),
    input TEXT NOT NULL CHECK(json_valid(input)),
    state TEXT NOT NULL CHECK(state IN ('accepted','running','completed')),
    result TEXT CHECK(result IS NULL OR json_valid(result)),
    CHECK((state='completed') = (result IS NOT NULL))
);
CREATE TABLE plugin_outbox (
    execution TEXT PRIMARY KEY REFERENCES plugin_executions(execution),
    event TEXT NOT NULL CHECK(json_valid(event))
);
CREATE TRIGGER bind_new_plugin AFTER INSERT ON plugin_executions BEGIN
    INSERT INTO execution_controllers SELECT NEW.execution,controller FROM controller_binding;
END;
