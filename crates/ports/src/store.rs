//! Content-store ports: immutable blobs, immutable revisions, CAS heads.
//!
//! Methods return `impl Future + Send` (not `async fn`): the axum handler
//! layer requires Send futures, and a plain `async fn` in trait fixes the
//! future's auto-traits at the trait level — `Send` there would be
//! unprovable for generic adapters. Adapters whose runtime futures are
//! `!Send` (WASM bindings) wrap them in `worker::send::SendFuture`, the
//! officially sanctioned bridge for single-threaded Workers (design §9.4).

use std::future::Future;

use brains_domain::{DomainResult, Head, HexId, Revision, VaultPath};

/// Condition for a HEAD compare-and-swap. `StorageToken` is the adapter's
/// internal token observed when the current HEAD was read (an R2 object ETag
/// in the cloud adapter; the HEAD revision string in the filesystem adapter).
/// It is never the public API revision ETag — that stays `revision`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpectedHead {
    Absent,
    StorageToken(String),
}

/// Only two CAS results exist. Network/storage failures are `Err`, never a
/// misreported `Committed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CasOutcome {
    Committed,
    PreconditionFailed,
}

/// HEAD as persisted, plus the adapter token a CAS will be conditioned on.
#[derive(Clone, Debug)]
pub struct StoredHead {
    pub head: Head,
    pub storage_token: String,
}

impl StoredHead {
    pub fn expected(&self) -> ExpectedHead {
        ExpectedHead::StorageToken(self.storage_token.clone())
    }
}

pub trait HeadStore {
    fn get(
        &self,
        path: &VaultPath,
    ) -> impl Future<Output = DomainResult<Option<StoredHead>>> + Send;

    /// The single atomic commit point. `Committed` means the new HEAD is the
    /// authoritative state for this path.
    fn cas(
        &self,
        path: &VaultPath,
        expected: &ExpectedHead,
        new_head: &Head,
    ) -> impl Future<Output = DomainResult<CasOutcome>> + Send;

    /// Heads ordered by logical path, strictly after `after` (exclusive),
    /// under `prefix` if given. Returns at most `limit` records; the caller
    /// advances pagination by the last returned path.
    fn list(
        &self,
        prefix: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<StoredHead>>> + Send;
}

pub trait RevisionStore {
    /// Create-only: a collision on the revision ID is a hard error, never an
    /// overwrite followed by a report.
    fn create(&self, revision: &Revision) -> impl Future<Output = DomainResult<()>> + Send;
    fn get(&self, revision: &HexId) -> impl Future<Output = DomainResult<Option<Revision>>> + Send;
}

pub trait BlobStore {
    /// Create-only immutable write; `sha256_hex` is verified against the
    /// actual bytes before the blob becomes visible under `key`.
    fn create(
        &self,
        key: &str,
        content: &[u8],
        sha256_hex: &str,
    ) -> impl Future<Output = DomainResult<()>> + Send;
    fn get(&self, key: &str) -> impl Future<Output = DomainResult<Vec<u8>>> + Send;
}
