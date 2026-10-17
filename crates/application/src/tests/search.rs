//! Bounded search behavior (design §10.3, U05): zero hits with a pending
//! cursor is never a complete answer; windows page through the whole vault.

use crate::Precondition;
use crate::search::{SearchMode, SearchQuery, SearchResult};

use super::support::{TestVault, open_vault, owner_principal, put_text};

fn literal_query(q: &str, cursor: Option<String>, scan_limit: usize) -> SearchQuery {
    SearchQuery {
        q: q.into(),
        mode: SearchMode::Literal,
        prefix: None,
        tags: vec![],
        status: None,
        type_: None,
        trust: None,
        stale: None,
        cursor,
        scan_limit,
    }
}

#[tokio::test]
async fn chinese_literal_search_pages_across_windows() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();

    // 250 files, every one contains the query — the answer needs 3 windows.
    for i in 0..250 {
        put_text(
            &vault,
            &owner,
            &format!("notes/file{i:03}.md"),
            &format!("缓存策略 {i}"),
            Precondition::Create,
        )
        .await
        .unwrap();
    }

    let mut cursor: Option<String> = None;
    let mut total = 0usize;
    let mut pages = 0usize;
    loop {
        let result: SearchResult = vault
            .search(&owner, literal_query("缓存", cursor.clone(), 100))
            .await
            .unwrap();
        pages += 1;
        total += result.items.len();
        if result.complete {
            assert!(result.next_cursor.is_none());
            break;
        }
        assert_eq!(result.scanned, 100);
        cursor = result.next_cursor;
        assert!(cursor.is_some(), "incomplete page must carry a cursor");
    }
    assert_eq!(pages, 3);
    assert_eq!(total, 250);
}

#[tokio::test]
async fn zero_hits_in_a_full_scan_reports_complete_and_one() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    put_text(&vault, &owner, "a.md", "没有要找的词", Precondition::Create)
        .await
        .unwrap();

    let result = vault
        .search(&owner, literal_query("缓存", None, 100))
        .await
        .unwrap();
    assert!(result.complete);
    assert!(result.items.is_empty());
    assert_eq!(result.scanned, 1);
}

#[tokio::test]
async fn cursor_binds_to_query_and_prefix() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    for i in 0..5 {
        put_text(
            &vault,
            &owner,
            &format!("notes/f{i}.md"),
            "缓存 content",
            Precondition::Create,
        )
        .await
        .unwrap();
    }
    let first = vault
        .search(&owner, literal_query("缓存", None, 2))
        .await
        .unwrap();
    assert!(!first.complete);
    let cursor = first.next_cursor.unwrap();

    // The same cursor against a different query must be rejected, not reused.
    let wrong_query = vault
        .search(&owner, literal_query("其他", Some(cursor.clone()), 2))
        .await;
    assert!(wrong_query.is_err());
}

#[tokio::test]
async fn fts_mode_matches_tokens_and_rejects_bad_syntax() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    put_text(
        &vault,
        &owner,
        "a.md",
        "rust ci cache strategy",
        Precondition::Create,
    )
    .await
    .unwrap();

    let hits = vault
        .search(
            &owner,
            SearchQuery {
                q: "cache".into(),
                mode: SearchMode::Fts,
                prefix: None,
                tags: vec![],
                status: None,
                type_: None,
                trust: None,
                stale: None,
                cursor: None,
                scan_limit: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(hits.items.len(), 1);
    assert_eq!(hits.items[0].path, "a.md");

    let bad = vault
        .search(
            &owner,
            SearchQuery {
                q: "\"unclosed".into(),
                mode: SearchMode::Fts,
                prefix: None,
                tags: vec![],
                status: None,
                type_: None,
                trust: None,
                stale: None,
                cursor: None,
                scan_limit: 10,
            },
        )
        .await;
    assert!(
        bad.is_err(),
        "invalid FTS syntax is a 400, never a fallback"
    );
}

#[tokio::test]
async fn tag_filter_narrows_literal_candidates() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    // Tagged note via frontmatter.
    put_text(
        &vault,
        &owner,
        "tagged.md",
        "---\ntags: [缓存专题]\n---\n缓存 tagged body",
        Precondition::Create,
    )
    .await
    .unwrap();
    put_text(
        &vault,
        &owner,
        "untagged.md",
        "缓存 untagged body",
        Precondition::Create,
    )
    .await
    .unwrap();

    let mut query = literal_query("缓存", None, 100);
    query.tags = vec!["缓存专题".into()];
    let hits = vault.search(&owner, query).await.unwrap();
    assert_eq!(
        hits.items
            .iter()
            .map(|h| h.path.as_str())
            .collect::<Vec<_>>(),
        ["tagged.md"]
    );
}

#[tokio::test]
async fn scoped_token_cannot_search_beyond_prefix() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    put_text(
        &vault,
        &owner,
        "secret/private.md",
        "缓存 secret",
        Precondition::Create,
    )
    .await
    .unwrap();
    put_text(
        &vault,
        &owner,
        "public/open.md",
        "缓存 open",
        Precondition::Create,
    )
    .await
    .unwrap();

    let limited = super::support::limited_principal("public");
    // A whole-vault query without prefix is forbidden for a scoped token.
    let whole = vault
        .search(&limited, literal_query("缓存", None, 10))
        .await;
    assert!(matches!(
        whole,
        Err(brains_domain::DomainError::Forbidden(_))
    ));

    // A prefix inside the scope works and never sees other scopes' hits.
    let mut query = literal_query("缓存", None, 10);
    query.prefix = Some("public".into());
    let hits = vault.search(&limited, query).await.unwrap();
    assert_eq!(
        hits.items
            .iter()
            .map(|h| h.path.as_str())
            .collect::<Vec<_>>(),
        ["public/open.md"]
    );
}

