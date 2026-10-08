use crate::FsBlobStore;
use brainstore_ports::BlobStore;
use sha2::{Digest, Sha256};

use super::temp_store;

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[tokio::test]
async fn create_is_create_only_and_verifies_digest() {
    let (_dir, store) = temp_store();
    let blobs = FsBlobStore::new(store);
    let key = "vaults/default/blobs/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/b.md";
    let body = b"hello vault";

    blobs.create(key, body, &digest(body)).await.unwrap();
    assert_eq!(blobs.get(key).await.unwrap(), body);

    // Digest mismatch is rejected and leaves nothing behind.
    assert!(blobs.create(key, body, &digest(b"other")).await.is_err());
    // Existing key is never overwritten.
    assert!(
        blobs
            .create(key, b"overwrite", &digest(b"overwrite"))
            .await
            .is_err()
    );
    assert_eq!(blobs.get(key).await.unwrap(), body);
}

#[tokio::test]
async fn reading_missing_blob_is_not_found() {
    let (_dir, store) = temp_store();
    let blobs = FsBlobStore::new(store);
    let err = blobs
        .get("vaults/default/blobs/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/gone.md")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        brainstore_domain::DomainError::NotFound { .. }
    ));
}
