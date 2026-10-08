//! Bounded search (design §10.3): literal windows by default, explicit FTS
//! optionally. Zero hits plus a pending cursor never means "whole vault has
//! no results"; index failures are loud 503s, never fake-empty pages.

use brainstore_domain::{AccessLevel, DomainError, DomainResult, Endpoint, Principal, VaultPath};
use brainstore_ports::{LinkRow, prefix_upper_bound};

use crate::Vault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Literal,
    Fts,
}

pub struct SearchQuery {
    pub q: String,
    pub mode: SearchMode,
    pub prefix: Option<String>,
    pub tag: Option<String>,
    pub cursor: Option<String>,
    pub scan_limit: usize,
}

pub struct SearchHit {
    pub path: String,
    pub indexed_revision: String,
    pub snippet: String,
}

pub struct SearchResult {
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub complete: bool,
    pub scanned: usize,
    pub building: bool,
}

fn literal_filter(query: &SearchQuery) -> String {
    format!(
        "literal:q={}:prefix={}:tag={}",
        query.q,
        query.prefix.as_deref().unwrap_or(""),
        query.tag.as_deref().unwrap_or("")
    )
}

/// FTS cursors bind the same conditions literal ones do: reusing a cursor
/// under a different q/prefix/tag must fail instead of mixing pages.
fn fts_filter(query: &SearchQuery) -> String {
    format!(
        "fts:q={}:prefix={}:tag={}",
        query.q,
        query.prefix.as_deref().unwrap_or(""),
        query.tag.as_deref().unwrap_or("")
    )
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brainstore_ports::HeadStore + Send + Sync,
    R: brainstore_ports::RevisionStore + Send + Sync,
    B: brainstore_ports::BlobStore + Send + Sync,
    I: brainstore_ports::IndexStore + Send + Sync,
{
    pub async fn search(
        &self,
        principal: &Principal,
        query: SearchQuery,
    ) -> DomainResult<SearchResult> {
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
        let building = self.index.files_count().await? == 0;
        match query.mode {
            SearchMode::Literal => self.search_literal(query, building).await,
            SearchMode::Fts => self.search_fts(query, building).await,
        }
    }

    async fn search_literal(
        &self,
        query: SearchQuery,
        building: bool,
    ) -> DomainResult<SearchResult> {
        let lower = query.prefix.clone().unwrap_or_default();
        let filter = literal_filter(&query);
        let after = match &query.cursor {
            None => String::new(),
            Some(cursor) => self.cursors.verify(Endpoint::Search, &filter, cursor)?,
        };
        let upper = query.prefix.as_deref().map(prefix_upper_bound);
        let rows = self
            .index
            .scan_literal(
                &lower,
                upper.as_deref(),
                &after,
                query.scan_limit,
                &query.q,
                query.tag.as_deref(),
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
            })
            .collect();
        Ok(SearchResult {
            items,
            next_cursor,
            complete,
            scanned,
            building,
        })
    }

    async fn search_fts(&self, query: SearchQuery, building: bool) -> DomainResult<SearchResult> {
        let filter = fts_filter(&query);
        let offset = match &query.cursor {
            None => 0usize,
            Some(cursor) => self
                .cursors
                .verify(Endpoint::Search, &filter, cursor)?
                .parse::<usize>()
                .map_err(|_| DomainError::InvalidArgument("malformed cursor".into()))?,
        };
        let lower = query.prefix.clone().unwrap_or_default();
        let upper = query.prefix.as_deref().map(prefix_upper_bound);
        let rows = self
            .index
            .search_fts(
                &query.q,
                &lower,
                upper.as_deref(),
                query.tag.as_deref(),
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
            })
            .collect();
        Ok(SearchResult {
            items,
            next_cursor,
            complete,
            scanned,
            building,
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
        let after = match cursor {
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
