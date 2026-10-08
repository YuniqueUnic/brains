use crate::{ContentRef, CopyProvenance, HexId, Revision, RevisionAction, VaultPath, blob_key};

fn hex(n: u8) -> HexId {
    HexId::from_hex(&format!("{n:032x}")).unwrap()
}

fn content_for(file: &str) -> ContentRef {
    ContentRef {
        key: blob_key("default", &hex(2), file),
        sha256: "a".repeat(64),
        size: 3,
        media_type: "text/markdown; charset=utf-8".into(),
        executable: false,
    }
}

#[test]
fn put_revision_json_matches_schema() {
    let rev = Revision::new(
        hex(2),
        &VaultPath::new("a.md").unwrap(),
        2,
        Some(hex(1)),
        RevisionAction::Put,
        "token:default".into(),
        HexId::from_hex(&"b".repeat(32)).unwrap(),
        "2026-10-07T14:00:00Z".into(),
        None,
        None,
        Some(content_for("a.md")),
    )
    .unwrap();
    let json = serde_json::to_value(&rev).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["action"], "put");
    assert_eq!(json["parent"], format!("{:032x}", 1u128));
    assert!(json.get("restores_revision").is_none());
    let round: Revision = serde_json::from_value(json).unwrap();
    assert_eq!(round.revision, hex(2));
}

#[test]
fn trash_revision_has_null_content() {
    let rev = Revision::new(
        hex(3),
        &VaultPath::new("a.md").unwrap(),
        3,
        Some(hex(2)),
        RevisionAction::Trash,
        "token:default".into(),
        hex(9),
        "2026-10-07T14:00:00Z".into(),
        None,
        None,
        None,
    )
    .unwrap();
    let json = serde_json::to_value(&rev).unwrap();
    assert!(json["content"].is_null());
    assert!(rev.validate().is_ok());
}

#[test]
fn rejects_provenance_on_put() {
    let err = Revision::new(
        hex(2),
        &VaultPath::new("a.md").unwrap(),
        2,
        Some(hex(1)),
        RevisionAction::Put,
        "actor".into(),
        hex(9),
        "2026-10-07T14:00:00Z".into(),
        Some(hex(5)),
        None,
        Some(content_for("a.md")),
    );
    assert!(err.is_err());
}

#[test]
fn parent_presence_ties_to_generation_one() {
    // generation 2 without a parent is internally inconsistent.
    let err = Revision::new(
        hex(2),
        &VaultPath::new("a.md").unwrap(),
        2,
        None,
        RevisionAction::Put,
        "actor".into(),
        hex(9),
        "2026-10-07T14:00:00Z".into(),
        None,
        None,
        Some(content_for("a.md")),
    );
    assert!(err.is_err());
}

#[test]
fn restore_and_copy_carry_provenance() {
    let restore = Revision::new(
        hex(4),
        &VaultPath::new("a.md").unwrap(),
        4,
        Some(hex(3)),
        RevisionAction::Restore,
        "actor".into(),
        hex(9),
        "2026-10-07T14:00:00Z".into(),
        Some(hex(1)),
        None,
        Some(content_for("a.md")),
    )
    .unwrap();
    assert!(restore.validate().is_ok());

    let copy = Revision::new(
        hex(5),
        &VaultPath::new("b.md").unwrap(),
        1,
        None,
        RevisionAction::Copy,
        "actor".into(),
        hex(9),
        "2026-10-07T14:00:00Z".into(),
        None,
        Some(CopyProvenance {
            path: "a.md".into(),
            revision: hex(4),
        }),
        Some(content_for("b.md")),
    )
    .unwrap();
    assert!(copy.validate().is_ok());
}

#[test]
fn blob_key_layout() {
    let key = blob_key("default", &hex(1), "a.md");
    assert_eq!(key, format!("vaults/default/blobs/{:032x}/a.md", 1u128));
    assert!(crate::revision::validate_blob_key(&key, "a.md").is_ok());
    assert!(
        crate::revision::validate_blob_key("vaults/default/revisions/xx/a.md", "a.md").is_err()
    );
}
