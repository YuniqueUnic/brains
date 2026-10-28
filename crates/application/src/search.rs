//! Bounded search (design §10.3): literal windows by default, explicit FTS
//! optionally. Zero hits plus a pending cursor never means "whole vault has
//! no results"; index failures are loud 503s, never fake-empty pages.

use brains_domain::path::normalize_prefix;
use brains_domain::{AccessLevel, DomainError, DomainResult, Endpoint, Principal, VaultPath};
use brains_ports::LinkRow;

use crate::Vault;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Literal,
    Fts,
    /// Weighted reciprocal rank fusion of every available retriever
    /// (issue #18): exact path/title match, literal body, FTS and semantic
    /// when the runtime provides an embedding model. Unavailable retrievers
    /// are reported in the response, never pretended away.
    Hybrid,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SearchQuery {
    pub q: String,
    pub mode: SearchMode,
    pub prefix: Option<String>,
    pub tags: Vec<String>,
    /// Lifecycle/trust filters (issue #13), applied identically to literal
    /// and FTS retrieval.
    pub status: Option<String>,
    pub type_: Option<String>,
    pub trust: Option<String>,
    /// `Some(true)` = only stale, `Some(false)` = only fresh.
    pub stale: Option<bool>,
    pub cursor: Option<String>,
    pub scan_limit: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            q: String::new(),
            mode: SearchMode::Literal,
            prefix: None,
            tags: Vec::new(),
            status: None,
            type_: None,
            trust: None,
            stale: None,
            cursor: None,
            scan_limit: brains_domain::Limits::default().scan_window_max,
        }
    }
}

pub struct SearchHit {
    pub path: String,
    pub indexed_revision: String,
    pub snippet: String,
    /// Hybrid only: which retrievers matched, with the rank in each.
    pub retrieved_by: Vec<Retrieval>,
}

/// One retriever's view of a hit in hybrid mode (issue #18).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Retrieval {
    pub retriever: String,
    pub rank: usize,
}

pub struct SearchResult {
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub complete: bool,
    pub scanned: usize,
    pub building: bool,
    /// Hybrid only: retrievers that did not contribute and why.
    pub unavailable: Vec<String>,
}

