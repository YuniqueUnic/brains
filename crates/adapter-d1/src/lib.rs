//! D1 derived index (design §9.2, §10). The schema and bounded queries come
//! from `migrations/*.sql` at the repository root. The projection upsert
//! and literal scan are shared with the native Turso adapter. Schema objects
//! are applied by `wrangler d1 migrations apply` (all statements are
//! idempotent); this adapter only runs queries.

mod metered;
pub mod resources;
pub mod settings;

use brains_domain::path::normalize_prefix;
use std::future::Future;

use brains_domain::{DomainError, DomainResult, FileKind};
use brains_ports::{
    FileProjection, IndexStore, LinkRow, ListFilter, ListOrder, ListRow, LiteralRow, MetaFilter,
    meta_filter_clause,
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
    budget: Option<std::sync::Arc<brains_application::resources::ResourceBudget>>,
}

impl D1Index {
    pub fn with_budget(
        mut self,
        budget: std::sync::Arc<brains_application::resources::ResourceBudget>,
    ) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn new(db: D1Database) -> Self {
        Self { db, budget: None }
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

/// Meta-filter binding: an unset dimension is the empty string → SQL NULL.
fn js_str_or_null(value: &str) -> JsValue {
    if value.is_empty() {
        JsValue::NULL
    } else {
        JsValue::from_str(value)
    }
}

/// Meta-filter binding for the stale flag: `"0"`/`"1"` → number, else NULL.
fn js_num_or_null(value: &str) -> JsValue {
    match value {
        "0" => JsValue::from_f64(0.0),
        "1" => JsValue::from_f64(1.0),
        _ => JsValue::NULL,
    }
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
    #[serde(default)]
    meta_json: String,
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
            meta_json: row.meta_json,
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
    fn full_text_search(&self) -> brains_domain::FullTextSearch {
        brains_domain::FullTextSearch::Fts5
    }

    fn remove_path(&self, path: &str) -> impl Future<Output = DomainResult<()>> + Send {
        let path = path.to_owned();
        SendFuture::new(async move {
            // The files_ad trigger cascades the FTS row, links and tags.
            self.execute(
                "index remove",
                "DELETE FROM files WHERE path = ?1",
                &[js_str(&path)],
            )
            .await
        })
    }

    fn upsert(&self, projection: &FileProjection) -> impl Future<Output = DomainResult<()>> + Send {
        let projection = projection.clone();
        SendFuture::new(async move {
            self.execute(
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
                    js_str(&projection.meta_json),
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
        prefix: Option<&str>,
        after: &str,
        limit: usize,
        query: &str,
        tags: &[String],
        meta: &MetaFilter,
    ) -> impl Future<Output = DomainResult<Vec<LiteralRow>>> + Send {
        let prefix = normalize_prefix(prefix).map(str::to_owned);
        let after = after.to_owned();
        let query = query.to_owned();
        let tags = serde_json::to_string(tags).expect("string array");
        // The meta filter is inlined in LITERAL_WINDOW_SQL (?6..?10); see
        // brains_ports::meta_filter_clause for the value encoding.
        let meta_params = meta_filter_clause(meta, 6).1;
        SendFuture::new(async move {
            if query.is_empty() {
                return Err(DomainError::InvalidArgument(
                    "literal query must not be empty".into(),
                ));
            }
            let rows = self
                .query::<LiteralDbRow>(
                    "literal window",
                    LITERAL_WINDOW_SQL,
                    &[
                        js_opt(prefix.as_deref()),
                        js_str(&after),
                        js_num(limit as i64),
                        js_str(&query),
                        js_str(&tags),
                        js_str_or_null(&meta_params[0]),
                        js_str_or_null(&meta_params[1]),
                        js_str_or_null(&meta_params[2]),
                        js_num_or_null(&meta_params[3]),
                        js_str_or_null(&meta_params[4]),
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
        prefix: Option<&str>,
        tags: &[String],
        meta: &MetaFilter,
        offset: usize,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<(String, String, String)>>> + Send {
        let query = query.to_owned();
        let prefix = normalize_prefix(prefix).map(str::to_owned);
        let tags = serde_json::to_string(tags).expect("string array");
        let (meta_sql, meta_params) = meta_filter_clause(meta, 4);
        SendFuture::new(async move {
            // Path and tag bounds are explicit
            // SQL branches, and a bad MATCH is a 400 — never a fake-empty
            // fallback.
            let sql = format!(
                "SELECT files_fts.path AS path, files.revision AS revision,
                        snippet(files_fts, 2, '', '', '…', 16) AS snip
                 FROM files_fts JOIN files ON files.rowid = files_fts.rowid
                 WHERE files_fts MATCH ?1 AND files.deleted = 0
                 AND (?2 IS NULL OR files.path = ?2 OR (files.path >= ?2 || '/' AND files.path < ?2 || '0'))
                 AND NOT EXISTS (SELECT 1 FROM json_each(?3) AS wanted WHERE NOT EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = wanted.value)){}
                 ORDER BY rank LIMIT ?9 OFFSET ?10",
                meta_sql
            );
            let rows = self
                .query::<FtsRow>(
                    "fts search",
                    &sql,
                    &[
                        js_str(&query),
                        js_opt(prefix.as_deref()),
                        js_str(&tags),
                        js_str_or_null(&meta_params[0]),
                        js_str_or_null(&meta_params[1]),
                        js_str_or_null(&meta_params[2]),
                        js_num_or_null(&meta_params[3]),
                        js_str_or_null(&meta_params[4]),
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
        let filter = ListFilter {
            prefix: filter.prefix.clone(),
            tags: filter.tags.clone(),
            kind: filter.kind,
            meta: filter.meta.clone(),
            include_deleted: filter.include_deleted,
            order: filter.order,
        };
        let after_path = after_path.map(str::to_owned);
        let after_updated = after_updated.map(str::to_owned);
        let (meta_sql, meta_params) = meta_filter_clause(&filter.meta, 8);
        SendFuture::new(async move {
            let prefix = normalize_prefix(filter.prefix.as_deref());
            let order_desc = filter.order == ListOrder::UpdatedDesc;

            let mut sql = String::from(
                "SELECT path, revision, generation, deleted, kind, title, size, sha256, media_type, updated_at, meta_json \
                 FROM files WHERE (?1 = 1 OR deleted = 0) \
                 AND (?2 IS NULL OR path = ?2 OR (path >= ?2 || '/' AND path < ?2 || '0'))",
            );
            sql.push_str(" AND NOT EXISTS (SELECT 1 FROM json_each(?3) AS wanted WHERE NOT EXISTS (SELECT 1 FROM tags WHERE tags.path = files.path AND tags.tag = wanted.value))");
            if filter.kind.is_some() {
                sql.push_str(" AND kind = ?4");
            } else {
                sql.push_str(" AND ?4 IS NULL");
            }
            if order_desc {
                sql.push_str(
                    " AND (?5 IS NULL OR updated_at < ?5 OR (updated_at = ?5 AND path > ?6))",
                );
            } else {
                sql.push_str(" AND (?6 IS NULL OR path > ?6)");
            }
            sql.push_str(&meta_sql);
            if order_desc {
                sql.push_str(" ORDER BY updated_at DESC, path LIMIT ?7");
            } else {
                sql.push_str(" ORDER BY path LIMIT ?7");
            }

            let rows = self
                .query::<ListDbRow>(
                    "index listing",
                    &sql,
                    &[
                        js_num(filter.include_deleted as i64),
                        js_opt(prefix),
                        js_str(&serde_json::to_string(&filter.tags).expect("string array")),
                        js_opt(filter.kind.map(|k| k.as_str())),
                        js_opt(after_updated.as_deref()),
                        js_opt(after_path.as_deref()),
                        js_num(limit as i64),
                        js_str_or_null(&meta_params[0]),
                        js_str_or_null(&meta_params[1]),
                        js_str_or_null(&meta_params[2]),
                        js_num_or_null(&meta_params[3]),
                        js_str_or_null(&meta_params[4]),
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
        let path = path.to_owned();
        let after = after.map(str::to_owned);
        SendFuture::new(async move {
            self.query::<LinkDbRow>(
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
        let path = path.to_owned();
        let after = after.map(str::to_owned);
        SendFuture::new(async move {
            self.query::<LinkDbRow>(
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
        SendFuture::new(async move {
            let rows = self
                .query::<Count>("files count", "SELECT COUNT(*) AS n FROM files", &[])
                .await?;
            Ok(rows.first().map(|c| c.n.max(0) as u64).unwrap_or(0))
        })
    }
}
