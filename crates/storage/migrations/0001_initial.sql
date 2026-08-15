PRAGMA foreign_keys = ON;

CREATE TABLE workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE collections (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES collections(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (parent_id IS NULL OR parent_id <> id)
);
CREATE INDEX idx_collections_workspace_parent ON collections(workspace_id, parent_id, position);

CREATE TABLE requests (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    collection_id TEXT REFERENCES collections(id) ON DELETE SET NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    body_kind TEXT NOT NULL CHECK (body_kind IN ('none', 'json', 'text')),
    body_content_type TEXT,
    body_content TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_requests_workspace_collection ON requests(workspace_id, collection_id, updated_at DESC);
CREATE INDEX idx_requests_method_url ON requests(method, url);

CREATE TABLE request_query_params (
    request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (request_id, position)
);

CREATE TABLE request_headers (
    request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (request_id, position)
);

CREATE TABLE environments (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_environments_workspace_name ON environments(workspace_id, name);

CREATE TABLE variables (
    environment_id TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    current_value TEXT,
    initial_value TEXT,
    sensitive INTEGER NOT NULL CHECK (sensitive IN (0, 1)),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    secret_reference TEXT,
    PRIMARY KEY (environment_id, position),
    UNIQUE (environment_id, name),
    CHECK ((sensitive = 0) OR (current_value IS NULL AND secret_reference IS NOT NULL))
);

CREATE TABLE history (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    request_id TEXT REFERENCES requests(id) ON DELETE SET NULL,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    status INTEGER,
    duration_ms INTEGER,
    error TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_history_workspace_created ON history(workspace_id, created_at DESC);
CREATE INDEX idx_history_method_url ON history(method, url);

CREATE TABLE history_headers (
    history_id TEXT NOT NULL REFERENCES history(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (history_id, position)
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
