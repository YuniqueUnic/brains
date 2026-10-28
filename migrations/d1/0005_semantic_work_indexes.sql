-- Match the queue's range/order so LIMIT bounds work without sorting the
-- entire backlog. Paused work is resumed only by explicit maintenance.
CREATE INDEX IF NOT EXISTS semantic_items_work
    ON semantic_items(space, updated_at, revision)
    WHERE state IN ('pending', 'queued', 'deleting');
CREATE INDEX IF NOT EXISTS semantic_items_paused
    ON semantic_items(space, updated_at, revision)
    WHERE state IN ('failed', 'blocked', 'delete_failed');
