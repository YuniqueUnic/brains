//! Index rebuild (design §10.5, U10): a fresh index rebuilds from main data
//! without changing any file bytes, and reports `building` while empty.

use crate::Precondition;
use crate::Vault;
use brainstore_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore, FsStore};
use brainstore_adapter_sqlite::SqliteIndex;
use brainstore_domain::{CursorSigner, Limits, VaultPath};

use super::support::{TestVault, open_vault, owner_principal, put_text};

#[tokio::test]
async fn fresh_index_reports_building_then_rebuild_restores_search() {
    let TestVault { vault, dir } = open_vault();
    let owner = owner_principal();
    for i in 0..7 {
        put_text(
            &vault,
            &owner,
            &format!("kb/note{i}.md"),
            &format!("缓存方案 {i}：分层失效"),
            Precondition::Create,
        )
        .await
        .unwrap();
    }

    // Baseline search from the live index.
    let before = vault
        .search(
            &owner,
            crate::search::SearchQuery {
                q: "缓存".into(),
                mode: crate::search::SearchMode::Literal,
                prefix: None,
                tag: None,
                cursor: None,
                scan_limit: 100,
            },
        )
        .await
        .unwrap();
    assert_eq!(before.items.len(), 7);

    // "Lose" the index: a brand-new empty database.
    let fresh_db = dir.path().join("fresh.db");
    let fresh = Vault::new(
        FsHeadStore::new(FsStore::open(dir.path(), "default").unwrap()),
        FsRevisionStore::new(FsStore::open(dir.path(), "default").unwrap()),
        FsBlobStore::new(FsStore::open(dir.path(), "default").unwrap()),
        SqliteIndex::open(&fresh_db).unwrap(),
        "default".into(),
        Limits::default(),
        CursorSigner::new([9u8; 32]),
    );
    let empty = fresh
        .search(
            &owner,
            crate::search::SearchQuery {
                q: "缓存".into(),
                mode: crate::search::SearchMode::Literal,
                prefix: None,
                tag: None,
                cursor: None,
                scan_limit: 100,
            },
        )
        .await
        .unwrap();
    assert!(empty.building, "an empty index reports building, not empty");
    assert!(empty.items.is_empty());

    // Drive the paged rebuild to completion.
    let mut cursor: Option<String> = None;
    let mut processed = 0usize;
    loop {
        let page = fresh
            .reindex_page(&owner, cursor.as_deref(), 5)
            .await
            .unwrap();
        processed += page.processed;
        assert!(
            page.failed.is_empty(),
            "rebuild failures: {:?}",
            page.failed
        );
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(processed, 7);

    // Search matches the pre-loss result; file bytes are untouched.
    let after = fresh
        .search(
            &owner,
            crate::search::SearchQuery {
                q: "缓存".into(),
                mode: crate::search::SearchMode::Literal,
                prefix: None,
                tag: None,
                cursor: None,
                scan_limit: 100,
            },
        )
        .await
        .unwrap();
    assert!(!after.building);
    assert_eq!(after.items.len(), before.items.len());
    let content = fresh
        .read_file(&owner, &VaultPath::new("kb/note0.md").unwrap())
        .await
        .unwrap();
    assert_eq!(content.bytes, "缓存方案 0：分层失效".as_bytes());
}

#[tokio::test]
async fn reindex_requires_whole_vault_admin() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    put_text(&vault, &owner, "a.md", "x", Precondition::Create)
        .await
        .unwrap();

    let limited = super::support::limited_principal("kb");
    let denied = vault.reindex_page(&limited, None, 5).await;
    assert!(matches!(
        denied,
        Err(brainstore_domain::DomainError::Forbidden(_))
    ));

    let allowed = vault.reindex_page(&owner, None, 5).await.unwrap();
    assert_eq!(allowed.processed, 1);
}
