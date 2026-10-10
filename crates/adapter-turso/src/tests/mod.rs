use crate::TursoIndex;
use brains_domain::FileKind;
use brains_ports::{FileProjection, IndexStore, ListFilter, ListOrder, PARSER_VERSION};

fn projection(path: &str, revision: &str, generation: i64, body: &str) -> FileProjection {
    FileProjection {
        path: path.into(),
        revision: revision.into(),
        generation,
        deleted: false,
        kind: FileKind::Note,
        title: "t".into(),
        body: body.into(),
        size: body.len() as i64,
        sha256: None,
        media_type: "text/markdown; charset=utf-8".into(),
        tags_json: "[\"rust\"]".into(),
        links_json: "[{\"target\":\"b.md\",\"kind\":\"link\"}]".into(),
        parser_version: PARSER_VERSION,
        updated_at: "2026-10-07T14:00:00Z".into(),
    }
}

#[tokio::test]
async fn monotonic_upsert_rejects_older_generation() {
    let idx = TursoIndex::open_in_memory().unwrap();
    idx.upsert(&projection("a.md", "rev2", 2, "新的缓存策略"))
        .await
        .unwrap();
    // An older generation must not overwrite the newer row.
    idx.upsert(&projection("a.md", "rev1", 1, "旧的缓存策略"))
        .await
        .unwrap();
    let rows = idx.scan_literal(None, "", 10, "缓存", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].revision, "rev2");
}

#[tokio::test]
async fn literal_window_returns_nonmatches_for_cursor() {
    let idx = TursoIndex::open_in_memory().unwrap();
    idx.upsert(&projection("a.md", "r1", 1, "命中 缓存 在此"))
        .await
        .unwrap();
    idx.upsert(&projection("b.md", "r1", 1, "没有那个词"))
        .await
        .unwrap();
    let rows = idx.scan_literal(None, "", 10, "缓存", &[]).await.unwrap();
    // Both candidates come back so the caller advances the cursor reliably.
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].path, "a.md");
    assert!(rows[0].match_pos > 0);
    assert_eq!(rows[1].match_pos, 0);
}

#[tokio::test]
async fn fts_matches_and_rejects_bad_syntax() {
    let idx = TursoIndex::open_in_memory().unwrap();
    idx.upsert(&projection("a.md", "r1", 1, "rust ci cache strategy"))
        .await
        .unwrap();
    if !cfg!(feature = "fts") {
        assert!(matches!(
            idx.search_fts("cache", None, &[], 0, 10).await,
            Err(brains_domain::DomainError::CapabilityUnsupported(
                "fts_search"
            ))
        ));
        return;
    }
    let hits = idx.search_fts("cache", None, &[], 0, 10).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "a.md");
    assert!(
        idx.search_fts("\"unclosed", None, &[], 0, 10)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn tombstone_row_stays_but_leaves_search_and_links() {
    let idx = TursoIndex::open_in_memory().unwrap();
    idx.upsert(&projection("a.md", "r1", 1, "缓存正文"))
        .await
        .unwrap();
    let mut tombstone = projection("a.md", "r2", 2, "");
    tombstone.deleted = true;
    idx.upsert(&tombstone).await.unwrap();
    let rows = idx.scan_literal(None, "", 10, "缓存", &[]).await.unwrap();
    assert!(rows.is_empty(), "trashed files leave literal search");
    let count = idx.files_count().await.unwrap();
    assert_eq!(count, 1, "tombstone rows persist with generation");
}

#[tokio::test]
async fn index_listing_supports_tag_and_order() {
    let idx = TursoIndex::open_in_memory().unwrap();
    idx.upsert(&projection("a.md", "r1", 1, "x")).await.unwrap();
    let mut older = projection("z.md", "r1", 1, "y");
    older.updated_at = "2026-10-06T00:00:00Z".into();
    idx.upsert(&older).await.unwrap();
    let rows = idx
        .list_files(
            &ListFilter {
                order: ListOrder::UpdatedDesc,
                ..ListFilter::default()
            },
            None,
            None,
            10,
        )
        .await
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        ["a.md", "z.md"]
    );
}

#[tokio::test]
async fn full_text_preview_and_mutations_follow_current_projection() {
    if !cfg!(feature = "fts") {
        return;
    }
    let index = TursoIndex::open_in_memory().unwrap();
    let body = format!("needle {}", "汉字".repeat(300));
    index
        .upsert(&projection("a.md", "r1", 1, &body))
        .await
        .unwrap();
    let hits = index
        .search_fts("needle", None, &["rust".into()], 0, 10)
        .await
        .unwrap();
    assert_eq!(hits[0].2.chars().count(), 240);
    assert_eq!(hits[0].2, body.chars().take(240).collect::<String>());
    assert!(matches!(
        index.search_fts("\"bad", None, &[], 0, 10).await,
        Err(brains_domain::DomainError::InvalidArgument(_))
    ));
    index
        .upsert(&projection("a.md", "r2", 2, "replacement"))
        .await
        .unwrap();
    assert!(
        index
            .search_fts("needle", None, &[], 0, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        index
            .search_fts("replacement", None, &[], 0, 10)
            .await
            .unwrap()[0]
            .1,
        "r2"
    );
    index.remove_path("a.md").await.unwrap();
    assert!(
        index
            .search_fts("replacement", None, &[], 0, 10)
            .await
            .unwrap()
            .is_empty()
    );
}
