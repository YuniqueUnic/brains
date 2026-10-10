//! Shared listing policy and pagination for embedded and HTTP consumers.
use crate::Vault;
use brains_domain::{DomainError, DomainResult, EntryInfo, HeadState, Principal};
use brains_ports::{ListFilter, ListOrder};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ListSource {
    #[default]
    Store,
    Index,
}
impl std::fmt::Display for ListSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Store => "store",
            Self::Index => "index",
        })
    }
}
impl std::str::FromStr for ListSource {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "store" => Ok(Self::Store),
            "index" => Ok(Self::Index),
            _ => Err(DomainError::InvalidArgument(
                "source must be `store` or `index`".into(),
            )),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ListQuery {
    pub source: ListSource,
    pub prefix: Option<String>,
    pub limit: usize,
    pub cursor: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<brains_domain::FileKind>,
    pub sort: ListOrder,
    pub include_deleted: bool,
}
impl Default for ListQuery {
    fn default() -> Self {
        Self {
            source: ListSource::Store,
            prefix: None,
            limit: brains_domain::Limits::default().list_page_default,
            cursor: None,
            tag: None,
            kind: None,
            sort: ListOrder::Path,
            include_deleted: false,
        }
    }
}
pub struct ListPage {
    pub items: Vec<EntryInfo>,
    pub next_cursor: Option<String>,
    pub source: ListSource,
}
impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    pub async fn list_files(
        &self,
        principal: &Principal,
        query: ListQuery,
    ) -> DomainResult<ListPage> {
        let ListQuery {
            source,
            prefix,
            limit,
            cursor,
            tag,
            kind,
            sort,
            include_deleted,
        } = query;
        let prefix = prefix.filter(|value| !value.is_empty());
        let tag = tag.filter(|value| !value.is_empty());
        let cursor = cursor.filter(|value| !value.is_empty());
        if limit == 0 || limit > self.limits.list_page_max {
            return Err(DomainError::InvalidArgument(format!(
                "limit must be between 1 and {}",
                self.limits.list_page_max
            )));
        }
        if source == ListSource::Store {
            if tag.is_some() || kind.is_some() || sort == ListOrder::UpdatedDesc {
                return Err(DomainError::InvalidArgument(
                    "tag/kind/sort=updated_desc require source=index".into(),
                ));
            }
            let page = self
                .list_from_store(
                    principal,
                    prefix.as_deref(),
                    cursor.as_deref(),
                    include_deleted,
                    limit,
                )
                .await?;
            return Ok(ListPage {
                items: page.items,
                next_cursor: page.next_cursor,
                source,
            });
        }
        let (after_path, after_updated) = match (cursor.as_deref(), sort) {
            (None, _) => (None, None),
            (Some(raw), ListOrder::UpdatedDesc) => {
                let (updated, path) = raw
                    .split_once('|')
                    .ok_or_else(|| DomainError::InvalidArgument("malformed cursor".into()))?;
                (Some(path.to_owned()), Some(updated.to_owned()))
            }
            (Some(raw), ListOrder::Path) => (Some(raw.to_owned()), None),
        };
        let filter = ListFilter {
            prefix,
            tag,
            kind,
            include_deleted,
            order: sort,
        };
        let page = self
            .list_from_index(principal, filter, after_path, after_updated, limit)
            .await?;
        let next_cursor = if page.exhausted {
            None
        } else {
            page.rows.last().map(|row| match sort {
                ListOrder::UpdatedDesc => format!("{}|{}", row.updated_at, row.path),
                ListOrder::Path => row.path.clone(),
            })
        };
        let items = page
            .rows
            .into_iter()
            .map(|row| EntryInfo {
                path: row.path,
                revision: row.revision,
                generation: row.generation,
                state: if row.deleted {
                    HeadState::Trashed
                } else {
                    HeadState::Live
                },
                updated_at: row.updated_at,
                size: Some(row.size),
                sha256: row.sha256,
                media_type: Some(row.media_type),
                kind: Some(row.kind),
                title: if row.title.is_empty() {
                    None
                } else {
                    Some(row.title)
                },
            })
            .collect();
        Ok(ListPage {
            items,
            next_cursor,
            source,
        })
    }
}