#[tokio::test]
async fn query_subtrees_preserve_scope_unicode_and_pagination() {
    use crate::listing::{ListQuery, ListSource};
    use brains_domain::DomainError;
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    for prefix in ["public", "public.md", "docs/ÿ", "资料/缓存"] {
        let expected = vec![
            prefix.to_owned(),
            format!("{prefix}/a.md"),
            format!("{prefix}/b.md"),
        ];
        for path in expected.iter().cloned().chain([
            format!("{prefix}-private.md"),
            format!("{prefix}2/private.md"),
        ]) {
            put_text(
                &vault,
                &owner,
                &path,
                "---\ntags: [topic]\n---\nneedle",
                Precondition::Create,
            )
            .await
            .unwrap();
        }
        let limited = super::support::limited_principal(&format!("{prefix}/"));
        for requested in [prefix.to_owned(), format!("{prefix}/")] {
            for mode in [SearchMode::Literal, SearchMode::Fts] {
                let mut paths = Vec::new();
                let mut cursor = None;
                loop {
                    let page = vault
                        .search(
                            &limited,
                            SearchQuery {
                                q: "needle".into(),
                                mode,
                                prefix: Some(requested.clone()),
                                tags: vec![],
                                status: None,
                                type_: None,
                                trust: None,
                                stale: None,
                                cursor,
                                scan_limit: 1,
                            },
                        )
                        .await
                        .unwrap();
                    assert!(page.scanned <= 1);
                    paths.extend(page.items.into_iter().map(|hit| hit.path));
                    if page.complete {
                        break;
                    }
                    cursor = page.next_cursor;
                    assert!(cursor.is_some());
                    assert!(paths.len() <= expected.len());
                }
                paths.sort();
                // Extensionless exact paths are binary; only declared text enters search.
                assert_eq!(
                    paths,
                    expected
                        .iter()
                        .filter(|p| p.ends_with(".md"))
                        .cloned()
                        .collect::<Vec<_>>()
                );
            }
            for source in [ListSource::Store, ListSource::Index] {
                let mut paths = Vec::new();
                let mut cursor = None;
                loop {
                    let page = vault
                        .list_files(
                            &limited,
                            ListQuery {
                                source,
                                prefix: Some(requested.clone()),
                                limit: 1,
                                cursor,
                                ..ListQuery::default()
                            },
                        )
                        .await
                        .unwrap();
                    paths.extend(page.items.into_iter().map(|entry| entry.path));
                    cursor = page.next_cursor;
                    if cursor.is_none() {
                        break;
                    }
                    assert!(paths.len() <= expected.len());
                }
                assert_eq!(paths, expected);
            }
        }
        assert!(matches!(
            vault.list_files(&limited, ListQuery::default()).await,
            Err(DomainError::Forbidden(_))
        ));
    }
}

#[tokio::test]
async fn combined_tags_filter_before_paging_and_bind_normalized_cursor() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    for (path, tags) in [
        ("a.md", "[agent-memory]"),
        ("b.md", "[agent-memory, 'a,b']"),
        ("c.md", "['a,b', agent-memory]"),
    ] {
        put_text(
            &vault,
            &owner,
            path,
            &format!("---\ntags: {tags}\n---\nneedle"),
            Precondition::Create,
        )
        .await
        .unwrap();
    }
    for mode in [SearchMode::Literal, SearchMode::Fts] {
        let mut query = literal_query("needle", None, 1);
        query.mode = mode;
        query.tags = vec!["agent-memory".into(), "a,b".into(), "agent-memory".into()];
        let first = vault.search(&owner, query.clone()).await.unwrap();
        assert_eq!(first.items.len(), 1);
        assert_ne!(first.items[0].path, "a.md");
        query.cursor = first.next_cursor;
        query.tags = vec!["a,b".into(), "agent-memory".into()];
        let next = vault.search(&owner, query.clone()).await.unwrap();
        assert_eq!(next.items.len(), 1);
        assert_ne!(next.items[0].path, first.items[0].path);
        query.tags = vec!["agent-memory".into()];
        assert!(matches!(
            vault.search(&owner, query).await,
            Err(brains_domain::DomainError::InvalidArgument(_))
        ));
    }
    let page = vault
        .list_files(
            &owner,
            crate::listing::ListQuery {
                source: crate::listing::ListSource::Index,
                tags: vec!["a,b".into(), "agent-memory".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>(),
        ["b.md", "c.md"]
    );
}

#[tokio::test]
async fn literal_search_matches_file_paths_with_the_same_tag_intersection() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    put_text(
        &vault,
        &owner,
        "notes/unique-path.md",
        "---\ntags: [agent-memory, photon]\n---\nbody without the filename",
        Precondition::Create,
    )
    .await
    .unwrap();
    let mut query = literal_query("unique-path", None, 100);
    query.tags = vec!["agent-memory".into(), "photon".into()];
    assert_eq!(
        vault.search(&owner, query).await.unwrap().items[0].path,
        "notes/unique-path.md"
    );
}
