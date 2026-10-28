-- User-managed policy is durable; redeployments do not reset it or usage.
CREATE TABLE IF NOT EXISTS resource_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    revision TEXT NOT NULL,
    enforced INTEGER NOT NULL CHECK (enforced IN (0, 1)),
    profile TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS resource_usage_rule ON resource_usage(rule, reset_at);
