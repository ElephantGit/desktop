CREATE TABLE runtime_binding (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    data TEXT NOT NULL CHECK(json_valid(data)),
    closed INTEGER NOT NULL CHECK(closed IN (0,1))
);
CREATE TABLE runtime_enforcement (singleton INTEGER PRIMARY KEY CHECK(singleton=1));
CREATE TABLE execution_control (
    execution TEXT PRIMARY KEY REFERENCES execution_identities(execution),
    permit TEXT NOT NULL CHECK(json_valid(permit)),
    started INTEGER NOT NULL DEFAULT 0 CHECK(started IN (0,1))
);
