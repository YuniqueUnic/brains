//! Application services (design §8, §17.3): every mutation flows through one
//! commit protocol — validate → immutable blob → immutable revision → atomic
//! HEAD CAS → best-effort index projection. The layer returns domain errors;
//! it never names HTTP statuses.

pub mod commit;
pub mod patch;
pub mod projection;
pub mod query;
pub mod reindex;
pub mod search;
pub mod seed;
pub mod transfer;

use brains_domain::{CursorSigner, DomainError, DomainResult, Limits, Principal, VaultPath};
use brains_ports::{BlobStore, HeadStore, IndexStore, RevisionStore, StoredHead};

/// Outcome of a committed mutation: the committed entry plus index state.
#[derive(Clone, Debug)]
pub struct MutationOutcome {
    pub path: String,
    pub revision: String,
    pub generation: i64,
    pub updated_at: String,
    pub size: i64,
    pub sha256: String,
    pub media_type: String,
    pub kind: brains_domain::FileKind,
    pub created: bool,
    pub index_state: IndexState,
    pub operation_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexState {
    Ready,
    Pending,
}

impl IndexState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Pending => "pending",
        }
    }
}

/// Write preconditions (design §8.1). There is no `Unconditional`.
#[derive(Clone, Debug)]
pub enum Precondition {
    /// `If-None-Match: *` — the path must not exist (tombstones exist).
    Create,
    /// `If-Match: "<revision>"` — act only on exactly this HEAD revision.
    IfRevision(brains_domain::HexId),
}

/// The vault facade binding four stores and instance configuration.
pub struct Vault<H, R, B, I> {
    pub heads: H,
    pub revisions: R,
    pub blobs: B,
    pub index: I,
    pub vault: String,
    pub limits: Limits,
    pub cursors: CursorSigner,
}

// Send + Sync: the axum handler layer requires Send futures; the R2/D1
// bindings satisfy this via worker's SendWrapper (design §9.4 keeps the
// ports themselves free of it).
impl<H, R, B, I> Vault<H, R, B, I>
where
    H: HeadStore + Send + Sync,
    R: RevisionStore + Send + Sync,
    B: BlobStore + Send + Sync,
    I: IndexStore + Send + Sync,
{
    pub fn new(
        heads: H,
        revisions: R,
        blobs: B,
        index: I,
        vault: String,
        limits: Limits,
        cursors: CursorSigner,
    ) -> Self {
        Self {
            heads,
            revisions,
            blobs,
            index,
            vault,
            limits,
            cursors,
        }
    }

    /// Load the current HEAD or report the right "absent" flavor of 404.
    pub(crate) async fn current_head(&self, path: &VaultPath) -> DomainResult<Option<StoredHead>> {
        self.heads.get(path).await
    }

    /// Resolve the current head against a client precondition, returning the
    /// expected CAS token. Fails closed: no precondition, no mutation.
    pub(crate) async fn check_precondition(
        &self,
        path: &VaultPath,
        precondition: &Precondition,
    ) -> DomainResult<Option<StoredHead>> {
        let current = self.current_head(path).await?;
        match precondition {
            Precondition::Create => {
                if current.is_some() {
                    // A tombstone is an existing HEAD: create-only fails 412.
                    return Err(conflict(
                        path,
                        current.as_ref().map(|s| s.head.revision.as_str()),
                    ));
                }
                Ok(None)
            }
            Precondition::IfRevision(rev) => match &current {
                Some(stored) if &stored.head.revision == rev => Ok(current),
                Some(stored) => Err(conflict(path, Some(stored.head.revision.as_str()))),
                None => Err(conflict(path, None)),
            },
        }
    }

    /// Enforce read/write/admin on a logical path with segment-boundary
    /// prefixes (design §15.2).
    pub(crate) fn require_scope(
        &self,
        principal: &Principal,
        level: brains_domain::AccessLevel,
        path: &VaultPath,
    ) -> DomainResult<()> {
        principal.require(level, path.as_str())
    }

    /// Size policy (design §1.3): up to the attachment cap everything is
    /// storable; beyond the text cap content is stored but not indexed.
    /// A path that declares text (by extension) with undecodable bytes is a
    /// fatal pre-CAS error, never a silent demotion to binary.
    pub(crate) fn size_class(&self, path: &VaultPath, content: &[u8]) -> DomainResult<SizeClass> {
        if content.len() > self.limits.max_attachment_bytes {
            return Ok(SizeClass::TooLarge);
        }
        let utf8 = std::str::from_utf8(content).is_ok();
        if brains_domain::declares_text(path.as_str()) {
            if !utf8 {
                return Err(DomainError::InvalidFormat(format!(
                    "{path} declares text but is not valid UTF-8"
                )));
            }
            if content.len() > self.limits.max_text_bytes {
                return Ok(SizeClass::OversizedText);
            }
            return Ok(SizeClass::Text);
        }
        if brains_domain::detect_kind(path.as_str(), content)
            == brains_domain::FileKind::Binary
        {
            Ok(SizeClass::Binary)
        } else {
            Ok(SizeClass::Text)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeClass {
    /// Indexable UTF-8 text; invalid encoding is a 422.
    Text,
    /// Not full-text indexed by nature (attachments, binaries).
    Binary,
    /// Stored as an attachment despite a text-like name; not indexed.
    OversizedText,
    TooLarge,
}

pub(crate) fn conflict(path: &VaultPath, current_revision: Option<&str>) -> DomainError {
    DomainError::RevisionConflict {
        path: path.as_str().to_owned(),
        current_revision: current_revision.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests;
