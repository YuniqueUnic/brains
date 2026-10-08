//! Paged index rebuild (design §10.5): one bounded page per call, driven
//! explicitly by the CLI/admin — never a promise of background completion.

use brainstore_domain::{AccessLevel, DomainError, DomainResult, Endpoint, HexId, Principal};

use crate::Vault;
use crate::projection;

#[derive(Debug)]
pub struct ReindexFailure {
    pub path: String,
    pub code: String,
}

pub struct ReindexPageResult {
    pub processed: usize,
    pub failed: Vec<ReindexFailure>,
    pub next_cursor: Option<String>,
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brainstore_ports::HeadStore + Send + Sync,
    R: brainstore_ports::RevisionStore + Send + Sync,
    B: brainstore_ports::BlobStore + Send + Sync,
    I: brainstore_ports::IndexStore + Send + Sync,
{
    /// Reindex one page of heads after `cursor`. The monotonic upsert makes
    /// this safe to run against live writes and to re-run per page.
    pub async fn reindex_page(
        &self,
        principal: &Principal,
        cursor: Option<&str>,
        limit: usize,
    ) -> DomainResult<ReindexPageResult> {
        // Rebuild touches every path; only a whole-vault admin may drive it.
        if !principal.allows(AccessLevel::Admin, "") {
            return Err(DomainError::Forbidden(
                "reindex requires whole-vault admin scope".into(),
            ));
        }
        let after = match cursor {
            None => None,
            Some(cursor) => {
                let position = self.cursors.verify(Endpoint::Reindex, "reindex", cursor)?;
                Some(position)
            }
        };
        let heads = self.heads.list(None, after.as_deref(), limit).await?;
        let mut failed = Vec::new();
        let mut last_path: Option<String> = None;
        for stored in &heads {
            last_path = Some(stored.head.path.clone());
            match self.reindex_one(&stored.head.revision).await {
                Ok(()) => {}
                Err(e) => failed.push(ReindexFailure {
                    path: stored.head.path.clone(),
                    code: e.code().to_owned(),
                }),
            }
        }
        let exhausted = heads.len() < limit;
        let next_cursor = last_path
            .filter(|_| !exhausted)
            .map(|path| self.cursors.issue(Endpoint::Reindex, "reindex", &path));
        Ok(ReindexPageResult {
            processed: heads.len() - failed.len(),
            failed,
            next_cursor,
        })
    }

    async fn reindex_one(&self, revision_id: &HexId) -> DomainResult<()> {
        let revision =
            self.revisions
                .get(revision_id)
                .await?
                .ok_or_else(|| DomainError::NotFound {
                    path: format!("revision {revision_id}"),
                })?;
        let content = match &revision.content {
            Some(content) => Some(self.blobs.get(&content.key).await?),
            None => None,
        };
        let deleted = revision.action == brainstore_domain::RevisionAction::Trash;
        let proj = projection::build_projection(
            &self.vault,
            &revision,
            content.as_deref(),
            deleted,
            self.limits.max_text_bytes,
        );
        self.index.upsert(&proj).await
    }
}
