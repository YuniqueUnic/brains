//! File-level view shared by listings, metadata reads and mutation results.

use crate::head::HeadState;
use crate::kind::FileKind;

#[derive(Clone, Debug)]
pub struct EntryInfo {
    pub path: String,
    pub revision: String,
    pub generation: i64,
    pub state: HeadState,
    pub updated_at: String,
    pub size: Option<i64>,
    pub sha256: Option<String>,
    pub media_type: Option<String>,
    pub kind: Option<FileKind>,
    pub title: Option<String>,
}

impl EntryInfo {
    /// A head-only entry without content-derived fields (e.g. tombstones).
    pub fn from_head_parts(
        path: String,
        revision: String,
        generation: i64,
        state: HeadState,
        updated_at: String,
    ) -> Self {
        Self {
            path,
            revision,
            generation,
            state,
            updated_at,
            size: None,
            sha256: None,
            media_type: None,
            kind: None,
            title: None,
        }
    }
}
