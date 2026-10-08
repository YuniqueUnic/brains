use crate::FsHeadStore;
use brainstore_domain::{Head, HeadState, HexId, VaultPath};
use brainstore_ports::{CasOutcome, ExpectedHead, HeadStore};

use super::temp_store;

fn head_for(path: &str, generation: i64, revision: &HexId) -> Head {
    Head {
        schema_version: 1,
        path: path.into(),
        revision: revision.clone(),
        generation,
        state: HeadState::Live,
        updated_at: "2026-10-07T14:00:00Z".into(),
    }
}

#[tokio::test]
async fn cas_absent_then_token() {
    let (_dir, store) = temp_store();
    let heads = FsHeadStore::new(store);
    let path = VaultPath::new("a.md").unwrap();
    let rev1 = HexId::from_hex(&"1".repeat(32)).unwrap();
    let rev2 = HexId::from_hex(&"2".repeat(32)).unwrap();
    let rev3 = HexId::from_hex(&"3".repeat(32)).unwrap();

    assert_eq!(
        heads
            .cas(&path, &ExpectedHead::Absent, &head_for("a.md", 1, &rev1))
            .await
            .unwrap(),
        CasOutcome::Committed
    );
    // Second Absent CAS must fail: the HEAD now exists.
    assert_eq!(
        heads
            .cas(&path, &ExpectedHead::Absent, &head_for("a.md", 1, &rev2))
            .await
            .unwrap(),
        CasOutcome::PreconditionFailed
    );
    // Token CAS against the winner succeeds.
    assert_eq!(
        heads
            .cas(
                &path,
                &ExpectedHead::StorageToken(rev1.as_str().to_owned()),
                &head_for("a.md", 2, &rev3)
            )
            .await
            .unwrap(),
        CasOutcome::Committed
    );
    assert_eq!(
        heads
            .get(&path)
            .await
            .unwrap()
            .unwrap()
            .head
            .revision
            .as_str(),
        rev3.as_str()
    );
}

#[tokio::test]
async fn list_is_ordered_and_prefixed() {
    let (_dir, store) = temp_store();
    let heads = FsHeadStore::new(store);
    for p in ["b/2.md", "a/1.md", "a/3.md", "c.md"] {
        let path = VaultPath::new(p).unwrap();
        heads
            .cas(
                &path,
                &ExpectedHead::Absent,
                &head_for(p, 1, &HexId::generate()),
            )
            .await
            .unwrap();
    }
    let all = heads.list(None, None, 10).await.unwrap();
    let paths: Vec<&str> = all.iter().map(|s| s.head.path.as_str()).collect();
    assert_eq!(paths, ["a/1.md", "a/3.md", "b/2.md", "c.md"]);

    let page1 = heads.list(Some("a"), None, 1).await.unwrap();
    assert_eq!(page1.len(), 1);
    assert_eq!(page1[0].head.path.as_str(), "a/1.md");
    let page2 = heads.list(Some("a"), Some("a/1.md"), 10).await.unwrap();
    assert_eq!(page2[0].head.path.as_str(), "a/3.md");
}

/// Two processes racing the same base revision: exactly one wins.
/// This is the local-adapter face of design §8.3.
#[tokio::test(flavor = "multi_thread")]
async fn cross_task_cas_admits_exactly_one_writer() {
    let (_dir, store) = temp_store();
    let heads = std::sync::Arc::new(FsHeadStore::new(store));
    let path = VaultPath::new("race.md").unwrap();
    let base = HexId::from_hex(&"1".repeat(32)).unwrap();
    heads
        .cas(&path, &ExpectedHead::Absent, &head_for("race.md", 1, &base))
        .await
        .unwrap();

    let mut handles = Vec::new();
    for n in 2..4 {
        let heads = heads.clone();
        let path = path.clone();
        let rev = HexId::from_hex(&format!("{n:032x}")).unwrap();
        let base = base.clone();
        handles.push(tokio::spawn(async move {
            heads
                .cas(
                    &path,
                    &ExpectedHead::StorageToken(base.as_str().to_owned()),
                    &head_for("race.md", 2, &rev),
                )
                .await
                .unwrap()
        }));
    }
    let outcomes: Vec<CasOutcome> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == CasOutcome::Committed)
            .count(),
        1,
        "exactly one CAS with the same base token may commit"
    );
}
