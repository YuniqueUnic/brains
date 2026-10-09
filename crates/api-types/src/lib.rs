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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub complete: bool,
    pub scanned: i64,
    pub index_state: String, // available | building
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
pub struct PatchDryRun {
    pub diff: String,
    pub base_revision: String,
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
