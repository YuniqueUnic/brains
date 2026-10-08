use crate::FsRevisionStore;
use brains_domain::{ContentRef, HexId, Revision, RevisionAction, VaultPath};
use brains_ports::RevisionStore;

use super::temp_store;

fn sample(generation: i64, id: &HexId, parent: Option<HexId>) -> Revision {
    let blob_rev = HexId::from_hex(&format!("{:032x}", 42u128)).unwrap();
    Revision::new(
        id.clone(),
        &VaultPath::new("a.md").unwrap(),
        generation,
        parent,
        RevisionAction::Put,
        "token:test".into(),
        HexId::generate(),
        "2026-10-07T14:00:00Z".into(),
        None,
        None,
        Some(ContentRef {
            key: format!("vaults/default/blobs/{blob_rev}/a.md"),
            sha256: "a".repeat(64),
            size: 1,
            media_type: "text/markdown; charset=utf-8".into(),
            executable: false,
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn create_then_get_roundtrip() {
    let (_dir, store) = temp_store();
    let store = FsRevisionStore::new(store);
    let id = HexId::generate();
    store.create(&sample(1, &id, None)).await.unwrap();
    let loaded = store.get(&id).await.unwrap().unwrap();
    assert_eq!(loaded.revision, id);
    assert!(store.get(&HexId::generate()).await.unwrap().is_none());
    // Create-only: rewriting the same ID is a hard collision.
    assert!(store.create(&sample(1, &id, None)).await.is_err());
}
