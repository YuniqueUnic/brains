-- One bounded literal scan window, not a complete all-vault search.
-- Shared native/D1 arguments:
-- ?1 canonical path prefix, NULL for the whole authorized vault
-- ?2 last visited path, initially empty string
-- ?3 scan_limit, application-clamped to 1..100
-- ?4 nonempty case-sensitive literal query
-- ?5 JSON array of exact tags, empty array means no tag filter
-- The service authorizes the complete requested subtree before querying.
-- A subtree is the exact path or descendants in [prefix+'/', prefix+'0').
-- This works with UTF-8 without incrementing bytes of the prefix itself.
-- Return every row to the API handler, including nonmatches, so it can advance
-- to the last scanned path without dropping a hit. No full body leaves D1.
-- When row count == limit, advertise a next page even if it later proves empty.
-- Exact total completion then needs one extra page; do not guess from hit count.
-- This statement relies on a MATERIALIZED CTE: verify support in real D1 in M0.
WITH candidates AS MATERIALIZED (
    SELECT path, revision, title, body
    FROM files
    WHERE deleted = 0 AND searchable = 1
      AND (?1 IS NULL OR path = ?1 OR (path >= ?1 || '/' AND path < ?1 || '0')) AND path > ?2
      AND NOT EXISTS (SELECT 1 FROM json_each(?5) AS wanted WHERE NOT EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = wanted.value))
    ORDER BY path
    LIMIT ?3
), matched AS (
    SELECT path, revision, title, body, instr(body, ?4) AS body_pos,
           instr(path, ?4) > 0 OR instr(title, ?4) > 0 AS metadata_match
    FROM candidates
)
SELECT path, revision,
       CASE WHEN body_pos > 0 THEN body_pos WHEN metadata_match THEN 1 ELSE 0 END AS match_pos,
       CASE WHEN body_pos > 0
            THEN substr(body, max(1, body_pos - 60), 240)
            WHEN metadata_match THEN substr(CASE WHEN title = '' THEN path ELSE title END, 1, 240)
            ELSE '' END AS snippet
FROM matched
ORDER BY path;
