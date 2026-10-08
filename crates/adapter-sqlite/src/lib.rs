//! SQLite derived index (design §10). The schema and every bounded query come
//! from `migrations/*.sql` at the repository root — the same SQL the Cloudflare
//! D1 adapter must run — embedded at compile time so the two backends cannot
//! drift. This store is rebuildable: losing it never touches main data.

use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use brains_domain::{DomainError, DomainResult, FileKind};
use brains_ports::{
    FileProjection, IndexStore, LinkRow, ListFilter, ListOrder, ListRow, LiteralRow,
    prefix_upper_bound,
};

const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/d1/0001_index.sql"
));
const UPSERT_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/upsert.sql"
));
const LITERAL_WINDOW_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/literal_window.sql"
));

pub struct SqliteIndex {
    conn: Mutex<Connection>,
}

impl SqliteIndex {
    /// Open (creating if needed) the index database at `path` and apply the
    /// idempotent schema.
    pub fn open(path: &std::path::Path) -> DomainResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DomainError::IndexUnavailable(format!("{}: {e}", parent.display())))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| DomainError::IndexUnavailable(format!("sqlite open: {e}")))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(index_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(index_err)?;
        conn.execute_batch(SCHEMA_SQL).map_err(index_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory index for tests and read-only tooling.
    pub fn open_in_memory() -> DomainResult<Self> {
        let conn = Connection::open_in_memory().map_err(index_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(index_err)?;
        conn.execute_batch(SCHEMA_SQL).map_err(index_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn index_err(e: rusqlite::Error) -> DomainError {
    DomainError::IndexUnavailable(e.to_string())
}

fn invalid_fts(e: rusqlite::Error) -> DomainError {
    DomainError::InvalidArgument(format!("invalid FTS query: {e}"))
}

fn kind_from(raw: &str) -> FileKind {
    FileKind::parse(raw).unwrap_or(FileKind::Binary)
}

impl IndexStore for SqliteIndex {
    async fn upsert(&self, projection: &FileProjection) -> DomainResult<()> {
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        conn.execute(
            UPSERT_SQL,
            params![
                projection.path,
                projection.revision,
                projection.generation,
                projection.deleted as i64,
                projection.kind.as_str(),
                projection.kind.searchable() as i64,
                projection.title,
                projection.body,
                projection.size,
                projection.sha256,
                projection.media_type,
                projection.tags_json,
                projection.links_json,
                projection.parser_version,
                projection.updated_at,
                brains_domain::timestamp::now_rfc3339(),
            ],
        )
        .map_err(index_err)?;
        Ok(())
    }

    async fn scan_literal(
        &self,
        lower: &str,
        upper: Option<&str>,
        after: &str,
        limit: usize,
        query: &str,
        tag: Option<&str>,
    ) -> DomainResult<Vec<LiteralRow>> {
        if query.is_empty() {
            return Err(DomainError::InvalidArgument(
                "literal query must not be empty".into(),
            ));
        }
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        let mut stmt = conn.prepare(LITERAL_WINDOW_SQL).map_err(index_err)?;
        let rows = stmt
            .query_map(
                params![lower, upper, after, limit as i64, query, tag],
                |row| {
                    Ok(LiteralRow {
                        path: row.get(0)?,
                        revision: row.get(1)?,
                        match_pos: row.get(2)?,
                        snippet: row.get(3)?,
                    })
                },
            )
            .map_err(index_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(index_err)
    }

    async fn search_fts(
        &self,
        query: &str,
        lower: &str,
        upper: Option<&str>,
        tag: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> DomainResult<Vec<(String, String, String)>> {
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        // Rejecting bad MATCH syntax here is deliberate: an invalid FTS query
        // is a 400, never a fallback to a fake-empty result. Path and tag
        // bounds mirror scan_literal so no filter is silently dropped.
        let mut sql = String::from(
            "SELECT files_fts.path, files.revision,
                    snippet(files_fts, 2, '', '', '…', 16) AS snip
             FROM files_fts JOIN files ON files.rowid = files_fts.rowid
             WHERE files_fts MATCH ?1 AND files.deleted = 0
             AND files.path >= ?2 AND (?3 IS NULL OR files.path < ?3)",
        );
        if tag.is_some() {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = ?4)",
            );
        } else {
            sql.push_str(" AND ?4 IS NULL");
        }
        sql.push_str(" ORDER BY rank LIMIT ?5 OFFSET ?6");
        let mut stmt = conn.prepare_cached(&sql).map_err(invalid_fts)?;
        let rows = stmt
            .query_map(
                params![query, lower, upper, tag, limit as i64, offset as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(invalid_fts)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DomainError::InvalidArgument(format!("invalid FTS query: {e}")))
    }

    async fn list_files(
        &self,
        filter: &ListFilter,
        after_path: Option<&str>,
        after_updated: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<ListRow>> {
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        let lower = filter.prefix.clone().unwrap_or_default();
        let upper = filter.prefix.as_deref().map(prefix_upper_bound);
        let order_desc = filter.order == ListOrder::UpdatedDesc;

        let mut sql = String::from(
            "SELECT path, revision, generation, deleted, kind, title, size, sha256, media_type, updated_at \
             FROM files WHERE (?1 = 1 OR deleted = 0) \
             AND path >= ?2 AND (?3 IS NULL OR path < ?3)",
        );
        if filter.tag.is_some() {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = ?4)",
            );
        } else {
            sql.push_str(" AND ?4 IS NULL");
        }
        if filter.kind.is_some() {
            sql.push_str(" AND kind = ?5");
        } else {
            sql.push_str(" AND ?5 IS NULL");
        }
        if order_desc {
            sql.push_str(" AND (?6 IS NULL OR updated_at < ?6 OR (updated_at = ?6 AND path > ?7))");
        } else {
            sql.push_str(" AND (?6 IS NULL OR path > ?7)");
        }
        if order_desc {
            sql.push_str(" ORDER BY updated_at DESC, path LIMIT ?8");
        } else {
            sql.push_str(" ORDER BY path LIMIT ?8");
        }

        let mut stmt = conn.prepare_cached(&sql).map_err(index_err)?;
        let rows = stmt
            .query_map(
                params![
                    filter.include_deleted as i64,
                    lower,
                    upper,
                    filter.tag,
                    filter.kind.map(|k| k.as_str()),
                    after_updated,
                    after_path,
                    limit as i64,
                ],
                map_list_row,
            )
            .map_err(index_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(index_err)
    }

    async fn links_outbound(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<LinkRow>> {
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT source, target, kind, source_revision FROM links \
                 WHERE source = ?1 AND (?2 IS NULL OR target > ?2) ORDER BY target LIMIT ?3",
            )
            .map_err(index_err)?;
        let rows = stmt
            .query_map(params![path, after, limit as i64], map_link_row)
            .map_err(index_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(index_err)
    }

    async fn links_backlinks(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<LinkRow>> {
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT source, target, kind, source_revision FROM links \
                 WHERE target = ?1 AND (?2 IS NULL OR source > ?2) ORDER BY source LIMIT ?3",
            )
            .map_err(index_err)?;
        let rows = stmt
            .query_map(params![path, after, limit as i64], map_link_row)
            .map_err(index_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(index_err)
    }

    async fn files_count(&self) -> DomainResult<u64> {
        let conn = self.conn.lock().map_err(|_| poisoned())?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .map_err(index_err)?;
        Ok(count.max(0) as u64)
    }
}

fn map_list_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ListRow> {
    Ok(ListRow {
        path: row.get(0)?,
        revision: row.get(1)?,
        generation: row.get(2)?,
        deleted: row.get::<_, i64>(3)? != 0,
        kind: kind_from(&row.get::<_, String>(4)?),
        title: row.get(5)?,
        size: row.get(6)?,
        sha256: row.get(7)?,
        media_type: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_link_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LinkRow> {
    Ok(LinkRow {
        source: row.get(0)?,
        target: row.get(1)?,
        kind: row.get(2)?,
        source_revision: row.get(3)?,
    })
}

fn poisoned() -> DomainError {
    DomainError::IndexUnavailable("index connection poisoned".into())
}

// OptionalExtension keeps rusqlite's Option helpers referenced even if the
// non-upsert path changes; drop if genuinely unused.
#[allow(unused_imports)]
use OptionalExtension as _;

#[cfg(test)]
mod tests;
