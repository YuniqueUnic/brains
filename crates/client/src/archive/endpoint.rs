use crate::{
    BrainClient, ClientResult, Library,
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
        // HTTP optionals follow the generated contract. The archive file and
        // immutable storage record keep domain serde's explicit null values.
        let body = brains_sdk::ArchiveRevision {
            schema_version: revision.schema_version,
            revision: revision.revision.to_string(),
            path: revision.path.clone(),
            generation: revision.generation,
            parent: revision.parent.as_ref().map(ToString::to_string),
            action: revision.action.as_str().to_owned(),
            actor: revision.actor.clone(),
            operation_id: revision.operation_id.to_string(),
            created_at: revision.created_at.clone(),
            restores_revision: revision.restores_revision.as_ref().map(ToString::to_string),
            copied_from: revision
                .copied_from
                .as_ref()
                .map(|copy| brains_sdk::CopiedFrom {
                    path: copy.path.clone(),
                    revision: copy.revision.to_string(),
                }),
            content: revision
                .content
                .as_ref()
                .map(|content| brains_sdk::ArchiveContent {
                    key: content.key.clone(),
                    sha256: content.sha256.clone(),
                    size: content.size,
                    media_type: content.media_type.clone(),
                    executable: content.executable,
                }),
        };
        self.expect_json(
            self.send(
                self.http
                    .post(self.url("/api/v1/maintenance/archive/revision", ""))
                    .json(&body),
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
