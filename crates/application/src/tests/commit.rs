//! Commit-protocol business behavior (design §8, §19 U03/U07/U08/U09/U06).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{IndexState, Precondition};
use brains_domain::DomainError;

use super::support::{FlakyIndex, TestVault, open_vault, owner_principal, put_text};

#[tokio::test(flavor = "multi_thread")]
async fn two_clients_same_base_exactly_one_wins() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();

    let first = put_text(&vault, &owner, "doc.md", "base", Precondition::Create)
        .await
        .unwrap();
    let base = Precondition::IfRevision(brains_domain::HexId::from_hex(&first.revision).unwrap());

    // Two agents read revision R1 and both try to write. Exactly one
    // outcome is Ok(committed); the other is a revision conflict.
    let vault_a = &vault;
    let (a, b) = tokio::join!(
        put_text(vault_a, &owner, "doc.md", "writer A", base.clone()),
        put_text(vault_a, &owner, "doc.md", "writer B", base.clone())
    );
    let committed = [&a, &b].iter().filter(|r| r.is_ok()).count();
    assert_eq!(committed, 1, "exactly one CAS may commit for a shared base");
    assert!(
        matches!(a, Err(DomainError::RevisionConflict { .. }))
            != matches!(b, Err(DomainError::RevisionConflict { .. }))
    );
    let winner = match (a, b) {
        (Ok(w), Err(_)) | (Err(_), Ok(w)) => w,
        _ => unreachable!("checked above"),
    };
    assert_eq!(winner.index_state, IndexState::Ready);

    // The loser's content must not be silently dropped into history.
    let mut cursor: Option<String> = None;
    let mut actions = Vec::new();
    loop {
        let page = vault
            .history_page(
                &owner,
                &brains_domain::VaultPath::new("doc.md").unwrap(),
                cursor.as_deref(),
                20,
            )
            .await
            .unwrap();
        actions.extend(page.items.iter().map(|rev| rev.action));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(actions.len(), 2, "only committed revisions join the chain");
}

