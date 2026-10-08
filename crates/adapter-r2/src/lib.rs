//! R2 content store (design §9.2 cloudflare profile): HEAD / revisions /
//! blobs live in one bucket under the same key layout as the local
//! filesystem store — `vaults/{vault}/…` — so a raw bucket dump IS the
//! raw backup (design §5.3).
//!
//! CAS conditions on the HEAD object's storage ETag ([`ExpectedHead`],
//! design §22.1); create-only writes use `etagDoesNotMatch("*")` per the
//! design's binding example (examples/r2_cas.rs). Worker futures are
//! `!Send`, so every port method wraps its body in `worker::send::SendFuture`
//! — the sanctioned bridge for single-threaded Workers behind axum.

use std::future::Future;

use brains_domain::{DomainError, DomainResult, Head, HexId, Revision, VaultPath};
use brains_ports::{BlobStore, CasOutcome, ExpectedHead, HeadStore, RevisionStore, StoredHead};
use sha2::{Digest, Sha256};
use worker::send::SendFuture;
use worker::{Bucket, Conditional, HttpMetadata};

/// Cheap handle clone: the binding wraps a JsValue reference, and the vault
/// name is shared.
#[derive(Clone)]
pub struct R2ContentStore {
    bucket: Bucket,
    vault: String,
}

impl R2ContentStore {
    pub fn new(bucket: Bucket, vault: impl Into<String>) -> Self {
        Self {
            bucket,
            vault: vault.into(),
        }
    }

    fn head_key(&self, path: &VaultPath) -> String {
        format!("vaults/{}/heads/{}.json", self.vault, path.as_str())
    }

    fn heads_root(&self) -> String {
        format!("vaults/{}/heads/", self.vault)
    }

    fn revision_key(&self, revision: &HexId) -> String {
        format!("vaults/{}/revisions/{}.json", self.vault, revision.as_str())
    }
}

fn storage_error(what: &str, key: &str, e: worker::Error) -> DomainError {
    DomainError::StorageUnavailable(format!("{what} {key}: {e}"))
}

async fn read_json<T: serde::de::DeserializeOwned>(
    bucket: &Bucket,
    what: &str,
    key: &str,
) -> DomainResult<Option<T>> {
    let Some(object) = bucket
        .get(key)
        .execute()
        .await
        .map_err(|e| storage_error(what, key, e))?
    else {
        return Ok(None);
    };
    let bytes = object
        .body()
        .ok_or_else(|| DomainError::Internal(format!("{what} object without body: {key}")))?
        .bytes()
        .await
        .map_err(|e| storage_error(what, key, e))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| DomainError::Internal(format!("{what} corrupt {key}: {e}")))
}

/// The design's create-only condition (examples/r2_cas.rs): a fresh object
/// must not exist.
fn absent_condition() -> Conditional {
    Conditional {
        etag_does_not_match: Some("*".to_owned()),
        ..Default::default()
    }
}

fn json_metadata() -> HttpMetadata {
    HttpMetadata {
        content_type: Some("application/json".into()),
        cache_control: Some("no-store".into()),
        ..Default::default()
    }
}

impl HeadStore for R2ContentStore {
    fn get(
        &self,
        path: &VaultPath,
    ) -> impl Future<Output = DomainResult<Option<StoredHead>>> + Send {
        let bucket = self.bucket.clone();
        let key = self.head_key(path);
        SendFuture::new(async move {
            let object = bucket
                .get(&key)
                .execute()
                .await
                .map_err(|e| storage_error("head read", &key, e))?;
            let Some(object) = object else {
                return Ok(None);
            };
            let head: Head = serde_json::from_slice(
                &object
                    .body()
                    .ok_or_else(|| {
                        DomainError::Internal(format!("head object without body: {key}"))
                    })?
                    .bytes()
                    .await
                    .map_err(|e| storage_error("head bytes", &key, e))?,
            )
            .map_err(|e| DomainError::Internal(format!("head corrupt {key}: {e}")))?;
            Ok(Some(StoredHead {
                head,
                storage_token: object.etag(),
            }))
        })
    }

    fn cas(
        &self,
        path: &VaultPath,
        expected: &ExpectedHead,
        new_head: &Head,
    ) -> impl Future<Output = DomainResult<CasOutcome>> + Send {
        let bucket = self.bucket.clone();
        let key = self.head_key(path);
        let condition = match expected {
            ExpectedHead::Absent => absent_condition(),
            ExpectedHead::StorageToken(token) => Conditional {
                etag_matches: Some(token.to_owned()),
                ..Default::default()
            },
        };
        let body = serde_json::to_vec(new_head).expect("head serializes");
        SendFuture::new(async move {
            // `execute()` yields None exactly when the condition failed —
            // never treat that as a commit (design §22.1).
            let committed = bucket
                .put(&key, body)
                .http_metadata(json_metadata())
                .only_if(condition)
                .execute()
                .await
                .map_err(|e| storage_error("head cas", &key, e))?;
            Ok(match committed {
                Some(_) => CasOutcome::Committed,
                None => CasOutcome::PreconditionFailed,
            })
        })
    }

