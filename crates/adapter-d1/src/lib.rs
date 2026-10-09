//! D1 derived index (design §9.2, §10). The schema and bounded queries come
//! from `migrations/*.sql` at the repository root — byte-identical to the
//! SQLite adapter — so the two index backends cannot drift. Schema objects
//! are applied by `wrangler d1 migrations apply` (all statements are
//! idempotent); this adapter only runs queries.

use std::future::Future;

use brains_domain::{DomainError, DomainResult, FileKind};
use brains_ports::{
    FileProjection, IndexStore, LinkRow, ListFilter, ListOrder, ListRow, LiteralRow,
    prefix_upper_bound,
};
use wasm_bindgen::JsValue;
use worker::D1Database;
use worker::send::SendFuture;

const UPSERT_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/upsert.sql"
));
const LITERAL_WINDOW_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/literal_window.sql"
));

pub struct D1Index {
    db: D1Database,
}

impl D1Index {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

fn index_err(what: &str, e: worker::Error) -> DomainError {
    DomainError::IndexUnavailable(format!("{what}: {e}"))
}

fn index_msg(what: &str, message: String) -> DomainError {
    DomainError::IndexUnavailable(format!("{what}: {message}"))
}

fn js_str(value: &str) -> JsValue {
    JsValue::from_str(value)
}

fn js_opt(value: Option<&str>) -> JsValue {
    match value {
        Some(v) => JsValue::from_str(v),
        None => JsValue::NULL,
    }
}

fn js_num(value: i64) -> JsValue {
    JsValue::from_f64(value as f64)
}

async fn run(db: &D1Database, what: &str, sql: &str, params: &[JsValue]) -> DomainResult<()> {
    db.prepare(sql)
        .bind(params)
        .map_err(|e| index_err(what, e))?
        .run()
        .await
        .map_err(|e| index_err(what, e))
        .and_then(|result| {
            if result.success() {
                Ok(())
            } else {
                Err(index_msg(
                    what,
                    result.error().unwrap_or_else(|| "unknown".into()),
                ))
            }
        })
}

async fn query_rows<T: for<'a> serde::de::Deserialize<'a>>(
    db: &D1Database,
    what: &str,
    sql: &str,
    params: &[JsValue],
) -> DomainResult<Vec<T>> {
    let result = db
        .prepare(sql)
        .bind(params)
        .map_err(|e| index_err(what, e))?
        .all()
        .await
        .map_err(|e| index_err(what, e))?;
    if !result.success() {
        return Err(index_msg(
            what,
            result.error().unwrap_or_else(|| "unknown".into()),
        ));
    }
    result.results::<T>().map_err(|e| index_err(what, e))
}

/// FTS rows ordered by rank: path, revision, snippet.
#[derive(serde::Deserialize)]
struct FtsRow {
    path: String,
    revision: String,
    snip: String,
}

#[derive(serde::Deserialize)]
struct LiteralDbRow {
    path: String,
    revision: String,
    match_pos: i64,
    snippet: String,
}

#[derive(serde::Deserialize)]
struct ListDbRow {
    path: String,
    revision: String,
    generation: i64,
    deleted: i64,
    kind: String,
    title: String,
    size: i64,
    sha256: Option<String>,
    media_type: String,
    updated_at: String,
}

impl From<ListDbRow> for ListRow {
    fn from(row: ListDbRow) -> Self {
        ListRow {
            path: row.path,
            revision: row.revision,
            generation: row.generation,
            deleted: row.deleted != 0,
            kind: FileKind::parse(&row.kind).unwrap_or(FileKind::Binary),
            title: row.title,
            size: row.size,
            sha256: row.sha256,
            media_type: row.media_type,
            updated_at: row.updated_at,
        }
    }
}

#[derive(serde::Deserialize)]
struct Count {
    n: i64,
}

#[derive(serde::Deserialize)]
struct LinkDbRow {
    source: String,
    target: String,
    kind: String,
    source_revision: String,
}

impl From<LinkDbRow> for LinkRow {
    fn from(row: LinkDbRow) -> Self {
        LinkRow {
            source: row.source,
            target: row.target,
            kind: row.kind,
            source_revision: row.source_revision,
        }
    }
}

impl IndexStore for D1Index {
    fn remove_path(&self, path: &str) -> impl Future<Output = DomainResult<()>> + Send {
        let db = &self.db;
        let path = path.to_owned();
        SendFuture::new(async move {
            // The files_ad trigger cascades the FTS row, links and tags.
            run(
                db,
                "index remove",
                "DELETE FROM files WHERE path = ?1",
                &[js_str(&path)],
            )
            .await
        })
    }

    fn upsert(&self, projection: &FileProjection) -> impl Future<Output = DomainResult<()>> + Send {
        let db = &self.db;
        let projection = projection.clone();
        SendFuture::new(async move {
            run(
                db,
                "index upsert",
                UPSERT_SQL,
                &[
                    js_str(&projection.path),
                    js_str(&projection.revision),
                    js_num(projection.generation),
                    js_num(projection.deleted as i64),
                    js_str(projection.kind.as_str()),
                    js_num(projection.kind.searchable() as i64),
                    js_str(&projection.title),
                    js_str(&projection.body),
                    js_num(projection.size),
                    js_opt(projection.sha256.as_deref()),
                    js_str(&projection.media_type),
                    js_str(&projection.tags_json),
                    js_str(&projection.links_json),
                    js_num(projection.parser_version),
                    js_str(&projection.updated_at),
                    js_str(&brains_domain::timestamp::now_rfc3339()),
                ],
            )
            .await
        })
    }

