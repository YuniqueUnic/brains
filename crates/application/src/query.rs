//! Read-side services: raw/meta reads, reachable history, revision access,
//! authoritative and index-backed listings.

use brains_domain::{
    AccessLevel, DomainError, DomainResult, Endpoint, Head, HeadState, HexId, Principal, Revision,
    VaultPath,
};
use brains_ports::ListFilter;

use crate::Vault;

pub struct FileContent {
    pub head: Head,
    pub revision: Revision,
    pub bytes: Vec<u8>,
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    /// Read current live bytes. Trashed paths answer 410; absent 404.
    pub async fn read_file(
        &self,
        principal: &Principal,
        path: &VaultPath,
    ) -> DomainResult<FileContent> {
        self.require_scope(principal, AccessLevel::Read, path)?;
        let stored = self
            .current_head(path)
            .await?
            .ok_or_else(|| DomainError::NotFound {
                path: path.as_str().to_owned(),
            })?;
        if stored.head.state == HeadState::Trashed {
            return Err(DomainError::Trashed {
                path: path.as_str().to_owned(),
                revision: stored.head.revision.as_str().to_owned(),
            });
        }
        let revision = self.load_revision(&stored.head.revision).await?;
        let content = revision
            .content
            .clone()
            .ok_or_else(|| DomainError::Internal("live HEAD points at a trash revision".into()))?;
        let bytes = self.blobs.get(&content.key).await?;
        Ok(FileContent {
            head: stored.head,
            revision,
            bytes,
        })
    }

    /// Metadata for one path (head + content facts). Tombstones answer the
    /// same way a raw GET would: 410 with the current revision attached.
    pub async fn read_entry(
        &self,
        principal: &Principal,
        path: &VaultPath,
    ) -> DomainResult<brains_domain::EntryInfo> {
        self.require_scope(principal, AccessLevel::Read, path)?;
        let stored = self
            .current_head(path)
            .await?
            .ok_or_else(|| DomainError::NotFound {
                path: path.as_str().to_owned(),
            })?;
        let head = stored.head;
        if head.state == HeadState::Trashed {
            return Err(DomainError::Trashed {
                path: path.as_str().to_owned(),
                revision: head.revision.as_str().to_owned(),
            });
        }
        let revision = self.load_revision(&head.revision).await?;
        let content = revision
            .content
            .clone()
            .ok_or_else(|| DomainError::Internal("live HEAD points at a trash revision".into()))?;
        let bytes = self.blobs.get(&content.key).await?;
        let kind = brains_domain::detect_kind(path.as_str(), &bytes);
        let title = if kind == brains_domain::FileKind::Binary {
            None
        } else {
            std::str::from_utf8(&bytes)
                .ok()
                .map(|text| brains_format::parse_document(path, text))
                .and_then(|doc| doc.ok())
                .and_then(|doc| doc.title)
        };
        Ok(brains_domain::EntryInfo {
            path: head.path.clone(),
            revision: head.revision.as_str().to_owned(),
            generation: head.generation,
            state: head.state,
            updated_at: head.updated_at.clone(),
            size: Some(content.size),
            sha256: Some(content.sha256.clone()),
            media_type: Some(content.media_type.clone()),
            kind: Some(kind),
            title,
        })
    }

    /// One page of the parent chain reachable from the CURRENT head. The
    /// anchor is pinned into the cursor so later pages walk the same chain
    /// even if HEAD moves meanwhile (design §11.1).
    pub async fn history_page(
        &self,
        principal: &Principal,
        path: &VaultPath,
        cursor: Option<&str>,
        limit: usize,
    ) -> DomainResult<HistoryPageResult> {
        self.require_scope(principal, AccessLevel::Read, path)?;
        let (anchor, next_parent) = match cursor.filter(|value| !value.is_empty()) {
            None => {
                let stored =
                    self.current_head(path)
                        .await?
                        .ok_or_else(|| DomainError::NotFound {
                            path: path.as_str().to_owned(),
                        })?;
                (stored.head.revision.clone(), stored.head.revision.clone())
            }
            Some(cursor) => {
                let filter = history_filter(path);
                let position = self.cursors.verify(Endpoint::Revisions, &filter, cursor)?;
                let (anchor_raw, parent_raw) = position
                    .split_once(':')
                    .ok_or_else(|| DomainError::InvalidArgument("malformed cursor".into()))?;
                (HexId::from_hex(anchor_raw)?, HexId::from_hex(parent_raw)?)
            }
        };
        // The anchor must still be (or have been) the head of THIS path.
        // Walk the pinned chain; the caller sees a coherent history even if
        // a concurrent write moved HEAD.
        let mut items = Vec::with_capacity(limit);
        let mut walker = next_parent;
        let mut next_cursor = None;
        while items.len() < limit {
            let Some(revision) = self.revisions.get(&walker).await? else {
                return Err(DomainError::StorageUnavailable(format!(
                    "history chain broken at {walker}"
                )));
            };
            if revision.path != path.as_str() {
                return Err(DomainError::StorageUnavailable(
                    "history chain crosses paths".into(),
                ));
            }
            let parent = revision.parent.clone();
            items.push(revision);
            match parent {
                Some(parent) => walker = parent,
                None => break,
            }
        }
        if items.len() == limit
            && let Some(last) = items.last()
            && last.parent.is_some()
        {
            let position = format!(
                "{}:{}",
                anchor.as_str(),
                last.parent.as_ref().expect("checked").as_str()
            );
            next_cursor = Some(self.cursors.issue(
                Endpoint::Revisions,
                &history_filter(path),
                &position,
            ));
        }
        Ok(HistoryPageResult {
            items,
            anchor_revision: anchor.as_str().to_owned(),
            next_cursor,
        })
    }

