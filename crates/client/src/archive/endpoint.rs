use crate::{
    BrainClient, ClientResult,
    endpoint::{FileEndpoint, invalid},
    urlencode,
};
use brains_domain::{Head, Revision};

/// Administrative immutable-object transport. The same archive logic works
/// across native FS/SQLite and Worker R2/D1 without knowing either adapter.
pub struct ArchiveTarget {
    pub vault: String,
    pub max_blob_bytes: usize,
}

pub trait ArchiveEndpoint: FileEndpoint {
    fn archive_target(&self) -> ClientResult<ArchiveTarget>;
    fn archive_revision(&self, path: &str, revision: &str) -> ClientResult<Revision>;
    fn store_revision(&self, revision: &Revision) -> ClientResult<Revision>;
    fn store_blob(&self, revision: &Revision, bytes: &[u8]) -> ClientResult<()>;
    fn publish_head(&self, revision: &Revision) -> ClientResult<Head>;
    fn rebuild_index(&self) -> ClientResult<()>;
}

impl ArchiveEndpoint for BrainClient {
    fn archive_target(&self) -> ClientResult<ArchiveTarget> {
        let caps = self.capabilities()?;
        if caps.features.get("archive") != Some(&true) {
            return Err(invalid("target does not support archive transfer"));
        }
        Ok(ArchiveTarget {
            vault: caps
                .vault
                .ok_or_else(|| invalid("target did not identify its vault"))?,
            max_blob_bytes: caps.limits.max_attachment_bytes,
        })
    }
    fn archive_revision(&self, path: &str, revision: &str) -> ClientResult<Revision> {
        let query = format!("path={}&revision={}", urlencode(path), urlencode(revision));
        self.expect_json(
            self.send(
                self.http
                    .get(self.url("/api/v1/maintenance/archive/revision", &query)),
            )?,
        )
    }
    fn store_revision(&self, revision: &Revision) -> ClientResult<Revision> {
        self.expect_json(
            self.send(
                self.http
                    .post(self.url("/api/v1/maintenance/archive/revision", ""))
                    .json(revision),
            )?,
        )
    }
    fn store_blob(&self, revision: &Revision, bytes: &[u8]) -> ClientResult<()> {
        let query = format!(
            "path={}&revision={}",
            urlencode(&revision.path),
            revision.revision
        );
        let result: serde_json::Value = self.expect_json(
            self.send(
                self.http
                    .put(self.url("/api/v1/maintenance/archive/blob", &query))
                    .header("Content-Type", "application/octet-stream")
                    .body(bytes.to_vec()),
            )?,
        )?;
        if result.get("stored") != Some(&serde_json::Value::Bool(true)) {
            return Err(invalid("blob was not acknowledged"));
        }
        Ok(())
    }
    fn publish_head(&self, revision: &Revision) -> ClientResult<Head> {
        #[derive(serde::Deserialize)]
        struct Published {
            head: Head,
        }
        let query = format!(
            "path={}&revision={}",
            urlencode(&revision.path),
            revision.revision
        );
        let result: Published = self.expect_json(
            self.send(
                self.http
                    .post(self.url("/api/v1/maintenance/archive/head", &query)),
            )?,
        )?;
        Ok(result.head)
    }
    fn rebuild_index(&self) -> ClientResult<()> {
        crate::verify::rebuild_index(self)
    }
}
