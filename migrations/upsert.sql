-- Parameter order: 1=path, 2=revision, 3=generation, 4=deleted, 5=kind, 6=searchable, 7=title, 8=body, 9=size, 10=sha256, 11=media_type, 12=tags_json, 13=links_json, 14=parser_version, 15=updated_at, 16=indexed_at
-- Ordered parameters are supported by both D1 and native SQLite.
-- Supply a projection derived from a verified, committed HEAD.
INSERT INTO files (
  path, revision, generation, deleted, kind, searchable, title, body,
  size, sha256, media_type, tags_json, links_json, parser_version, updated_at, indexed_at
) VALUES (
  ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
  ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
)
ON CONFLICT(path) DO UPDATE SET
  revision=excluded.revision, generation=excluded.generation,
  deleted=excluded.deleted, kind=excluded.kind, searchable=excluded.searchable,
  title=excluded.title, body=excluded.body, size=excluded.size,
  sha256=excluded.sha256, media_type=excluded.media_type,
  tags_json=excluded.tags_json, links_json=excluded.links_json,
  parser_version=excluded.parser_version, updated_at=excluded.updated_at, indexed_at=excluded.indexed_at
WHERE excluded.generation > files.generation
   OR (excluded.generation = files.generation
       AND excluded.revision = files.revision
       AND excluded.parser_version > files.parser_version);
