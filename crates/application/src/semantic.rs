//! Authoritative validation happens after candidate retrieval and before output.

use std::collections::BTreeSet;

use brains_domain::{AccessLevel, DomainResult, HeadState, HexId, Principal, VaultPath};
use brains_ports::{FileProjection, MetaFilter};

use crate::{Vault, search::SearchQuery};

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    pub(crate) async fn semantic_hits(
        &self,
        principal: &Principal,
        query: &SearchQuery,
        meta: &MetaFilter,
    ) -> DomainResult<(Vec<Vec<String>>, bool)> {
        let Some(provider) = &self.semantic else {
            return Ok((Vec::new(), true));
        };
        let candidates = provider.query(&query.q).await?;
        let total = candidates.items.len();
        let provider_complete = candidates.complete;
        let mut checked_count = 0;
        let mut seen = BTreeSet::new();
        let mut hits = Vec::new();
        for candidate in candidates
            .items
            .into_iter()
            .take(provider.ranking().validation_limit)
        {
            checked_count += 1;
            let revision_id = HexId::from_hex(&candidate.revision)?;
            let Some(revision) = self.revisions.get(&revision_id).await? else {
                continue;
            };
            let path = VaultPath::new(&revision.path)?;
            if !principal.allows(AccessLevel::Read, path.as_str())
                || !prefix_matches(path.as_str(), query.prefix.as_deref())
            {
                continue;
            }
            let Some(head) = self.heads.get(&path).await? else {
                continue;
            };
            if head.head.state != HeadState::Live || head.head.revision != revision_id {
                continue;
            }
            let Some(content) = &revision.content else {
                continue;
            };
            let bytes = self.blobs.get(&content.key).await?;
            let projection = crate::projection::build_projection(
                &self.vault,
                &revision,
                Some(&bytes),
                false,
                self.limits.max_text_bytes,
            );
            if !matches_filters(&projection, &query.tags, meta)
                || !seen.insert(projection.path.clone())
            {
                continue;
            }
            hits.push(vec![
                projection.path,
                projection.revision,
                projection.body.chars().take(320).collect(),
            ]);
            if hits.len() >= provider.ranking().result_limit {
                break;
            }
        }
        Ok((hits, provider_complete && checked_count == total))
    }
}

fn prefix_matches(path: &str, prefix: Option<&str>) -> bool {
    prefix.is_none_or(|prefix| {
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn matches_filters(projection: &FileProjection, tags: &[String], filter: &MetaFilter) -> bool {
    let actual_tags: Vec<String> = serde_json::from_str(&projection.tags_json).unwrap_or_default();
    if !tags.iter().all(|tag| actual_tags.contains(tag)) {
        return false;
    }
    let meta: serde_json::Value = serde_json::from_str(&projection.meta_json).unwrap_or_default();
    for (key, expected) in [
        ("status", &filter.status),
        ("type", &filter.type_),
        ("trust", &filter.trust),
    ] {
        if expected
            .as_deref()
            .is_some_and(|value| meta.get(key).and_then(serde_json::Value::as_str) != Some(value))
        {
            return false;
        }
    }
    if let Some(expected) = filter.stale {
        let stale = meta
            .get("stale_after")
            .and_then(serde_json::Value::as_str)
            .zip(filter.today.as_deref())
            .is_some_and(|(date, today)| date <= today);
        if stale != expected {
            return false;
        }
    }
    !projection.deleted && !projection.body.is_empty()
}