    fn list(
        &self,
        prefix: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<StoredHead>>> + Send {
        let bucket = self.bucket.clone();
        let heads_root = self.heads_root();
        // R2 lists keys in KEY order, and key = path + ".json" does not
        // preserve logical-path order for prefix pairs (`"a b"` sorts before
        // `"a.json"`). The port contract demands path order with a
        // strict-after path cursor, so: sweep the vault heads root (R2's own
        // cursor pages the raw scan), narrow with a SEGMENT-BOUNDARY filter
        // (a bare key prefix would leak `notes-backup/` into `prefix=notes`),
        // sort by path, then keep what is strictly after `after`.
        let scan_prefix = match prefix {
            Some(p) if !p.is_empty() => format!("{heads_root}{p}"),
            _ => heads_root.clone(),
        };
        let after = after.filter(|a| !a.is_empty()).map(str::to_owned);
        SendFuture::new(async move {
            let mut candidates: Vec<String> = Vec::new();
            let mut r2_cursor: Option<String> = None;
            loop {
                let mut options = bucket.list().prefix(scan_prefix.clone()).limit(1000);
                if let Some(cursor) = &r2_cursor {
                    options = options.cursor(cursor.clone());
                }
                let objects = options
                    .execute()
                    .await
                    .map_err(|e| storage_error("heads list", &scan_prefix, e))?;
                candidates.extend(objects.objects().iter().map(|o| o.key()));
                if !objects.truncated() {
                    break;
                }
                r2_cursor = objects.cursor();
            }

            let mut paths: Vec<VaultPath> = candidates
                .iter()
                .filter_map(|key| {
                    key.strip_prefix(&heads_root)
                        .and_then(|p| p.strip_suffix(".json"))
                        .map(str::to_owned)
                })
                // Segment-boundary match, identical to the filesystem adapter.
                .filter(|path| prefix.is_none_or(|p| brains_domain::path::prefix_has(p, path)))
                .filter(|path| after.as_deref().is_none_or(|a| path.as_str() > a))
                .map(VaultPath::trusted)
                .collect();
            paths.sort();
            paths.truncate(limit);

            let mut heads = Vec::with_capacity(paths.len());
            for path in &paths {
                // A head vanishing between scan and read is the filesystem
                // adapter's skip-and-continue case, not an error.
                if let Some(stored) = HeadStore::get(self, path).await? {
                    heads.push(stored);
                }
            }
            Ok(heads)
        })
    }
}

impl RevisionStore for R2ContentStore {
    fn create(&self, revision: &Revision) -> impl Future<Output = DomainResult<()>> + Send {
        let bucket = self.bucket.clone();
        let key = self.revision_key(&revision.revision);
        let body = serde_json::to_vec(revision).expect("revision serializes");
        SendFuture::new(async move {
            let created = bucket
                .put(&key, body)
                .http_metadata(json_metadata())
                .only_if(absent_condition())
                .execute()
                .await
                .map_err(|e| storage_error("revision create", &key, e))?;
            match created {
                Some(_) => Ok(()),
                // Create-only (design §22.1): a revision-ID collision is a
                // hard error, never a silent overwrite — same as the
                // filesystem adapter's create_new.
                None => Err(DomainError::Internal(format!(
                    "revision id collision: {key}"
                ))),
            }
        })
    }

    fn get(&self, revision: &HexId) -> impl Future<Output = DomainResult<Option<Revision>>> + Send {
        let bucket = self.bucket.clone();
        let key = self.revision_key(revision);
        SendFuture::new(async move { read_json::<Revision>(&bucket, "revision", &key).await })
    }
}

impl BlobStore for R2ContentStore {
    fn create(
        &self,
        key: &str,
        content: &[u8],
        sha256_hex: &str,
    ) -> impl Future<Output = DomainResult<()>> + Send {
        let bucket = self.bucket.clone();
        let key = key.to_owned();
        let content = content.to_vec();
        let expected = sha256_hex.to_owned();
        SendFuture::new(async move {
            let actual = hex::encode(Sha256::digest(&content));
            if actual != expected {
                return Err(DomainError::ChecksumMismatch { path: key.clone() });
            }
            let digest = hex::decode(&expected)
                .map_err(|e| DomainError::Internal(format!("declared digest malformed: {e}")))?;
            // R2 verifies the stored bytes against this checksum server-side
            // (design §22.3); the create-only condition rejects overwrites.
            let created = bucket
                .put(&key, content)
                .sha256(digest)
                .only_if(absent_condition())
                .execute()
                .await
                .map_err(|e| storage_error("blob create", &key, e))?;
            match created {
                Some(_) => Ok(()),
                None => Err(DomainError::StorageUnavailable(format!(
                    "immutable blob collision: {key}"
                ))),
            }
        })
    }

    fn get(&self, key: &str) -> impl Future<Output = DomainResult<Vec<u8>>> + Send {
        let bucket = self.bucket.clone();
        let key = key.to_owned();
        SendFuture::new(async move {
            let object = bucket
                .get(&key)
                .execute()
                .await
                .map_err(|e| storage_error("blob read", &key, e))?;
            let Some(object) = object else {
                return Err(DomainError::NotFound { path: key });
            };
            object
                .body()
                .ok_or_else(|| DomainError::Internal(format!("blob object without body: {key}")))?
                .bytes()
                .await
                .map_err(|e| storage_error("blob bytes", &key, e))
        })
    }
}
