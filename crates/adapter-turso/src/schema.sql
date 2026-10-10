-- Native Turso derived index. FTS is a separate optional index. Source of truth remains HEAD/revisions/blobs.
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS files (
    path TEXT PRIMARY KEY,
    revision TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0,1)),
    kind TEXT NOT NULL DEFAULT 'text',
    searchable INTEGER NOT NULL DEFAULT 1 CHECK (searchable IN (0,1)),
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0 CHECK (size >= 0),
    sha256 TEXT,
    media_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    tags_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tags_json)),
    links_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(links_json)),
    parser_version INTEGER NOT NULL DEFAULT 1 CHECK (parser_version > 0),
    updated_at TEXT NOT NULL,
    indexed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS files_live_path ON files(deleted, path);
CREATE INDEX IF NOT EXISTS files_kind_path ON files(kind, deleted, path);

CREATE INDEX IF NOT EXISTS files_live_updated ON files(deleted, updated_at DESC, path);

CREATE TABLE IF NOT EXISTS links (
    source TEXT NOT NULL,
    source_revision TEXT NOT NULL,
    target TEXT NOT NULL,
    kind TEXT NOT NULL,
    PRIMARY KEY(source, target, kind)
);
CREATE INDEX IF NOT EXISTS links_target ON links(target, source);

CREATE TABLE IF NOT EXISTS tags (
    path TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY(path, tag)
);
CREATE INDEX IF NOT EXISTS tags_tag ON tags(tag, path);

CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
    INSERT OR IGNORE INTO links(source, source_revision, target, kind)
      SELECT NEW.path, NEW.revision, json_extract(value, '$.target'),
             COALESCE(json_extract(value, '$.kind'), 'link')
      FROM json_each(NEW.links_json)
      WHERE NEW.deleted = 0 AND json_type(value, '$.target') = 'text';
    INSERT OR IGNORE INTO tags(path, tag)
      SELECT NEW.path, value FROM json_each(NEW.tags_json)
      WHERE NEW.deleted = 0 AND type = 'text';
END;

CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
    DELETE FROM links WHERE source = OLD.path;
    DELETE FROM tags WHERE path = OLD.path;
    INSERT OR IGNORE INTO links(source, source_revision, target, kind)
      SELECT NEW.path, NEW.revision, json_extract(value, '$.target'),
             COALESCE(json_extract(value, '$.kind'), 'link')
      FROM json_each(NEW.links_json)
      WHERE NEW.deleted = 0 AND json_type(value, '$.target') = 'text';
    INSERT OR IGNORE INTO tags(path, tag)
      SELECT NEW.path, value FROM json_each(NEW.tags_json)
      WHERE NEW.deleted = 0 AND type = 'text';
END;

CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
    DELETE FROM links WHERE source = OLD.path;
    DELETE FROM tags WHERE path = OLD.path;
END;
