//! First-boot template seeding (design §19 cold start): template validity,
//! empty-vault seeding, and the not-empty no-op guarantee.

use super::support::{open_vault, owner_principal};
use crate::Precondition;
use crate::commit::CommitInput;
use crate::seed::{seed_if_empty, template_notes};
use brains_domain::{HexId, Limits, VaultPath};

#[test]
fn template_paths_are_valid_and_content_is_within_limits() {
    let limits = Limits::default();
    for (raw_path, body) in template_notes() {
        let path = VaultPath::new(raw_path).unwrap_or_else(|e| panic!("path {raw_path}: {e}"));
        assert!(
            body.len() < limits.max_text_bytes,
            "{raw_path} exceeds the text limit"
        );
        assert!(!body.trim().is_empty(), "{raw_path} is empty");
        assert!(
            path.as_str().ends_with(".md"),
            "{raw_path}: template notes are markdown"
        );
    }
}

#[tokio::test]
async fn seeds_an_empty_vault_and_is_a_no_op_afterwards() {
    let test = open_vault();
    assert!(template_notes().len() >= 3, "template is non-trivial");

    let seeded = seed_if_empty(&test.vault).await.unwrap();
    assert_eq!(seeded, template_notes().len());

    // Every template note is now live at generation 1…
    let page = test
        .vault
        .list_from_store(&owner_principal(), None, None, false, 100)
        .await
        .unwrap();
    assert_eq!(page.items.len(), template_notes().len());
    for (raw_path, _) in template_notes() {
        assert!(
            page.items
                .iter()
                .any(|item| item.path.as_str() == *raw_path),
            "{raw_path} missing after seeding"
        );
    }

    // …and a second boot must not touch anything.
    let again = seed_if_empty(&test.vault).await.unwrap();
    assert_eq!(again, 0, "seeding must skip a non-empty vault");
}

#[tokio::test]
async fn seeding_skips_vaults_with_any_head_including_tombstones() {
    let test = open_vault();
    let principal = owner_principal();
    let path = VaultPath::new("notes/pre-existing.md").unwrap();
    test.vault
        .put_file(
            &principal,
            CommitInput {
                path: &path,
                content: b"user content",
                media_type: None,
                actor: principal.actor.clone(),
                operation_id: HexId::generate(),
                precondition: Precondition::Create,
                executable: false,
                copied_from: None,
            },
        )
        .await
        .unwrap();

    let seeded = seed_if_empty(&test.vault).await.unwrap();
    assert_eq!(seeded, 0, "user content must never be touched by seeding");
}
