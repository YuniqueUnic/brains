-- Source files and revisions remain in R2. These tables are disposable
-- semantic projections and persistent application usage accounting.
CREATE TABLE IF NOT EXISTS resource_usage (
    key TEXT PRIMARY KEY,
    meter TEXT NOT NULL,
    rule TEXT NOT NULL,
    unit TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT 'bounded',
    limit_value REAL NOT NULL CHECK (limit_value >= 0),
    consumed REAL NOT NULL DEFAULT 0 CHECK (consumed >= 0),
    reserved REAL NOT NULL DEFAULT 0 CHECK (reserved >= 0),
    reset_at INTEGER
);
CREATE INDEX IF NOT EXISTS resource_usage_meter ON resource_usage(meter);
CREATE INDEX IF NOT EXISTS resource_usage_held ON resource_usage(meter) WHERE reserved>0;
CREATE INDEX IF NOT EXISTS resource_usage_reset ON resource_usage(reset_at);
CREATE TABLE IF NOT EXISTS resource_attempts (
    id TEXT PRIMARY KEY,
    payload TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('reserved', 'uncertain', 'consumed', 'released')),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE IF NOT EXISTS resource_charges (
    attempt_id TEXT NOT NULL REFERENCES resource_attempts(id),
    meter_key TEXT NOT NULL REFERENCES resource_usage(key),
    amount REAL NOT NULL CHECK (amount >= 0),
    PRIMARY KEY (attempt_id, meter_key)
);
CREATE TRIGGER IF NOT EXISTS resource_reserve
AFTER INSERT ON resource_charges
BEGIN
    UPDATE resource_usage SET reserved = reserved + NEW.amount WHERE key = NEW.meter_key;
END;
CREATE TRIGGER IF NOT EXISTS resource_settle
AFTER UPDATE OF state ON resource_attempts
WHEN OLD.state IN ('reserved', 'uncertain') AND NEW.state IN ('consumed', 'released')
BEGIN
    UPDATE resource_usage
    SET reserved = MAX(0, reserved - COALESCE((
            SELECT SUM(amount) FROM resource_charges WHERE attempt_id = NEW.id AND meter_key = resource_usage.key
        ), 0)),
        consumed = consumed + CASE WHEN NEW.state = 'consumed' THEN COALESCE((
            SELECT SUM(amount) FROM resource_charges WHERE attempt_id = NEW.id AND meter_key = resource_usage.key
        ), 0) ELSE 0 END
    WHERE key IN (SELECT meter_key FROM resource_charges WHERE attempt_id = NEW.id);
END;

CREATE TABLE IF NOT EXISTS semantic_items (
    space TEXT NOT NULL,
    revision TEXT NOT NULL,
    path TEXT NOT NULL,
    generation INTEGER NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    item_id TEXT,
    state TEXT NOT NULL DEFAULT 'pending',
    last_error TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (space, revision)
);
CREATE INDEX IF NOT EXISTS semantic_items_path ON semantic_items(space, path, generation);
CREATE INDEX IF NOT EXISTS semantic_items_state ON semantic_items(space, state, updated_at);
