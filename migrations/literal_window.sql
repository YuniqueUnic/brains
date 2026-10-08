-- One bounded literal scan window, not a complete all-vault search.
-- D1/SQLite ordered arguments:
-- ?1 inclusive path lower bound (requested prefix intersected with ONE allowed scope)
-- ?2 exclusive path upper bound, NULL only for full authorized vault
-- ?3 last visited path, initially empty string
-- ?4 scan_limit, application-clamped to 1..100
-- ?5 nonempty case-sensitive literal query
-- ?6 exact tag, or NULL
--
-- The service resolves/intersects authorization ranges BEFORE binding this query.
-- Multiple disjoint permitted ranges require a cursor carrying the range index.
-- Return every row to the API handler, including nonmatches, so it can advance
-- to the last scanned path without dropping a hit. No full body leaves D1.
-- When row count == limit, advertise a next page even if it later proves empty.
-- Exact total completion then needs one extra page; do not guess from hit count.
-- This statement relies on a MATERIALIZED CTE: verify support in real D1 in M0.
WITH candidates AS MATERIALIZED (
    SELECT path, revision, title, body
    FROM files
    WHERE deleted = 0 AND searchable = 1
      AND path >= ?1 AND (?2 IS NULL OR path < ?2) AND path > ?3
      AND (?6 IS NULL OR EXISTS (
          SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = ?6
      ))
    ORDER BY path
    LIMIT ?4
), matched AS (
    SELECT path, revision, body, instr(body, ?5) AS match_pos
    FROM candidates
)
SELECT path, revision, match_pos,
       CASE WHEN match_pos > 0
            THEN substr(body, max(1, match_pos - 60), 240)
            ELSE '' END AS snippet
FROM matched
ORDER BY path;
