-- v2: lifecycle/trust/provenance projection column. Applied once to every
-- database (old and fresh); the column starts empty and `bsx reindex
-- --all-pages` fills it. SQLite has no IF NOT EXISTS on ALTER TABLE ADD
-- COLUMN; wrangler applies each migration file exactly once per database.
ALTER TABLE files ADD COLUMN meta_json TEXT NOT NULL DEFAULT '{}';
