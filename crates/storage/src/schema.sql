-- Current host records; future resource catalog tables extend this store directly.
CREATE TABLE store_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    store_id TEXT NOT NULL UNIQUE,
    environment_id TEXT NOT NULL UNIQUE,
    owner_scope_id TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
) STRICT;
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    snapshot_json TEXT CHECK (snapshot_json IS NULL OR (json_valid(snapshot_json)
        AND json_extract(snapshot_json, '$.id') IS session_id))
) STRICT;
CREATE TABLE submission_records (
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    request_key TEXT NOT NULL,
    submission_id TEXT NOT NULL UNIQUE,
    record_json TEXT NOT NULL CHECK (json_valid(record_json)
        AND json_extract(record_json, '$.receipt.sessionId') IS session_id
        AND json_extract(record_json, '$.receipt.idempotencyKey') IS request_key
        AND json_extract(record_json, '$.receipt.id') IS submission_id),
    PRIMARY KEY (session_id, request_key)
) STRICT;
CREATE TABLE session_events (
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    event_json TEXT NOT NULL CHECK (json_valid(event_json)
        AND json_extract(event_json, '$.sessionId') IS session_id
        AND json_extract(event_json, '$.sequence') IS sequence),
    PRIMARY KEY (session_id, sequence)
) STRICT;
CREATE TRIGGER event_sequence_guard BEFORE INSERT ON session_events BEGIN
    SELECT CASE WHEN EXISTS (SELECT 1 FROM session_events
        WHERE session_id = NEW.session_id AND sequence >= NEW.sequence)
        THEN RAISE(ABORT, 'transcript sequences must increase') END;
END;
CREATE TRIGGER event_update_guard BEFORE UPDATE ON session_events BEGIN
    SELECT RAISE(ABORT, 'canonical events are immutable');
END;
CREATE TRIGGER event_delete_guard BEFORE DELETE ON session_events BEGIN
    SELECT RAISE(ABORT, 'canonical events are retained');
END;
CREATE TRIGGER submission_identity_guard BEFORE UPDATE ON submission_records BEGIN
    SELECT CASE WHEN NEW.session_id != OLD.session_id OR NEW.request_key != OLD.request_key
        OR NEW.submission_id != OLD.submission_id
        OR json_extract(NEW.record_json, '$.authored') IS NOT json_extract(OLD.record_json, '$.authored')
        OR json_extract(NEW.record_json, '$.projected') IS NOT json_extract(OLD.record_json, '$.projected')
        THEN RAISE(ABORT, 'submission identity and input are immutable') END;
END;
CREATE TABLE directories (
    directory_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL CHECK (revision = 1),
    environment_id TEXT NOT NULL REFERENCES store_identity(environment_id),
    record_json TEXT NOT NULL CHECK (json_valid(record_json)),
    UNIQUE (directory_id, revision)
) STRICT;
CREATE TABLE project_revisions (
    project_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    directory_id TEXT,
    directory_revision INTEGER,
    record_json TEXT NOT NULL CHECK (json_valid(record_json)),
    PRIMARY KEY (project_id, revision),
    CHECK ((directory_id IS NULL) = (directory_revision IS NULL)),
    FOREIGN KEY (directory_id, directory_revision) REFERENCES directories(directory_id, revision)
) STRICT;
CREATE TABLE project_heads (
    project_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL,
    FOREIGN KEY (project_id, revision) REFERENCES project_revisions(project_id, revision)
) STRICT;
CREATE TABLE default_directory_revisions (
    revision INTEGER PRIMARY KEY CHECK (revision > 0),
    environment_id TEXT NOT NULL REFERENCES store_identity(environment_id),
    directory_id TEXT NOT NULL,
    directory_revision INTEGER NOT NULL,
    record_json TEXT NOT NULL CHECK (json_valid(record_json)),
    FOREIGN KEY (directory_id, directory_revision) REFERENCES directories(directory_id, revision)
) STRICT;
CREATE TABLE default_directory_head (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL REFERENCES default_directory_revisions(revision)
) STRICT;
CREATE TABLE working_contexts (
    context_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL UNIQUE REFERENCES sessions(session_id),
    binding_id TEXT NOT NULL UNIQUE,
    environment_id TEXT NOT NULL REFERENCES store_identity(environment_id),
    directory_id TEXT NOT NULL,
    directory_revision INTEGER NOT NULL,
    project_id TEXT,
    project_revision INTEGER,
    default_revision INTEGER REFERENCES default_directory_revisions(revision),
    record_json TEXT NOT NULL CHECK (json_valid(record_json)),
    CHECK ((project_id IS NULL) = (project_revision IS NULL)),
    FOREIGN KEY (directory_id, directory_revision) REFERENCES directories(directory_id, revision),
    FOREIGN KEY (project_id, project_revision) REFERENCES project_revisions(project_id, revision)
) STRICT;
CREATE TABLE extension_state (
    extension_id TEXT PRIMARY KEY,
    record_json TEXT NOT NULL CHECK (json_valid(record_json))
) STRICT;
CREATE TRIGGER immutable_directory_update BEFORE UPDATE ON directories BEGIN
    SELECT RAISE(ABORT, 'retained directory is immutable'); END;
