//! Derived-index port (design §10): a rebuildable projection of committed
//! heads. Losing it degrades search/links only — never reads or writes.
//! Futures are `+ Send` (see store.rs for why).

use std::future::Future;

use brains_domain::{DomainResult, FileKind};

/// Bump when the parser's projection shape changes; the monotonic upsert
/// then refreshes rows at equal generation (design §10.2).
// Version 2 projects tags and resolved link relationships as unique sets.
pub const PARSER_VERSION: i64 = 2;

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

/// Index listing filters. Only meaningful with `source=index` at the API
/// layer; the store source never reaches here.
#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub prefix: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<FileKind>,
    pub include_deleted: bool,
    pub order: ListOrder,
}

/// Exclusive upper bound for a prefix range scan: increment the final byte.
/// A prefix ending in `0xFF` (or an empty prefix) has no upper bound.
pub fn prefix_upper_bound(prefix: &str) -> String {
    let mut bytes = prefix.as_bytes().to_vec();
    while let Some(last) = bytes.last_mut() {
        if *last == 0xFF {
            bytes.pop();
        } else {
            *last += 1;
            break;
        }
    }
    String::from_utf8(bytes).unwrap_or_default()
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
    /// `upper` is an exclusive prefix bound, `None` = whole vault.
    fn scan_literal(
        &self,
        lower: &str,
        upper: Option<&str>,
        after: &str,
        limit: usize,
        query: &str,
        tag: Option<&str>,
    ) -> impl Future<Output = DomainResult<Vec<LiteralRow>>> + Send;

    /// Explicit engine-native full-text search. Invalid syntax is an error, not a fallback.
    /// `lower`/`upper` bound the path range (exclusive upper, `None` = whole
    /// vault) and `tag` requires the tag row — filters are never silently
    /// ignored (design §11.1).
    fn search_fts(
        &self,
        query: &str,
        lower: &str,
        upper: Option<&str>,
        tag: Option<&str>,
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
