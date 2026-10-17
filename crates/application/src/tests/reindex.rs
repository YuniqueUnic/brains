//! Index rebuild (design §10.5, U10): a fresh index rebuilds from main data
//! without changing any file bytes, and reports `building` while empty.

use crate::Precondition;
use crate::Vault;
use brains_adapter_turso::TursoIndex;
use brains_domain::{CursorSigner, Limits, VaultPath};

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
                tags: vec![],
                status: None,
                type_: None,
                trust: None,
                stale: None,
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
        vault.heads,
        vault.revisions,
        vault.blobs,
        TursoIndex::open(&fresh_db).unwrap(),
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
                tags: vec![],
                status: None,
                type_: None,
                trust: None,
                stale: None,
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
                tags: vec![],
                status: None,
                type_: None,
                trust: None,
                stale: None,
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
        Err(brains_domain::DomainError::Forbidden(_))
    ));

    let allowed = vault.reindex_page(&owner, None, 5).await.unwrap();
    assert_eq!(allowed.processed, 1);
}

#[tokio::test]
async fn reindex_refreshes_old_projection_with_repeated_relations() {
    use brains_ports::{IndexStore, PARSER_VERSION, RevisionStore};

    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let text = "---\ntags: [rust, rust]\n---\n[First](target.md) [Again](./target.md#section)";
    let committed = put_text(&vault, &owner, "source.md", text, Precondition::Create)
        .await
        .unwrap();
    let id = brains_domain::HexId::from_hex(&committed.revision).unwrap();
    let revision = vault.revisions.get(&id).await.unwrap().unwrap();
    let mut old = crate::projection::build_projection(
        "default",
        &revision,
        Some(text.as_bytes()),
        false,
        vault.limits.max_text_bytes,
    );
    old.parser_version = PARSER_VERSION - 1;
    old.tags_json = r#"["rust","rust"]"#.into();
    old.links_json =
        r#"[{"target":"target.md","kind":"link"},{"target":"target.md","kind":"link"}]"#.into();
    vault.index.remove_path("source.md").await.unwrap();
    vault.index.upsert(&old).await.unwrap();

    // Same HEAD and generation: the parser version admits the repaired projection.
    let rebuilt = vault.reindex_page(&owner, None, 5).await.unwrap();
    assert!(rebuilt.failed.is_empty(), "{:?}", rebuilt.failed);
    assert_eq!(rebuilt.processed, 1);
    let links = vault
        .index
        .links_outbound("source.md", None, 50)
        .await
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].source_revision, committed.revision);
    let read = vault
        .read_file(&owner, &VaultPath::new("source.md").unwrap())
        .await
        .unwrap();
    assert_eq!(read.head.revision, id);
    assert_eq!(read.bytes, text.as_bytes());
}

#[tokio::test]
async fn reindex_reparses_yaml_tags_without_republishing_the_document() {
    use brains_ports::{IndexStore, RevisionStore};

    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let text = "---\ntags: ['a,b', yes]\n---\n正文保持不变";
    let committed = put_text(&vault, &owner, "quoted.md", text, Precondition::Create)
        .await
        .unwrap();
    let id = brains_domain::HexId::from_hex(&committed.revision).unwrap();
    let revision = vault.revisions.get(&id).await.unwrap().unwrap();
    let mut old = crate::projection::build_projection(
        "default",
        &revision,
        Some(text.as_bytes()),
        false,
        vault.limits.max_text_bytes,
    );
    // The released v2 parser split quoted commas. Keep the historical version
    // explicit so forgetting to advance PARSER_VERSION cannot pass this check.
    old.parser_version = 2;
    old.tags_json = r#"["a","b","yes"]"#.into();
    vault.index.remove_path("quoted.md").await.unwrap();
    vault.index.upsert(&old).await.unwrap();

    let rebuilt = vault.reindex_page(&owner, None, 5).await.unwrap();
    assert!(rebuilt.failed.is_empty());
    assert_eq!(rebuilt.processed, 1);
    for (tag, expected) in [("a,b", 1), ("yes", 1), ("a", 0), ("b", 0)] {
        let hits = vault
            .search(
                &owner,
                crate::search::SearchQuery {
                    q: "正文".into(),
                    mode: crate::search::SearchMode::Literal,
                    prefix: None,
                    tags: vec![tag.into()],
                    status: None,
                    type_: None,
                    trust: None,
                    stale: None,
                    cursor: None,
                    scan_limit: 100,
                },
            )
            .await
            .unwrap();
        assert_eq!(hits.items.len(), expected, "tag: {tag}");
    }
    let read = vault
        .read_file(&owner, &VaultPath::new("quoted.md").unwrap())
        .await
        .unwrap();
    assert_eq!(read.head.revision, id);
    assert_eq!(read.bytes, text.as_bytes());
}
