//! Transport DTOs mirroring contracts/openapi.yaml (design §11, §17.4).
//! snake_case JSON throughout; these types are the HTTP surface — the
//! application layer speaks domain types and never sees these shapes.

use serde::{Deserialize, Serialize};

use brains_domain::{EntryInfo, FileKind};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ErrorBody {
    pub fn new(detail: ErrorDetail) -> Self {
        Self { error: detail }
    }
}

// ---------------------------------------------------------------------------
// Entries and listings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub path: String,
    pub revision: String,
    pub generation: i64,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<FileKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Lifecycle/trust projection (issue #13); absent when the serving path
    /// has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<NoteMeta>,
}

/// The lifecycle/trust wire shape (mirrors `api/brains.idl`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NoteMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub trust: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated: Option<Generated>,
    #[serde(default)]
    pub verified: Vec<Verification>,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Generated {
    pub by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Verification {
    pub by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceRef {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

impl NoteMeta {
    /// Decode the index's `meta_json` projection. Empty or unparseable
    /// projections yield `None`: reads never fail because a projection is
    /// behind. Shared by the HTTP service and the embedded client.
    pub fn from_projection(meta_json: Option<&str>) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(meta_json?).ok()?;
        if value.as_object().is_none_or(|map| map.is_empty()) {
            return None;
        }
        let string_at = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let actors = |key: &str| -> Vec<Verification> {
            value
                .get(key)
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .map(|item| Verification {
                            by: item
                                .get("by")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_owned(),
                            at: item.get("at").and_then(|v| v.as_str()).map(str::to_owned),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Self {
            r#type: string_at("type"),
            description: string_at("description"),
            status: string_at("status").unwrap_or_else(|| "stable".to_owned()),
            trust: string_at("trust").unwrap_or_else(|| "unverified".to_owned()),
            stale_after: string_at("stale_after"),
            generated: value.get("generated").map(|generated| Generated {
                by: generated
                    .get("by")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                at: generated
                    .get("at")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            }),
            verified: actors("verified"),
            sources: value
                .get("sources")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            let text = |key: &str| {
                                item.get(key)
                                    .and_then(|v| v.as_str())
                                    .filter(|text| !text.is_empty())
                                    .map(str::to_owned)
                            };
                            SourceRef {
                                id: item
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_owned(),
                                resource: text("resource"),
                                title: text("title"),
                                last_modified: text("last_modified"),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

impl Entry {
    pub fn from_info(info: &EntryInfo) -> Self {
        Self {
            path: info.path.clone(),
            revision: info.revision.clone(),
            generation: info.generation,
            state: info.state.as_str().to_owned(),
            updated_at: Some(info.updated_at.clone()),
            size: info.size,
            sha256: info.sha256.clone(),
            media_type: info.media_type.clone(),
            kind: info.kind,
            title: info.title.clone(),
            meta: NoteMeta::from_projection(info.meta_json.as_deref()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationResult {
    #[serde(flatten)]
    pub entry: Entry,
    pub index_state: String,
    pub operation_id: String,
}

/// Permanent delete of a tombstone: the path is gone from every listing and
/// its history chain is unreachable. No Entry — there is no head anymore.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PurgeResult {
    pub path: String,
    pub purged: bool,
    pub index_state: String,
    pub operation_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilePage {
    pub items: Vec<Entry>,
    pub next_cursor: Option<String>,
    pub source: String,
    pub consistency: String, // always "per_file"
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevisionDto {
    pub revision: String,
    pub path: String,
    pub generation: i64,
    pub parent: Option<String>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restores_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copied_from: Option<CopiedFromDto>,
    /// `reachable` inside history pages; `unknown` on direct reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentRefDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopiedFromDto {
    pub path: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentRefDto {
    pub size: i64,
    pub sha256: String,
    pub media_type: String,
    pub executable: bool,
}

impl RevisionDto {
    pub fn from_revision(revision: &brains_domain::Revision, commit_state: &str) -> Self {
        Self {
            revision: revision.revision.as_str().to_owned(),
            path: revision.path.clone(),
            generation: revision.generation,
            parent: revision.parent.as_ref().map(|p| p.as_str().to_owned()),
            action: revision.action.as_str().to_owned(),
            actor: Some(revision.actor.clone()),
            operation_id: Some(revision.operation_id.as_str().to_owned()),
            created_at: revision.created_at.clone(),
            restores_revision: revision
                .restores_revision
                .as_ref()
                .map(|r| r.as_str().to_owned()),
            copied_from: revision.copied_from.as_ref().map(|c| CopiedFromDto {
                path: c.path.clone(),
                revision: c.revision.as_str().to_owned(),
            }),
            commit_state: Some(commit_state.to_owned()),
            content: revision.content.as_ref().map(|c| ContentRefDto {
                size: c.size,
                sha256: c.sha256.clone(),
                media_type: c.media_type.clone(),
                executable: c.executable,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryPage {
    pub items: Vec<RevisionDto>,
    pub anchor_revision: String,
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Search and links
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub indexed_revision: String,
    pub snippet: String,
    /// Hybrid mode only: which retrievers matched, with the rank in each.
    #[serde(default)]
    pub retrieved_by: Vec<Retrieval>,
}

/// One retriever's view of a hybrid hit (issue #18).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Retrieval {
    pub retriever: String,
    pub rank: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub complete: bool,
    pub scanned: i64,
    pub index_state: String, // available | building
    /// Hybrid mode only: retrievers that did not contribute and why.
    #[serde(default)]
    pub unavailable: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkItem {
    pub source: String,
    pub target: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkPage {
    pub items: Vec<LinkItem>,
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Maintenance
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReindexRequest {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReindexFailure {
    pub path: String,
    pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReindexPage {
    pub processed: i64,
    pub failed: Vec<ReindexFailure>,
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub actor: String,
    pub scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OkResult {
    pub ok: bool,
}

// ---------------------------------------------------------------------------
// P1 bodies
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatchEditDto {
    pub old_text: String,
    pub new_text: String,
    pub expected_matches: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatchBody {
    pub edits: Vec<PatchEditDto>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatchResult {
    pub diff: String,
    pub base_revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<MutationResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoreBody {
    pub path: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferBody {
    pub source: String,
    pub target: String,
    pub expected_source_revision: String,
    pub operation_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferResult {
    pub copied: bool,
    pub source_trashed: bool,
    pub partial: bool,
    pub operation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetail>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiffResult {
    pub diff: String,
}