    /// Read one immutable revision by ID. The record's path must equal the
    /// authorized path (design §11.1); reachability is NOT verified here —
    /// history listing is the authority for committed chains.
    pub async fn read_revision(
        &self,
        principal: &Principal,
        path: &VaultPath,
        revision: &HexId,
    ) -> DomainResult<Revision> {
        self.require_scope(principal, AccessLevel::Read, path)?;
        let record = self.load_revision(revision).await?;
        if record.path != path.as_str() {
            return Err(DomainError::NotFound {
                path: path.as_str().to_owned(),
            });
        }
        Ok(record)
    }

    pub async fn load_revision(&self, revision: &HexId) -> DomainResult<Revision> {
        self.revisions
            .get(revision)
            .await?
            .ok_or_else(|| DomainError::NotFound {
                path: format!("revision {revision}"),
            })
    }

    /// Content bytes of a revision (for `GET /revision?format=raw`, diff).
    pub async fn revision_bytes(&self, revision: &Revision) -> DomainResult<Vec<u8>> {
        let content = revision.content.clone().ok_or_else(|| {
            DomainError::InvalidArgument("tombstone revisions have no content".into())
        })?;
        self.blobs.get(&content.key).await
    }

    /// Authoritative listing (source=store): heads only, no index involved.
    /// Index-only filters are rejected upstream (design §11.1). The page
    /// cursor is a signed, filter-bound position — same integrity rules as
    /// history pagination, never a client-supplied raw path.
    pub async fn list_from_store(
        &self,
        principal: &Principal,
        prefix: Option<&str>,
        cursor: Option<&str>,
        include_deleted: bool,
        limit: usize,
    ) -> DomainResult<StoreListResult> {
        self.require_list_prefix(principal, prefix)?;
        // A trailing slash is display sugar (`brain ls ord/`): the segment-
        // boundary filters downstream expect the bare prefix (design §6.2).
        let prefix = prefix.map(|p| p.strip_suffix('/').unwrap_or(p));
        let filter = store_list_filter(prefix, include_deleted);
        let after = match cursor.filter(|c| !c.is_empty()) {
            None => None,
            Some(cursor) => Some(self.cursors.verify(Endpoint::Files, &filter, cursor)?),
        };
        let heads = self.heads.list(prefix, after.as_deref(), limit).await?;
        // Exhaustion and the cursor anchor are computed on the RAW head scan:
        // tombstone-free pages must not end pagination early, and a page of
        // pure tombstones must still advance the cursor (design §11.1).
        let scanned = heads.len();
        let anchor = heads
            .last()
            .map(|stored| stored.head.path.as_str().to_owned());
        let items: Vec<brains_domain::EntryInfo> = heads
            .into_iter()
            .filter(|stored| include_deleted || stored.head.is_live())
            .map(|stored| {
                brains_domain::EntryInfo::from_head_parts(
                    stored.head.path.clone(),
                    stored.head.revision.as_str().to_owned(),
                    stored.head.generation,
                    stored.head.state,
                    stored.head.updated_at.clone(),
                )
            })
            .collect();
        let exhausted = scanned < limit;
        let next_cursor = match (exhausted, anchor) {
            (false, Some(anchor)) => Some(self.cursors.issue(Endpoint::Files, &filter, &anchor)),
            _ => None,
        };
        Ok(StoreListResult {
            items,
            exhausted,
            next_cursor,
        })
    }

    /// Index-backed listing with tag/kind filters and updated_desc order.
    pub async fn list_from_index(
        &self,
        principal: &Principal,
        filter: ListFilter,
        after_path: Option<String>,
        after_updated: Option<String>,
        limit: usize,
    ) -> DomainResult<IndexListResult> {
        self.require_list_prefix(principal, filter.prefix.as_deref())?;
        let rows = self
            .index
            .list_files(
                &filter,
                after_path.as_deref(),
                after_updated.as_deref(),
                limit,
            )
            .await?;
        let exhausted = rows.len() < limit;
        Ok(IndexListResult {
            rows,
            exhausted,
            next_after_path: after_path,
            next_after_updated: after_updated,
        })
    }

    /// A listing prefix must sit inside exactly one granted read scope —
    /// results are never silently intersected (design §15.2).
    fn require_list_prefix(&self, principal: &Principal, prefix: Option<&str>) -> DomainResult<()> {
        match prefix {
            Some(prefix) => principal.require(AccessLevel::Read, prefix),
            None => {
                if principal.allows(AccessLevel::Read, "") {
                    Ok(())
                } else {
                    Err(DomainError::Forbidden(
                        "token scopes do not cover the whole vault; pass an explicit prefix".into(),
                    ))
                }
            }
        }
    }
}

fn history_filter(path: &VaultPath) -> String {
    format!("path={}", path.as_str())
}

/// Fingerprint of everything in a store listing that changes what a cursor
/// anchor means; part of the signed cursor payload (design §11.1).
fn store_list_filter(prefix: Option<&str>, include_deleted: bool) -> String {
    format!(
        "prefix={};deleted={}",
        prefix.unwrap_or(""),
        include_deleted
    )
}

pub struct HistoryPageResult {
    pub items: Vec<Revision>,
    pub anchor_revision: String,
    pub next_cursor: Option<String>,
}

pub struct StoreListResult {
    pub items: Vec<brains_domain::EntryInfo>,
    pub exhausted: bool,
    /// Signed cursor for the next page (bound to this listing's filters);
    /// `None` once exhausted. Opaque to clients.
    pub next_cursor: Option<String>,
}

pub struct IndexListResult {
    pub rows: Vec<brains_ports::ListRow>,
    pub exhausted: bool,
    pub next_after_path: Option<String>,
    pub next_after_updated: Option<String>,
}