CREATE TRIGGER immutable_directory_delete BEFORE DELETE ON directories BEGIN
    SELECT RAISE(ABORT, 'retained directory is immutable'); END;
CREATE TRIGGER immutable_project_update BEFORE UPDATE ON project_revisions BEGIN
    SELECT RAISE(ABORT, 'retained project revision is immutable'); END;
CREATE TRIGGER immutable_project_delete BEFORE DELETE ON project_revisions BEGIN
    SELECT RAISE(ABORT, 'retained project revision is immutable'); END;
CREATE TRIGGER immutable_default_update BEFORE UPDATE ON default_directory_revisions BEGIN
    SELECT RAISE(ABORT, 'retained default revision is immutable'); END;
CREATE TRIGGER immutable_default_delete BEFORE DELETE ON default_directory_revisions BEGIN
    SELECT RAISE(ABORT, 'retained default revision is immutable'); END;
CREATE TRIGGER immutable_context_update BEFORE UPDATE ON working_contexts BEGIN
    SELECT RAISE(ABORT, 'retained working context is immutable'); END;
CREATE TRIGGER immutable_context_delete BEFORE DELETE ON working_contexts BEGIN
    SELECT RAISE(ABORT, 'retained working context is immutable'); END;
-- Client-owned node registration; destination runtime catalogs remain authoritative.
CREATE TABLE node_records (
    environment_id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL,
    owner_scope_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    local INTEGER NOT NULL CHECK (local IN (0, 1)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    last_verified_at_ms INTEGER NOT NULL CHECK (last_verified_at_ms >= 0)
) STRICT;
CREATE TABLE node_connections (
    name TEXT PRIMARY KEY,
    environment_id TEXT NOT NULL REFERENCES node_records(environment_id),
    transport_json TEXT NOT NULL CHECK (json_valid(transport_json)),
    credential_ref TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    last_verified_at_ms INTEGER NOT NULL CHECK (last_verified_at_ms >= 0)
) STRICT;
CREATE TABLE selected_node (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    environment_id TEXT NOT NULL REFERENCES node_records(environment_id)
) STRICT;
CREATE TRIGGER node_identity_guard BEFORE UPDATE ON node_records BEGIN
    SELECT CASE WHEN NEW.environment_id != OLD.environment_id OR NEW.store_id != OLD.store_id
        OR NEW.owner_scope_id != OLD.owner_scope_id OR NEW.local < OLD.local
        THEN RAISE(ABORT, 'retained node identity is immutable') END;
END;

CREATE TABLE node_registry_clock (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT;
INSERT INTO node_registry_clock VALUES (1,0);
