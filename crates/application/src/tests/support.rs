//! Test seams for business tests: the production stack (fs store + sqlite
//! index) plus injectable failure wrappers for fault-injection scenarios
//! (design §8.4, §17.2: 故障注入是验收的一部分).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::Vault;
use brainstore_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore, FsStore};
use brainstore_adapter_sqlite::SqliteIndex;
use brainstore_domain::{CursorSigner, Limits, Principal, Scope};
use brainstore_ports::{FileProjection, IndexStore, LinkRow, ListFilter, ListRow, LiteralRow};

/// A real vault backed by real adapters, in a temp directory.
pub struct TestVault {
    pub vault: Vault<FsHeadStore, FsRevisionStore, FsBlobStore, SqliteIndex>,
    pub dir: tempfile::TempDir,
}

pub fn owner_principal() -> Principal {
    Principal {
        actor: "token:test-owner".into(),
        scopes: vec![
            Scope::parse("read:/").unwrap(),
            Scope::parse("write:/").unwrap(),
            Scope::parse("admin:/").unwrap(),
        ],
    }
}

pub fn limited_principal(prefix: &str) -> Principal {
    Principal {
        actor: "token:test-limited".into(),
        scopes: vec![
            Scope::parse(&format!("read:{prefix}")).unwrap(),
            Scope::parse(&format!("write:{prefix}")).unwrap(),
        ],
    }
}

pub fn open_vault() -> TestVault {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::create(dir.path(), "default").unwrap();
    let index = SqliteIndex::open(&dir.path().join("index.db")).unwrap();
    let vault = Vault::new(
        FsHeadStore::new(store.clone()),
        FsRevisionStore::new(store.clone()),
        FsBlobStore::new(store),
        index,
        "default".into(),
        Limits::default(),
        CursorSigner::new([9u8; 32]),
    );
    TestVault { vault, dir }
}

/// IndexStore wrapper whose first `fail_after` upserts succeed and every
/// later one fails — modeling "HEAD committed, index write failed" (§8.4).
pub struct FlakyIndex {
    inner: SqliteIndex,
    upserts: AtomicUsize,
    pub fail_after: usize,
    pub broken: Arc<AtomicUsize>,
}

impl FlakyIndex {
    pub fn new(inner: SqliteIndex, fail_after: usize, broken: Arc<AtomicUsize>) -> Self {
        Self {
            inner,
            upserts: AtomicUsize::new(0),
            fail_after,
            broken,
        }
    }
}

impl IndexStore for FlakyIndex {
    async fn upsert(&self, projection: &FileProjection) -> brainstore_domain::DomainResult<()> {
        let count = self.upserts.fetch_add(1, Ordering::SeqCst);
        if count >= self.fail_after {
            self.broken.fetch_add(1, Ordering::SeqCst);
            return Err(brainstore_domain::DomainError::IndexUnavailable(
                "injected index failure".into(),
            ));
        }
        self.inner.upsert(projection).await
    }

    async fn scan_literal(
        &self,
        _lower: &str,
        _upper: Option<&str>,
        _after: &str,
        _limit: usize,
        _query: &str,
        _tag: Option<&str>,
    ) -> brainstore_domain::DomainResult<Vec<LiteralRow>> {
        Err(brainstore_domain::DomainError::IndexUnavailable(
            "index unavailable".into(),
        ))
    }

    async fn search_fts(
        &self,
        _query: &str,
        _lower: &str,
        _upper: Option<&str>,
        _tag: Option<&str>,
        _offset: usize,
        _limit: usize,
    ) -> brainstore_domain::DomainResult<Vec<(String, String, String)>> {
        Err(brainstore_domain::DomainError::IndexUnavailable(
            "index unavailable".into(),
        ))
    }

    async fn list_files(
        &self,
        filter: &ListFilter,
        after_path: Option<&str>,
        after_updated: Option<&str>,
        limit: usize,
    ) -> brainstore_domain::DomainResult<Vec<ListRow>> {
        // Listing falls through to the healthy inner index by design: the
        // degraded-vault tests focus on search/upsert behavior.
        self.inner
            .list_files(filter, after_path, after_updated, limit)
            .await
    }

    async fn links_outbound(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> brainstore_domain::DomainResult<Vec<LinkRow>> {
        self.inner.links_outbound(path, after, limit).await
    }

    async fn links_backlinks(
        &self,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> brainstore_domain::DomainResult<Vec<LinkRow>> {
        self.inner.links_backlinks(path, after, limit).await
    }

    async fn files_count(&self) -> brainstore_domain::DomainResult<u64> {
        self.inner.files_count().await
    }
}

// Convenience: put text content and return the mutation outcome.
pub async fn put_text<VH, VR, VB, VI>(
    vault: &Vault<VH, VR, VB, VI>,
    principal: &Principal,
    path: &str,
    body: &str,
    precondition: crate::Precondition,
) -> brainstore_domain::DomainResult<crate::MutationOutcome>
where
    VH: brainstore_ports::HeadStore + Send + Sync,
    VR: brainstore_ports::RevisionStore + Send + Sync,
    VB: brainstore_ports::BlobStore + Send + Sync,
    VI: brainstore_ports::IndexStore + Send + Sync,
{
    let path = brainstore_domain::VaultPath::new(path).unwrap();
    vault
        .put_file(
            principal,
            crate::commit::CommitInput {
                path: &path,
                content: body.as_bytes(),
                media_type: None,
                actor: principal.actor.clone(),
                operation_id: brainstore_domain::HexId::generate(),
                precondition,
                executable: false,
                copied_from: None,
            },
        )
        .await
}
