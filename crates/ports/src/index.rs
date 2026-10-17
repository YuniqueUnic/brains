//! Derived-index port (design §10): a rebuildable projection of committed
//! heads. Losing it degrades search/links only — never reads or writes.
//! Futures are `+ Send` (see store.rs for why).

use std::future::Future;

use brains_domain::{DomainResult, FileKind};

/// Bump when the parser's projection shape changes; the monotonic upsert
/// then refreshes rows at equal generation (design §10.2).
// Version 3 reads standard YAML scalars and sequences for frontmatter tags.
// Version 4 adds root-relative link targets and the lifecycle/trust/provenance
// projection (`meta_json`).
pub const PARSER_VERSION: i64 = 4;

/// One searchable projection derived from a verified, committed revision.
#[derive(Clone, Debug)]
pub struct FileProjection {
    pub path: String,
    pub revision: String,
    pub generation: i64,
    pub deleted: bool,
    pub kind: FileKind,
    pub title: String,
    /// Raw UTF-8 text including frontmatter (grep-like semantics); empty for
    /// binary and trash rows.
    pub body: String,
    pub size: i64,
    pub sha256: Option<String>,
    pub media_type: String,
    pub tags_json: String,
    pub links_json: String,
    /// Lifecycle/trust/provenance projection (type, description, status,
    /// stale_after, generated, verified, sources, trust tier). Derived from
    /// frontmatter; the note stays authoritative.
    pub meta_json: String,
    pub parser_version: i64,
    /// From HEAD, never the time of indexing (design §10.1).
    pub updated_at: String,
}

/// One candidate row of a bounded literal scan — matches and non-matches, so
/// the caller can advance the cursor past the full window.
#[derive(Clone, Debug)]
pub struct LiteralRow {
    pub path: String,
    pub revision: String,
    pub match_pos: i64,
    pub snippet: String,
}

#[derive(Clone, Debug)]
pub struct ListRow {
    pub path: String,
    pub revision: String,
    pub generation: i64,
    pub deleted: bool,
    pub kind: FileKind,
    pub title: String,
    pub size: i64,
    pub sha256: Option<String>,
    pub media_type: String,
    pub updated_at: String,
    /// Lifecycle/trust/provenance projection; `{}` when the note carries no
    /// recognized fields.
    pub meta_json: String,
}

#[derive(Clone, Debug)]
pub struct LinkRow {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub source_revision: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListOrder {
    #[default]
    Path,
    UpdatedDesc,
}

/// Lifecycle/trust filters over the `meta_json` projection (issue #13).
/// `None` leaves the dimension unfiltered; a set requires membership.
#[derive(Clone, Debug, Default)]
pub struct MetaFilter {
    /// `draft` | `stable` | `deprecated`
    pub status: Option<String>,
    /// Note `type` frontmatter, exact match.
    pub type_: Option<String>,
    /// `unverified` | `machine-confirmed` | `human-reviewed`
    pub trust: Option<String>,
    /// `true` = only stale (`stale_after` reached), `false` = only fresh.
    pub stale: Option<bool>,
    /// ISO date used to compute staleness at query time (UTC `YYYY-MM-DD`).
    pub today: Option<String>,
}

impl MetaFilter {
    pub fn is_empty(&self) -> bool {
        self.status.is_none()
            && self.type_.is_none()
            && self.trust.is_none()
            && self.stale.is_none()
    }
}

/// Positional SQL clauses for [`MetaFilter`] over the `meta_json` column.
/// Returns the clause text with `?N` placeholders starting at `start`, plus
/// the bound values in order: status, type, trust, stale (0/1), today.
/// Unset dimensions bind NULL, which their clause short-circuits on — the
/// same SQL shape serves filtered and unfiltered queries.
pub fn meta_filter_clause(meta: &MetaFilter, start: usize) -> (String, Vec<String>) {
    let stale = match meta.stale {
        Some(true) => "1".to_owned(),
        Some(false) => "0".to_owned(),
        None => String::new(),
    };
    let params = vec![
        meta.status.clone().unwrap_or_default(),
        meta.type_.clone().unwrap_or_default(),
        meta.trust.clone().unwrap_or_default(),
        stale,
        meta.today.clone().unwrap_or_default(),
    ];
    let (s, t, tr, want, today) = (start, start + 1, start + 2, start + 3, start + 4);
    let sql = [
        format!(" AND (?{s} IS NULL OR json_extract(meta_json, '$.status') = ?{s})"),
        format!(" AND (?{t} IS NULL OR json_extract(meta_json, '$.type') = ?{t})"),
        format!(" AND (?{tr} IS NULL OR json_extract(meta_json, '$.trust') = ?{tr})"),
        format!(
            " AND (?{want} IS NULL OR\
              (CASE WHEN json_extract(meta_json, '$.stale_after') IS NULL THEN 0 \
              WHEN json_extract(meta_json, '$.stale_after') <= ?{today} THEN 1 \
              ELSE 0 END) = ?{want})"
        ),
    ]
    .concat();
    (sql, params)
}

/// Index listing filters. Only meaningful with `source=index` at the API
/// layer; the store source never reaches here.
#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub prefix: Option<String>,
    pub tags: Vec<String>,
    pub kind: Option<FileKind>,
    pub meta: MetaFilter,
    pub include_deleted: bool,
    pub order: ListOrder,
}

pub trait IndexStore {
    /// Available full-text query grammar; consumers report this capability.
    fn full_text_search(&self) -> brains_domain::FullTextSearch;

    /// Monotonic upsert (design §10.2): an older generation, or an older
    /// parser at the same generation, must never overwrite a newer row.
    fn upsert(&self, projection: &FileProjection) -> impl Future<Output = DomainResult<()>> + Send;

    /// Drop the whole projection of one path (permanent delete): file row,
    /// FTS entry, links and tags. Missing path is fine — purge is idempotent
    /// at the index layer.
    fn remove_path(&self, path: &str) -> impl Future<Output = DomainResult<()>> + Send;

    /// One bounded literal window (design §10.3). Rows are ordered by path;
    /// `prefix` selects the exact path and its descendants; `None` = whole vault.
    fn scan_literal(
        &self,
        prefix: Option<&str>,
        after: &str,
        limit: usize,
        query: &str,
        tags: &[String],
        meta: &MetaFilter,
    ) -> impl Future<Output = DomainResult<Vec<LiteralRow>>> + Send;

    /// Explicit engine-native full-text search. Invalid syntax is an error, not a fallback.
    /// `prefix` selects the exact path and its descendants (`None` = whole
    /// vault) and `tags` requires every requested tag row — filters are never silently
    /// ignored (design §11.1).
    fn search_fts(
        &self,
        query: &str,
        prefix: Option<&str>,
        tags: &[String],
        meta: &MetaFilter,
        offset: usize,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<(String, String, String)>>> + Send; // path, revision, snippet

    fn list_files(
        &self,
        filter: &ListFilter,
        after_path: Option<&str>,
        after_updated: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<ListRow>>> + Send;

    fn links_outbound(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<LinkRow>>> + Send;
    fn links_backlinks(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<LinkRow>>> + Send;

    /// Rows in the projection; zero rows mean the index is still building.
    fn files_count(&self) -> impl Future<Output = DomainResult<u64>> + Send;
}