#[tokio::test]
async fn restore_makes_new_revision_and_never_rewinds() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let path = brains_domain::VaultPath::new("restore.md").unwrap();

    let v1 = put_text(
        &vault,
        &owner,
        "restore.md",
        "version one",
        Precondition::Create,
    )
    .await
    .unwrap();
    let v2 = put_text(
        &vault,
        &owner,
        "restore.md",
        "version two",
        Precondition::IfRevision(brains_domain::HexId::from_hex(&v1.revision).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(v2.generation, 2);

    let restored = vault
        .restore_file(
            &owner,
            &path,
            &brains_domain::HexId::from_hex(&v1.revision).unwrap(),
            Precondition::IfRevision(brains_domain::HexId::from_hex(&v2.revision).unwrap()),
            owner.actor.clone(),
            brains_domain::HexId::generate(),
        )
        .await
        .unwrap();
    assert_eq!(restored.generation, 3, "restore advances generation");
    assert_eq!(
        restored.sha256, v1.sha256,
        "restored bytes equal the source revision"
    );

    // The old base stays stale: writing against v1 must still be a conflict.
    let stale = put_text(
        &vault,
        &owner,
        "restore.md",
        "stale write",
        Precondition::IfRevision(brains_domain::HexId::from_hex(&v1.revision).unwrap()),
    )
    .await;
    assert!(matches!(stale, Err(DomainError::RevisionConflict { .. })));

    // History chain: restore → put(v2) → put(v1), all reachable.
    let page = vault.history_page(&owner, &path, None, 20).await.unwrap();
    let actions: Vec<&str> = page.items.iter().map(|rev| rev.action.as_str()).collect();
    assert_eq!(actions, ["restore", "put", "put"]);
    assert_eq!(
        page.items[0].restores_revision.as_ref().map(|r| r.as_str()),
        Some(v1.revision.as_str())
    );
}

#[tokio::test]
async fn trash_tombstone_and_recreate_from_exact_revision() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let path = brains_domain::VaultPath::new("trash-me.md").unwrap();

    let created = put_text(
        &vault,
        &owner,
        "trash-me.md",
        "content",
        Precondition::Create,
    )
    .await
    .unwrap();
    let trashed = vault
        .trash_file(
            &owner,
            &path,
            Precondition::IfRevision(brains_domain::HexId::from_hex(&created.revision).unwrap()),
            owner.actor.clone(),
            brains_domain::HexId::generate(),
        )
        .await
        .unwrap();
    assert_eq!(trashed.generation, 2);

    // Reading a tombstone answers Trashed, not NotFound.
    let read = vault.read_file(&owner, &path).await;
    assert!(matches!(read, Err(DomainError::Trashed { .. })));

    // Create-only against a tombstone fails: a tombstone is an existing HEAD.
    let recreate = put_text(&vault, &owner, "trash-me.md", "again", Precondition::Create).await;
    assert!(matches!(
        recreate,
        Err(DomainError::RevisionConflict { .. })
    ));

    // PUT with the tombstone's exact revision creates new live content.
    let revived = put_text(
        &vault,
        &owner,
        "trash-me.md",
        "revived",
        Precondition::IfRevision(brains_domain::HexId::from_hex(&trashed.revision).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(revived.generation, 3);
    // DELETE against the same tombstone revision would now miss: HEAD moved.
    let double_trash = vault
        .trash_file(
            &owner,
            &path,
            Precondition::IfRevision(brains_domain::HexId::from_hex(&trashed.revision).unwrap()),
            owner.actor.clone(),
            brains_domain::HexId::generate(),
        )
        .await;
    assert!(matches!(
        double_trash,
        Err(DomainError::RevisionConflict { .. })
    ));
}

/// HEAD CAS committed but the index write failed: the file stays readable,
/// the mutation reports `pending`, and search fails loudly instead of
/// pretending the vault is empty (design §8.4, U06).
#[tokio::test]
async fn index_failure_does_not_lose_the_commit() {
    let TestVault { vault, dir } = open_vault();
    let owner = owner_principal();

    let ok = put_text(&vault, &owner, "ok.md", "indexed", Precondition::Create)
        .await
        .unwrap();
    assert_eq!(ok.index_state, IndexState::Ready);

    // Swap in a flaky index that fails every subsequent upsert.
    let broken = Arc::new(AtomicUsize::new(0));
    let index = brains_adapter_turso::TursoIndex::open(&dir.path().join("index.db")).unwrap();
    let flaky = FlakyIndex::new(index, 0, broken.clone());
    let degraded = crate::Vault::new(
        vault.heads,
        vault.revisions,
        vault.blobs,
        flaky,
        "default".into(),
        brains_domain::Limits::default(),
        brains_domain::CursorSigner::new([9u8; 32]),
    );

    let outcome = put_text(
        &degraded,
        &owner,
        "pending.md",
        "committed but unindexed",
        Precondition::Create,
    )
    .await
    .unwrap();
    assert_eq!(outcome.index_state, IndexState::Pending);
    assert_eq!(broken.load(Ordering::SeqCst), 1);

    // The committed file is still readable from main data.
    let content = degraded
        .read_file(
            &owner,
            &brains_domain::VaultPath::new("pending.md").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(content.bytes, b"committed but unindexed");

    // Search must report the dependency failure, never an empty result.
    let search = degraded
        .search(
            &owner,
            crate::search::SearchQuery {
                q: "committed".into(),
                mode: crate::search::SearchMode::Literal,
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
    assert!(matches!(search, Err(DomainError::IndexUnavailable(_))));
}

#[tokio::test]
async fn text_limits_reject_non_utf8_and_demote_oversize() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();

    // A .md that is not UTF-8 is a fatal pre-CAS error (§22.2).
    let path = brains_domain::VaultPath::new("broken.md").unwrap();
    let err = vault
        .put_file(
            &owner,
            crate::commit::CommitInput {
                path: &path,
                content: &[0xff, 0xfe, 0x61],
                media_type: None,
                actor: owner.actor.clone(),
                operation_id: brains_domain::HexId::generate(),
                precondition: Precondition::Create,
                executable: Some(false),
                copied_from: None,
            },
        )
        .await;
    assert!(matches!(err, Err(DomainError::InvalidFormat(_))));

    // Content over the text cap is stored (attachment semantics) but not indexed.
    let big = "x".repeat(300 * 1024);
    let stored = put_text(&vault, &owner, "big.md", &big, Precondition::Create)
        .await
        .unwrap();
    assert_eq!(stored.size, 300 * 1024);
    let content = vault
        .read_file(&owner, &brains_domain::VaultPath::new("big.md").unwrap())
        .await
        .unwrap();
    assert_eq!(content.bytes.len(), 300 * 1024);

    // Beyond the attachment cap the write is rejected outright.
    let huge = "y".repeat(9 * 1024 * 1024);
    let err = put_text(&vault, &owner, "huge.bin", &huge, Precondition::Create).await;
    assert!(matches!(err, Err(DomainError::TooLarge { .. })));
}

#[tokio::test]
async fn attachment_and_note_share_the_revision_protocol() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();

    // Upload a CSV attachment.
    let csv = "latency_ms,sample\n12,1\n9,2\n";
    let attachment = put_text(
        &vault,
        &owner,
        "attachments/bench.csv",
        csv,
        Precondition::Create,
    )
    .await
    .unwrap();

    // Link it from a note.
    let note = "# 基准\n\n[结果](../attachments/bench.csv)\n".to_owned();
    let _note = put_text(
        &vault,
        &owner,
        "notes/bench.md",
        &note,
        Precondition::Create,
    )
    .await
    .unwrap();

    // The attachment bytes are byte-identical through a full round trip.
    let content = vault
        .read_file(
            &owner,
            &brains_domain::VaultPath::new("attachments/bench.csv").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(content.bytes, csv.as_bytes());
    assert_eq!(content.revision.content.unwrap().sha256, attachment.sha256);

    // The outbound link resolves to the attachment path.
    let links = vault
        .links(
            &owner,
            &brains_domain::VaultPath::new("notes/bench.md").unwrap(),
            false,
            None,
            10,
        )
        .await
        .unwrap();
    assert_eq!(links.rows.len(), 1);
    assert_eq!(links.rows[0].target, "attachments/bench.csv");
    let backlinks = vault
        .links(
            &owner,
            &brains_domain::VaultPath::new("attachments/bench.csv").unwrap(),
            true,
            None,
            10,
        )
        .await
        .unwrap();
    assert_eq!(backlinks.rows.len(), 1);
    assert_eq!(backlinks.rows[0].source, "notes/bench.md");
}

/// Lost-response recovery (U09): the client retries with the same
/// If-Match and finds the operation already committed — one writer wins,
/// and the chain shows one `put`, never two appends.
#[tokio::test]
async fn retry_after_lost_response_does_not_double_append() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let first = put_text(&vault, &owner, "log.md", "line1", Precondition::Create)
        .await
        .unwrap();
    let base = Precondition::IfRevision(brains_domain::HexId::from_hex(&first.revision).unwrap());

    let winner = put_text(&vault, &owner, "log.md", "line1\nline2", base.clone())
        .await
        .unwrap();
    // The retry (same base revision, new operation id) hits 412...
    let retry = put_text(&vault, &owner, "log.md", "line1\nline2", base).await;
    assert!(matches!(
        &retry,
        Err(DomainError::RevisionConflict { current_revision, .. })
            if current_revision.as_deref() == Some(winner.revision.as_str())
    ));

    // ...and the history contains exactly one follow-up put.
    let page = vault
        .history_page(
            &owner,
            &brains_domain::VaultPath::new("log.md").unwrap(),
            None,
            20,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
}

/// The commit result timestamp equals the HEAD timestamp so raw-backup
/// validation (`rev.created_at == head.updated_at`) always holds.
#[tokio::test]
async fn head_and_revision_timestamps_agree() {
    let TestVault { vault, dir } = open_vault();
    let owner = owner_principal();
    put_text(&vault, &owner, "t.md", "x", Precondition::Create)
        .await
        .unwrap();

    let head_file = dir.path().join("vaults/default/heads/t.md.json");
    let head: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(head_file).unwrap()).unwrap();
    let rev_file = std::fs::read_dir(dir.path().join("vaults/default/revisions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let rev: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(rev_file).unwrap()).unwrap();
    assert_eq!(head["updated_at"], rev["created_at"]);
}

#[tokio::test]
async fn purge_removes_tombstone_and_rejects_live_paths() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let path = brains_domain::VaultPath::new("purge-me.md").unwrap();

    let created = put_text(
        &vault,
        &owner,
        "purge-me.md",
        "content",
        Precondition::Create,
    )
    .await
    .unwrap();
    let tombstone = brains_domain::HexId::from_hex(&created.revision).unwrap();

    // A live path must not purge — trash first, always.
    let live = vault
        .purge_file(
            &owner,
            &path,
            Precondition::IfRevision(tombstone.clone()),
            brains_domain::HexId::generate(),
        )
        .await;
    assert!(matches!(live, Err(DomainError::PurgeLive { .. })));

    let trashed = vault
        .trash_file(
            &owner,
            &path,
            Precondition::IfRevision(tombstone),
            owner.actor.clone(),
            brains_domain::HexId::generate(),
        )
        .await
        .unwrap();

    // Stale base: the tombstone moved (it did not, but the guard must hold).
    let stale = vault
        .purge_file(
            &owner,
            &path,
            Precondition::IfRevision(brains_domain::HexId::from_hex(&created.revision).unwrap()),
            brains_domain::HexId::generate(),
        )
        .await;
    assert!(matches!(stale, Err(DomainError::RevisionConflict { .. })));

    let purged = vault
        .purge_file(
            &owner,
            &path,
            Precondition::IfRevision(brains_domain::HexId::from_hex(&trashed.revision).unwrap()),
            brains_domain::HexId::generate(),
        )
        .await
        .unwrap();
    assert_eq!(purged.path, "purge-me.md");

    // Gone from listings and unreadable — Trashed became NotFound.
    let page = vault
        .list_from_store(&owner, None, None, true, 50)
        .await
        .unwrap();
    assert!(!page.items.iter().any(|e| e.path == "purge-me.md"));
    let read = vault.read_file(&owner, &path).await;
    assert!(matches!(read, Err(DomainError::NotFound { .. })));

    // Re-purging an absent path cannot meet its precondition: the 412 is
    // the honest answer (never a fake success).
    let again = vault
        .purge_file(
            &owner,
            &path,
            Precondition::IfRevision(brains_domain::HexId::from_hex(&trashed.revision).unwrap()),
            brains_domain::HexId::generate(),
        )
        .await;
    assert!(matches!(again, Err(DomainError::RevisionConflict { .. })));
}
