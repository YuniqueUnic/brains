//! Patch semantics (design §11.5): sequential exact edits, any mismatch
//! aborts the whole operation with HEAD untouched; dry-run writes nothing.

use crate::{Precondition, patch::PatchEdit, patch::PatchInput};
use brains_domain::{DomainError, HexId, VaultPath};

use super::support::{TestVault, open_vault, owner_principal, put_text};

fn hex_id(rev: &str) -> HexId {
    HexId::from_hex(rev).unwrap()
}

#[tokio::test]
async fn unique_edit_applies_and_records_history() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let first = put_text(
        &vault,
        &owner,
        "p.md",
        "缓存暂时关闭。",
        Precondition::Create,
    )
    .await
    .unwrap();

    let outcome = vault
        .patch_file(
            &owner,
            &VaultPath::new("p.md").unwrap(),
            Precondition::IfRevision(hex_id(&first.revision)),
            owner.actor.clone(),
            HexId::generate(),
            PatchInput {
                edits: vec![PatchEdit {
                    old_text: "缓存暂时关闭。".into(),
                    new_text: "缓存已启用，并记录命中率。".into(),
                    expected_matches: 1,
                }],
                dry_run: false,
            },
        )
        .await
        .unwrap();
    let committed = outcome.committed.expect("patch commits");
    assert_eq!(committed.generation, 2);

    let content = vault
        .read_file(&owner, &VaultPath::new("p.md").unwrap())
        .await
        .unwrap();
    assert_eq!(content.bytes, "缓存已启用，并记录命中率。".as_bytes());
}

#[tokio::test]
async fn ambiguous_match_aborts_without_touching_head() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let first = put_text(&vault, &owner, "p.md", "same same", Precondition::Create)
        .await
        .unwrap();

    let outcome = vault
        .patch_file(
            &owner,
            &VaultPath::new("p.md").unwrap(),
            Precondition::IfRevision(hex_id(&first.revision)),
            owner.actor.clone(),
            HexId::generate(),
            PatchInput {
                edits: vec![PatchEdit {
                    old_text: "same".into(),
                    new_text: "different".into(),
                    expected_matches: 1,
                }],
                dry_run: false,
            },
        )
        .await;
    assert!(matches!(outcome, Err(DomainError::EditMismatch { .. })));

    // HEAD is unchanged: the next write against the same base succeeds.
    let second = put_text(
        &vault,
        &owner,
        "p.md",
        "replaced wholesale",
        Precondition::IfRevision(hex_id(&first.revision)),
    )
    .await
    .unwrap();
    assert_eq!(second.generation, 2);
}

#[tokio::test]
async fn sequential_edits_apply_in_order_and_all_or_nothing() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let first = put_text(
        &vault,
        &owner,
        "p.md",
        "alpha beta gamma",
        Precondition::Create,
    )
    .await
    .unwrap();

    // Edit 2 targets text that edit 1 creates, in order; edit 3 misses → all abort.
    let outcome = vault
        .patch_file(
            &owner,
            &VaultPath::new("p.md").unwrap(),
            Precondition::IfRevision(hex_id(&first.revision)),
            owner.actor.clone(),
            HexId::generate(),
            PatchInput {
                edits: vec![
                    PatchEdit {
                        old_text: "alpha".into(),
                        new_text: "ALPHA".into(),
                        expected_matches: 1,
                    },
                    PatchEdit {
                        old_text: "delta".into(),
                        new_text: "DELTA".into(),
                        expected_matches: 1,
                    },
                ],
                dry_run: false,
            },
        )
        .await;
    assert!(matches!(outcome, Err(DomainError::EditMismatch { .. })));
    let content = vault
        .read_file(&owner, &VaultPath::new("p.md").unwrap())
        .await
        .unwrap();
    assert_eq!(
        content.bytes,
        "alpha beta gamma".as_bytes(),
        "no partial application"
    );
}

#[tokio::test]
async fn dry_run_returns_diff_without_committing() {
    let TestVault { vault, .. } = open_vault();
    let owner = owner_principal();
    let first = put_text(&vault, &owner, "p.md", "before", Precondition::Create)
        .await
        .unwrap();

    let outcome = vault
        .patch_file(
            &owner,
            &VaultPath::new("p.md").unwrap(),
            Precondition::IfRevision(hex_id(&first.revision)),
            owner.actor.clone(),
            HexId::generate(),
            PatchInput {
                edits: vec![PatchEdit {
                    old_text: "before".into(),
                    new_text: "after".into(),
                    expected_matches: 1,
                }],
                dry_run: true,
            },
        )
        .await
        .unwrap();
    assert!(outcome.committed.is_none());
    assert!(outcome.diff.contains("-before"));
    assert!(outcome.diff.contains("+after"));
    assert_eq!(outcome.base_revision, first.revision);
}