    fn scan_literal(
        &self,
        lower: &str,
        upper: Option<&str>,
        after: &str,
        limit: usize,
        query: &str,
        tag: Option<&str>,
    ) -> impl Future<Output = DomainResult<Vec<LiteralRow>>> + Send {
        let db = &self.db;
        let lower = lower.to_owned();
        let upper = upper.map(str::to_owned);
        let after = after.to_owned();
        let query = query.to_owned();
        let tag = tag.map(str::to_owned);
        SendFuture::new(async move {
            if query.is_empty() {
                return Err(DomainError::InvalidArgument(
                    "literal query must not be empty".into(),
                ));
            }
            let rows = query_rows::<LiteralDbRow>(
                db,
                "literal window",
                LITERAL_WINDOW_SQL,
                &[
                    js_str(&lower),
                    js_opt(upper.as_deref()),
                    js_str(&after),
                    js_num(limit as i64),
                    js_str(&query),
                    js_opt(tag.as_deref()),
                ],
            )
            .await?;
            Ok(rows
                .into_iter()
                .map(|row| LiteralRow {
                    path: row.path,
                    revision: row.revision,
                    match_pos: row.match_pos,
                    snippet: row.snippet,
                })
                .collect())
        })
    }

    fn search_fts(
        &self,
        query: &str,
        lower: &str,
        upper: Option<&str>,
        tag: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<(String, String, String)>>> + Send {
        let db = &self.db;
        let query = query.to_owned();
        let lower = lower.to_owned();
        let upper = upper.map(str::to_owned);
        let tag = tag.map(str::to_owned);
        SendFuture::new(async move {
            // Mirrors the SQLite adapter: path and tag bounds are explicit
            // SQL branches, and a bad MATCH is a 400 — never a fake-empty
            // fallback.
            let mut sql = String::from(
                "SELECT files_fts.path AS path, files.revision AS revision,
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
            let rows = query_rows::<FtsRow>(
                db,
                "fts search",
                &sql,
                &[
                    js_str(&query),
                    js_str(&lower),
                    js_opt(upper.as_deref()),
                    js_opt(tag.as_deref()),
                    js_num(limit as i64),
                    js_num(offset as i64),
                ],
            )
            .await
            .map_err(|e| match e {
                DomainError::IndexUnavailable(m) => {
                    DomainError::InvalidArgument(format!("invalid FTS query: {m}"))
                }
                other => other,
            })?;
            Ok(rows
                .into_iter()
                .map(|row| (row.path, row.revision, row.snip))
                .collect())
        })
    }

    fn list_files(
        &self,
        filter: &ListFilter,
        after_path: Option<&str>,
        after_updated: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<ListRow>>> + Send {
        let db = &self.db;
        let filter = ListFilter {
            prefix: filter.prefix.clone(),
            tag: filter.tag.clone(),
            kind: filter.kind,
            include_deleted: filter.include_deleted,
            order: filter.order,
        };
        let after_path = after_path.map(str::to_owned);
        let after_updated = after_updated.map(str::to_owned);
        SendFuture::new(async move {
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
                sql.push_str(
                    " AND (?6 IS NULL OR updated_at < ?6 OR (updated_at = ?6 AND path > ?7))",
                );
            } else {
                sql.push_str(" AND (?6 IS NULL OR path > ?7)");
            }
            if order_desc {
                sql.push_str(" ORDER BY updated_at DESC, path LIMIT ?8");
            } else {
                sql.push_str(" ORDER BY path LIMIT ?8");
            }

            let rows = query_rows::<ListDbRow>(
                db,
                "index listing",
                &sql,
                &[
                    js_num(filter.include_deleted as i64),
                    js_str(&lower),
                    js_opt(upper.as_deref()),
                    js_opt(filter.tag.as_deref()),
                    js_opt(filter.kind.map(|k| k.as_str())),
                    js_opt(after_updated.as_deref()),
                    js_opt(after_path.as_deref()),
                    js_num(limit as i64),
                ],
            )
            .await?;
            Ok(rows.into_iter().map(ListRow::from).collect())
        })
    }

    fn links_outbound(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<LinkRow>>> + Send {
        let db = &self.db;
        let path = path.to_owned();
        let after = after.map(str::to_owned);
        SendFuture::new(async move {
            query_rows::<LinkDbRow>(
                db,
                "links outbound",
                "SELECT source, target, kind, source_revision FROM links \
                 WHERE source = ?1 AND (?2 IS NULL OR target > ?2) ORDER BY target LIMIT ?3",
                &[
                    js_str(&path),
                    js_opt(after.as_deref()),
                    js_num(limit as i64),
                ],
            )
            .await
            .map(|rows| rows.into_iter().map(LinkRow::from).collect())
        })
    }

    fn links_backlinks(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<LinkRow>>> + Send {
        let db = &self.db;
        let path = path.to_owned();
        let after = after.map(str::to_owned);
        SendFuture::new(async move {
            query_rows::<LinkDbRow>(
                db,
                "links backlinks",
                "SELECT source, target, kind, source_revision FROM links \
                 WHERE target = ?1 AND (?2 IS NULL OR source > ?2) ORDER BY source LIMIT ?3",
                &[
                    js_str(&path),
                    js_opt(after.as_deref()),
                    js_num(limit as i64),
                ],
            )
            .await
            .map(|rows| rows.into_iter().map(LinkRow::from).collect())
        })
    }

    fn files_count(&self) -> impl Future<Output = DomainResult<u64>> + Send {
        let db = &self.db;
        SendFuture::new(async move {
            let rows =
                query_rows::<Count>(db, "files count", "SELECT COUNT(*) AS n FROM files", &[])
                    .await?;
            Ok(rows.first().map(|c| c.n.max(0) as u64).unwrap_or(0))
        })
    }
}