// Structural serialization prevents delimiters inside q/prefix/tags from
// colliding, and binds the complete normalized filter set to each cursor.
fn search_filter(query: &SearchQuery) -> String {
    serde_json::to_string(&(
        query.mode,
        &query.q,
        &query.prefix,
        &query.tags,
        &query.status,
        &query.type_,
        &query.trust,
        &query.stale,
    ))
    .expect("query filters contain only serializable strings")
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    pub async fn search(
        &self,
        principal: &Principal,
        mut query: SearchQuery,
    ) -> DomainResult<SearchResult> {
        query.prefix = normalize_prefix(query.prefix.as_deref()).map(str::to_owned);
        crate::normalize_tags(&mut query.tags);
        query.cursor = query.cursor.filter(|value| !value.is_empty());
        if query.q.is_empty() || query.q.len() > 512 {
            return Err(DomainError::InvalidArgument(
                "q must contain 1..512 bytes".into(),
            ));
        }
        if query.scan_limit == 0 || query.scan_limit > self.limits.scan_window_max {
            return Err(DomainError::InvalidArgument(format!(
                "scan_limit must be between 1 and {}",
                self.limits.scan_window_max
            )));
        }
        // The effective prefix must sit inside one granted read scope; results
        // are never silently intersected (design §15.2).
        match &query.prefix {
            Some(prefix) => principal.require(AccessLevel::Read, prefix)?,
            None => {
                if !principal.allows(AccessLevel::Read, "") {
                    return Err(DomainError::Forbidden(
                        "token scopes do not cover the whole vault; pass an explicit prefix".into(),
                    ));
                }
            }
        }
        let meta = crate::meta_filter(
            query.status.clone(),
            query.type_.clone(),
            query.trust.clone(),
            query.stale,
        )?;
        let building = self.index.files_count().await? == 0;
        match query.mode {
            SearchMode::Literal => self.search_literal(query, meta, building).await,
            SearchMode::Fts => self.search_fts(query, meta, building).await,
            SearchMode::Hybrid => self.search_hybrid(principal, query, meta, building).await,
        }
    }

    async fn search_literal(
        &self,
        query: SearchQuery,
        meta: brains_ports::MetaFilter,
        building: bool,
    ) -> DomainResult<SearchResult> {
        let filter = search_filter(&query);
        let after = match &query.cursor {
            None => String::new(),
            Some(cursor) => self.cursors.verify(Endpoint::Search, &filter, cursor)?,
        };
        let rows = self
            .index
            .scan_literal(
                query.prefix.as_deref(),
                &after,
                query.scan_limit,
                &query.q,
                &query.tags,
                &meta,
            )
            .await?;
        let scanned = rows.len();
        let complete = scanned < query.scan_limit;
        let next_cursor = rows
            .last()
            .filter(|_| !complete)
            .map(|last| self.cursors.issue(Endpoint::Search, &filter, &last.path));
        let items = rows
            .into_iter()
            .filter(|row| row.match_pos > 0)
            .map(|row| SearchHit {
                path: row.path,
                indexed_revision: row.revision,
                snippet: row.snippet,
                retrieved_by: Vec::new(),
            })
            .collect();
        Ok(SearchResult {
            items,
            next_cursor,
            complete,
            scanned,
            building,
            unavailable: Vec::new(),
        })
    }

    async fn search_fts(
        &self,
        query: SearchQuery,
        meta: brains_ports::MetaFilter,
        building: bool,
    ) -> DomainResult<SearchResult> {
        let filter = search_filter(&query);
        let offset = match &query.cursor {
            None => 0usize,
            Some(cursor) => self
                .cursors
                .verify(Endpoint::Search, &filter, cursor)?
                .parse::<usize>()
                .map_err(|_| DomainError::InvalidArgument("malformed cursor".into()))?,
        };
        let rows = self
            .index
            .search_fts(
                &query.q,
                query.prefix.as_deref(),
                &query.tags,
                &meta,
                offset,
                query.scan_limit,
            )
            .await?;
        let scanned = rows.len();
        let complete = scanned < query.scan_limit;
        let next_cursor = (!complete).then(|| {
            self.cursors.issue(
                Endpoint::Search,
                &filter,
                &(offset + rows.len()).to_string(),
            )
        });
        let items = rows
            .into_iter()
            .map(|(path, indexed_revision, snippet)| SearchHit {
                path,
                indexed_revision,
                snippet,
                retrieved_by: Vec::new(),
            })
            .collect();
        Ok(SearchResult {
            items,
            next_cursor,
            complete,
            scanned,
            building,
            unavailable: Vec::new(),
        })
    }

    /// Hybrid mode (issue #18): run every available retriever over the same
    /// filters and merge with weighted reciprocal rank fusion. Hybrid is a
    /// single fused page — no cursor pagination, because rank depends on the
    /// complete candidate sets of all retrievers.
    async fn search_hybrid(
        &self,
        principal: &Principal,
        query: SearchQuery,
        meta: brains_ports::MetaFilter,
        building: bool,
    ) -> DomainResult<SearchResult> {
        let mut unavailable = Vec::new();
        // Retriever 1: literal (path/title/body substring). Index failures
        // stay loud (503) exactly as in single-mode search.
        let literal = self
            .index
            .scan_literal(
                query.prefix.as_deref(),
                "",
                query.scan_limit,
                &query.q,
                &query.tags,
                &meta,
            )
            .await?;
        let literal_truncated = literal.len() >= query.scan_limit;
        let literal: Vec<Vec<String>> = literal
            .into_iter()
            .filter(|row| row.match_pos > 0)
            .map(|row| vec![row.path, row.revision, row.snippet])
            .collect();
        // Hybrid accepts natural language; only explicit FTS mode accepts
        // the backend's query grammar. Plain words work in FTS5 and Tantivy.
        let fts_query = query
            .q
            .split(|character: char| {
                !character.is_alphanumeric()
                    && !unicode_normalization::char::is_combining_mark(character)
            })
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ");
        let fts = if fts_query.is_empty() {
            Ok(Vec::new())
        } else {
            self.index
                .search_fts(
                    &fts_query,
                    query.prefix.as_deref(),
                    &query.tags,
                    &meta,
                    0,
                    query.scan_limit,
                )
                .await
        };
        let fts_truncated;
        let fts: Vec<Vec<String>> = match fts {
            Ok(rows) => {
                fts_truncated = rows.len() >= query.scan_limit;
                rows.into_iter()
                    .map(|(path, revision, snippet)| vec![path, revision, snippet])
                    .collect()
            }
            Err(DomainError::CapabilityUnsupported(_)) => {
                fts_truncated = false;
                unavailable.push("fts".into());
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        let (semantic, semantic_complete) = match &self.semantic {
            None => {
                unavailable.push("semantic".into());
                (Vec::new(), true)
            }
            Some(_) => match self.semantic_hits(principal, &query, &meta).await {
                Ok(results) => results,
                Err(error) => {
                    unavailable.push(format!("semantic: {}", error.code()));
                    (Vec::new(), false)
                }
            },
        };
        let scanned = literal.len() + fts.len() + semantic.len();
        let mut items = fuse(vec![
            ("literal".to_owned(), literal),
            ("fts".to_owned(), fts),
            ("semantic".to_owned(), semantic),
        ]);
        let output_truncated = self
            .semantic
            .as_ref()
            .is_some_and(|provider| items.len() > provider.ranking().result_limit);
        if let Some(provider) = &self.semantic {
            items.truncate(provider.ranking().result_limit);
        }
        Ok(SearchResult {
            items,
            next_cursor: None,
            complete: !literal_truncated
                && !fts_truncated
                && semantic_complete
                && !output_truncated,
            scanned,
            building,
            unavailable,
        })
    }

    /// Outbound links or backlinks of one path, from the derived index.
    pub async fn links(
        &self,
        principal: &Principal,
        path: &VaultPath,
        backlinks: bool,
        cursor: Option<&str>,
        limit: usize,
    ) -> DomainResult<LinkPageResult> {
        self.require_scope(principal, AccessLevel::Read, path)?;
        let direction = if backlinks { "backlinks" } else { "outbound" };
        let filter = format!("links:{direction}:path={}", path.as_str());
        let after = match cursor.filter(|value| !value.is_empty()) {
            None => None,
            Some(cursor) => Some(self.cursors.verify(Endpoint::Links, &filter, cursor)?),
        };
        let rows = if backlinks {
            self.index
                .links_backlinks(path.as_str(), after.as_deref(), limit)
                .await?
        } else {
            self.index
                .links_outbound(path.as_str(), after.as_deref(), limit)
                .await?
        };
        let exhausted = rows.len() < limit;
        let next_cursor = rows.last().filter(|_| !exhausted).map(|last: &LinkRow| {
            let key = if backlinks {
                &last.source
            } else {
                &last.target
            };
            self.cursors.issue(Endpoint::Links, &filter, key)
        });
        Ok(LinkPageResult { rows, next_cursor })
    }
}

pub struct LinkPageResult {
    pub rows: Vec<LinkRow>,
    pub next_cursor: Option<String>,
}

/// Weighted reciprocal rank fusion (issue #18). Deterministic: equal scores
/// break by path; duplicate hits merge their retriever lists; an empty
/// retriever contributes nothing. Weights favor earlier evidence — literal
/// exactness, then FTS relevance — while `k=60` dampens head-of-list
/// dominance, the standard RRF constant.
pub fn fuse(retrievers: Vec<(String, Vec<Vec<String>>)>) -> Vec<SearchHit> {
    const K: f64 = 60.0;
    let weights = [1.0, 0.9, 0.8];
    struct Fused {
        score: f64,
        revision: String,
        snippet: String,
        retrieved_by: Vec<Retrieval>,
    }
    let mut scores: std::collections::BTreeMap<String, Fused> = std::collections::BTreeMap::new();
    for (index, (name, hits)) in retrievers.iter().enumerate() {
        let weight = weights.get(index).copied().unwrap_or(0.5);
        for (rank, hit) in hits.iter().enumerate() {
            if hit.len() < 3 {
                continue;
            }
            let contribution = weight / (K + (rank + 1) as f64);
            let entry = scores.entry(hit[0].clone()).or_insert_with(|| Fused {
                score: 0.0,
                revision: hit[1].clone(),
                snippet: hit[2].clone(),
                retrieved_by: Vec::new(),
            });
            entry.score += contribution;
            entry.retrieved_by.push(Retrieval {
                retriever: name.clone(),
                rank: rank + 1,
            });
        }
    }
    let mut fused: Vec<(String, Fused)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
    fused
        .into_iter()
        .map(|(path, fused)| SearchHit {
            path,
            indexed_revision: fused.revision,
            snippet: fused.snippet,
            retrieved_by: fused.retrieved_by,
        })
        .collect()
}
