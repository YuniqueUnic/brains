-- Active credentials supersede deployment bootstrap secrets after the first
-- security mutation. Keep this table when rebuilding the derived file index.
CREATE TABLE IF NOT EXISTS security_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    revision TEXT NOT NULL,
    registry TEXT NOT NULL
);
