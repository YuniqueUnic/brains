//! Native derived index. Turso owns the schema and Tantivy index maintenance;
//! D1 owns its SQLite schema. Both implement the same committed-HEAD projection.
use brains_domain::path::normalize_prefix;
use futures::{executor::block_on, lock::Mutex};
use turso::{Connection, IntoParams, Row, params};

use brains_domain::{DomainError, DomainResult, FileKind, FullTextSearch};
use brains_ports::{
    FileProjection, IndexStore, LinkRow, ListFilter, ListOrder, ListRow, LiteralRow,
};

const SCHEMA_SQL: &str = include_str!("schema.sql");
const UPSERT_SQL: &str = include_str!("../../../migrations/upsert.sql");
const LITERAL_WINDOW_SQL: &str = include_str!("../../../migrations/literal_window.sql");

/// The filename is also the schema capability identity. Switching builds
/// invalidates the previous derived index instead of opening unknown FTS SQL.
pub const INDEX_FILE: &str = if cfg!(feature = "fts") {
    "index.turso-fts.db"
} else {
    "index.turso-basic.db"
};
pub const OTHER_INDEX_FILE: &str = if cfg!(feature = "fts") {
    "index.turso-basic.db"
} else {
    "index.turso-fts.db"
};

pub struct TursoIndex {
    conn: Mutex<Connection>,
    database: turso::Database,
}
impl TursoIndex {
    pub fn open(path: &std::path::Path) -> DomainResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DomainError::IndexUnavailable(e.to_string()))?;
        }
        let path = path
            .to_str()
            .ok_or_else(|| DomainError::InvalidArgument("index path must be UTF-8".into()))?;
        block_on(Self::connect(path))
    }
    pub fn open_in_memory() -> DomainResult<Self> {
        block_on(Self::connect(":memory:"))
    }
    async fn connect(path: &str) -> DomainResult<Self> {
        let database = turso::Builder::new_local(path)
            .experimental_index_method(cfg!(feature = "fts"))
            .build()
            .await
            .map_err(index_err)?;
        let conn = database.connect().map_err(index_err)?;
        conn.execute_batch(SCHEMA_SQL).await.map_err(index_err)?;
        if cfg!(feature = "fts") {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS files_full_text ON files USING fts (title, body)",
                (),
            )
            .await
            .map_err(index_err)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            database,
        })
    }
    /// Publish only after an explicit successful checkpoint; release every
    /// owner before the caller renames the database file.
    pub fn close(self) -> DomainResult<()> {
        let Self { conn, database } = self;
        let conn = conn.into_inner();
        block_on(async {
            let rows = query(&conn, "PRAGMA wal_checkpoint(TRUNCATE)", (), |row| {
                Ok((row.get::<i64>(0)?, row.get::<i64>(1)?, row.get::<i64>(2)?))
            })
            .await
            .map_err(index_err)?;
            if !matches!(rows.as_slice(), [(0, pages, checkpointed)] if pages == checkpointed) {
                return Err(DomainError::IndexUnavailable(
                    "index checkpoint did not complete".into(),
                ));
            }
            Ok(())
        })?;
        drop(conn);
        drop(database);
        Ok(())
    }
}
fn index_err(error: turso::Error) -> DomainError {
    DomainError::IndexUnavailable(error.to_string())
}
fn search_err(error: turso::Error) -> DomainError {
    // The public SDK erases the parser's error type. Classify only the
    // explicit FTS parser diagnostics; storage/SQL execution failures stay 503.
    if let turso::Error::Error(message) = &error
        && (message.starts_with("Internal error: FTS parse error:")
            || message.starts_with("Internal error: FTS query nests deeper than"))
    {
        return DomainError::InvalidArgument(format!("invalid Tantivy query: {message}"));
    }
    index_err(error)
}
async fn query<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    map: fn(&Row) -> turso::Result<T>,
) -> turso::Result<Vec<T>> {
    let mut rows = conn.query(sql, params).await?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await? {
        result.push(map(&row)?);
    }
    Ok(result)
}
impl IndexStore for TursoIndex {
    fn full_text_search(&self) -> FullTextSearch {
        if cfg!(feature = "fts") {
            FullTextSearch::Tantivy
        } else {
            FullTextSearch::Disabled
        }
    }
    async fn remove_path(&self, path: &str) -> DomainResult<()> {
        self.conn
            .lock()
            .await
            .execute("DELETE FROM files WHERE path = ?1", [path])
            .await
            .map_err(index_err)?;
        Ok(())
    }
    async fn upsert(&self, projection: &FileProjection) -> DomainResult<()> {
        self.conn
            .lock()
            .await
            .execute(
                UPSERT_SQL,
                params![
                    projection.path.as_str(),
                    projection.revision.as_str(),
                    projection.generation,
                    projection.deleted as i64,
                    projection.kind.as_str(),
                    projection.kind.searchable() as i64,
                    projection.title.as_str(),
                    projection.body.as_str(),
                    projection.size,
                    projection.sha256.as_deref(),
                    projection.media_type.as_str(),
                    projection.tags_json.as_str(),
                    projection.links_json.as_str(),
                    projection.parser_version,
                    projection.updated_at.as_str(),
                    brains_domain::timestamp::now_rfc3339(),
                ],
            )
            .await
            .map_err(index_err)?;
        Ok(())
    }
    async fn scan_literal(
        &self,
        prefix: Option<&str>,
        after: &str,
        limit: usize,
        text: &str,
        tags: &[String],
    ) -> DomainResult<Vec<LiteralRow>> {
        if text.is_empty() {
            return Err(DomainError::InvalidArgument(
                "literal query must not be empty".into(),
            ));
        }
        let conn = self.conn.lock().await;
        query(
            &conn,
            LITERAL_WINDOW_SQL,
            params![
                normalize_prefix(prefix),
                after,
                limit as i64,
                text,
                serde_json::to_string(tags).expect("string array")
            ],
            |row| {
                Ok(LiteralRow {
                    path: row.get(0)?,
                    revision: row.get(1)?,
                    match_pos: row.get(2)?,
                    snippet: row.get(3)?,
                })
            },
        )
        .await
        .map_err(index_err)
    }
    async fn search_fts(
        &self,
        text: &str,
        prefix: Option<&str>,
        tags: &[String],
        offset: usize,
        limit: usize,
    ) -> DomainResult<Vec<(String, String, String)>> {
        if self.full_text_search() == FullTextSearch::Disabled {
            return Err(DomainError::CapabilityUnsupported("fts_search"));
        }
        let conn = self.conn.lock().await;
        // Native FTS uses Tantivy grammar, BM25 ranking and a bounded plain
        // preview. It never translates FTS5 syntax or returns unbounded HTML.
        query(&conn, "SELECT path, revision, substr(body, 1, 240) FROM files
            WHERE fts_match(title, body, ?1) AND deleted=0 AND searchable=1
              AND (?2 IS NULL OR path = ?2 OR (path >= ?2 || '/' AND path < ?2 || '0'))
              AND NOT EXISTS (SELECT 1 FROM json_each(?3) AS wanted WHERE NOT EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = wanted.value))
            ORDER BY fts_score(title, body, ?1) DESC, path LIMIT ?4 OFFSET ?5",
            params![text, normalize_prefix(prefix), serde_json::to_string(tags).expect("string array"), limit as i64, offset as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .await.map_err(search_err)
    }
    async fn list_files(
        &self,
        filter: &ListFilter,
        after_path: Option<&str>,
        after_updated: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<ListRow>> {
        let prefix = normalize_prefix(filter.prefix.as_deref());
        let order_desc = filter.order == ListOrder::UpdatedDesc;
        let mut sql = String::from("SELECT path, revision, generation, deleted, kind, title, size, sha256, media_type, updated_at
            FROM files WHERE (?1 = 1 OR deleted = 0) AND (?2 IS NULL OR path = ?2 OR (path >= ?2 || '/' AND path < ?2 || '0'))
            AND NOT EXISTS (SELECT 1 FROM json_each(?3) AS wanted WHERE NOT EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = wanted.value))
            AND (?4 IS NULL OR kind=?4)");
        if order_desc {
            sql.push_str(" AND (?5 IS NULL OR updated_at < ?5 OR (updated_at = ?5 AND path > ?6)) ORDER BY updated_at DESC, path LIMIT ?7");
        } else {
            sql.push_str(" AND (?6 IS NULL OR path > ?6) ORDER BY path LIMIT ?7");
        }
        let conn = self.conn.lock().await;
        query(
            &conn,
            &sql,
            params![
                filter.include_deleted as i64,
                prefix,
                serde_json::to_string(&filter.tags).expect("string array"),
                filter.kind.map(|kind| kind.as_str()),
                after_updated,
                after_path,
                limit as i64
            ],
            map_list_row,
        )
        .await
        .map_err(index_err)
    }
    async fn links_outbound(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<LinkRow>> {
        let conn = self.conn.lock().await;
        query(&conn, "SELECT source,target,kind,source_revision FROM links WHERE source=?1 AND (?2 IS NULL OR target>?2) ORDER BY target LIMIT ?3",
            params![path, after, limit as i64], map_link_row).await.map_err(index_err)
    }
    async fn links_backlinks(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<LinkRow>> {
        let conn = self.conn.lock().await;
        query(&conn, "SELECT source,target,kind,source_revision FROM links WHERE target=?1 AND (?2 IS NULL OR source>?2) ORDER BY source LIMIT ?3",
            params![path, after, limit as i64], map_link_row).await.map_err(index_err)
    }
    async fn files_count(&self) -> DomainResult<u64> {
        let conn = self.conn.lock().await;
        let rows = query(&conn, "SELECT count(*) FROM files", (), |row| {
            row.get::<i64>(0)
        })
        .await
        .map_err(index_err)?;
        let count = rows
            .first()
            .ok_or_else(|| DomainError::IndexUnavailable("missing file count".into()))?;
        Ok((*count).max(0) as u64)
    }
}
fn map_list_row(row: &Row) -> turso::Result<ListRow> {
    Ok(ListRow {
        path: row.get(0)?,
        revision: row.get(1)?,
        generation: row.get(2)?,
        deleted: row.get::<i64>(3)? != 0,
        kind: FileKind::parse(&row.get::<String>(4)?).unwrap_or(FileKind::Binary),
        title: row.get(5)?,
        size: row.get(6)?,
        sha256: row.get(7)?,
        media_type: row.get(8)?,
        updated_at: row.get(9)?,
    })
}
fn map_link_row(row: &Row) -> turso::Result<LinkRow> {
    Ok(LinkRow {
        source: row.get(0)?,
        target: row.get(1)?,
        kind: row.get(2)?,
        source_revision: row.get(3)?,
    })
}
#[cfg(test)]
mod tests;
