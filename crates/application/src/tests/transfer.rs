//! Copy/move semantics (design §8.7, U15): copy keeps source history;
//! move trashes an unchanged source; a source that changed mid-move keeps
//! both files and reports partial success.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::Precondition;
use crate::Vault;
use crate::transfer::TransferInput;
use brainstore_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore, FsStore};
use brainstore_adapter_sqlite::SqliteIndex;
use brainstore_domain::{CursorSigner, DomainError, HexId, Limits, VaultPath};
use brainstore_ports::{CasOutcome, ExpectedHead};

use super::support::{TestVault, open_vault, owner_principal, put_text};

#[tokio::test]
async fn copy_records_provenance_and_keeps_source() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let source = put_text(
        &vault,
        &owner,
        "src.md",
        "shared bytes",
        Precondition::Create,
    )
    .await
    .unwrap();

    let outcome = vault
        .copy_file(
            &owner,
            TransferInput {
                source: VaultPath::new("src.md").unwrap(),
                target: VaultPath::new("dst/copy.md").unwrap(),
                expected_source_revision: HexId::from_hex(&source.revision).unwrap(),
                actor: owner.actor.clone(),
                operation_id: HexId::generate(),
            },
        )
        .await
        .unwrap();
    assert!(outcome.copied && !outcome.partial);

    // The target's history is its own: one copy revision with provenance.
    let history = vault
        .history_page(&owner, &VaultPath::new("dst/copy.md").unwrap(), None, 10)
        .await
        .unwrap();
    assert_eq!(history.items.len(), 1);
    assert_eq!(
        history.items[0].action,
        brainstore_domain::RevisionAction::Copy
    );
    assert_eq!(
        history.items[0].copied_from.as_ref().unwrap().path,
        "src.md"
    );

    // Source stays live and untouched; writing against its old revision still works.
    let still = put_text(
        &vault,
        &owner,
        "src.md",
        "shared bytes v2",
        Precondition::IfRevision(HexId::from_hex(&source.revision).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(still.generation, 2);
}

#[tokio::test]
async fn copy_to_existing_target_conflicts() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    put_text(&vault, &owner, "s.md", "s", Precondition::Create)
        .await
        .unwrap();
    let target = put_text(&vault, &owner, "t.md", "t", Precondition::Create)
        .await
        .unwrap();

    let outcome = vault
        .copy_file(
            &owner,
            TransferInput {
                source: VaultPath::new("s.md").unwrap(),
                target: VaultPath::new("t.md").unwrap(),
                expected_source_revision: HexId::from_hex(&target.revision).unwrap(),
                actor: owner.actor.clone(),
                operation_id: HexId::generate(),
            },
        )
        .await;
    assert!(matches!(outcome, Err(DomainError::RevisionConflict { .. })));
}

#[tokio::test]
async fn move_trashes_source_and_target_keeps_own_history() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let source = put_text(
        &vault,
        &owner,
        "old/place.md",
        "content",
        Precondition::Create,
    )
    .await
    .unwrap();

    let outcome = vault
        .move_file(
            &owner,
            TransferInput {
                source: VaultPath::new("old/place.md").unwrap(),
                target: VaultPath::new("new/name.md").unwrap(),
                expected_source_revision: HexId::from_hex(&source.revision).unwrap(),
                actor: owner.actor.clone(),
                operation_id: HexId::generate(),
            },
        )
        .await
        .unwrap();
    assert!(outcome.copied && outcome.source_trashed && !outcome.partial);

    let source_head = vault
        .current_head(&VaultPath::new("old/place.md").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        source_head.head.state,
        brainstore_domain::HeadState::Trashed
    );
    let target_content = vault
        .read_file(&owner, &VaultPath::new("new/name.md").unwrap())
        .await
        .unwrap();
    assert_eq!(target_content.bytes, "content".as_bytes());
}

/// A source that changed after the copy keeps BOTH files (U15): the trash
/// step's CAS fails and the caller gets a partial result to repair.
#[tokio::test]
async fn move_with_changed_source_reports_partial() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::create(dir.path(), "default").unwrap();
    // The move performs: put(source) CAS → copy target CAS → source trash
    // CAS. Fail exactly the third so the copy lands and the trash loses.
    let heads = FlakyNthCasHeads::new(FsHeadStore::new(store.clone()), 3);
    let vault = Vault::new(
        heads,
        FsRevisionStore::new(store.clone()),
        FsBlobStore::new(store),
        SqliteIndex::open(&dir.path().join("index.db")).unwrap(),
        "default".into(),
        Limits::default(),
        CursorSigner::new([9u8; 32]),
    );
    let owner = owner_principal();
    let source = put_text(&vault, &owner, "moving.md", "bytes", Precondition::Create)
        .await
        .unwrap();

    let outcome = vault
        .move_file(
            &owner,
            TransferInput {
                source: VaultPath::new("moving.md").unwrap(),
                target: VaultPath::new("moved.md").unwrap(),
                expected_source_revision: HexId::from_hex(&source.revision).unwrap(),
                actor: owner.actor.clone(),
                operation_id: HexId::generate(),
            },
        )
        .await
        .unwrap();
    assert!(outcome.partial);
    assert!(outcome.copied);
    assert!(!outcome.source_trashed);

    // Both sides survive for manual repair.
    assert!(
        vault
            .current_head(&VaultPath::new("moving.md").unwrap())
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        vault
            .current_head(&VaultPath::new("moved.md").unwrap())
            .await
            .unwrap()
            .is_some()
    );
}

/// HeadStore wrapper failing the n-th CAS call, 1-based.
struct FlakyNthCasHeads {
    inner: FsHeadStore,
    calls: Arc<AtomicUsize>,
    fail_on: usize,
}

impl FlakyNthCasHeads {
    fn new(inner: FsHeadStore, fail_on: usize) -> Self {
        Self {
            inner,
            calls: Arc::new(AtomicUsize::new(0)),
            fail_on,
        }
    }
}

impl brainstore_ports::HeadStore for FlakyNthCasHeads {
    async fn get(
        &self,
        path: &VaultPath,
    ) -> brainstore_domain::DomainResult<Option<brainstore_ports::StoredHead>> {
        self.inner.get(path).await
    }

    async fn cas(
        &self,
        path: &VaultPath,
        expected: &ExpectedHead,
        new_head: &brainstore_domain::Head,
    ) -> brainstore_domain::DomainResult<CasOutcome> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_on {
            return Ok(CasOutcome::PreconditionFailed);
        }
        self.inner.cas(path, expected, new_head).await
    }

    async fn list(
        &self,
        prefix: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> brainstore_domain::DomainResult<Vec<brainstore_ports::StoredHead>> {
        self.inner.list(prefix, after, limit).await
    }
}
