//! Convert immutable source bytes into an ordinary conditional-write note.

use brains_domain::{AccessLevel, DomainError, DomainResult, HexId, Principal, VaultPath};
use brains_format::companion::{GeneratedNote, render_companion, stable_companion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Precondition, Vault, commit::CommitInput};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionOutcome {
    pub companion_path: String,
    pub companion_revision: String,
    pub converter: String,
    pub created: bool,
    pub unchanged: bool,
}

#[derive(Debug)]
pub struct UrlImportOutcome {
    pub path: String,
    pub revision: Option<String>,
    pub partial: bool,
    pub error: Option<DomainError>,
    pub source_revision: Option<String>,
    pub source_url: String,
    pub source_path: Option<String>,
    pub created: bool,
    pub unchanged: bool,
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    pub async fn convert_document(
        &self,
        principal: &Principal,
        source: &VaultPath,
        destination: Option<&str>,
    ) -> DomainResult<ConversionOutcome> {
        let destination = VaultPath::new(
            &destination
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{source}.md")),
        )?;
        if &destination == source {
            return Err(DomainError::InvalidArgument(
                "conversion destination must differ from its source".into(),
            ));
        }
        self.require_scope(principal, AccessLevel::Read, source)?;
        self.require_scope(principal, AccessLevel::Write, &destination)?;
        let converter = self
            .converter
            .as_ref()
            .ok_or(DomainError::CapabilityUnsupported("document_conversion"))?;
        let input = self.read_file(principal, source).await?;
        let content = input
            .revision
            .content
            .as_ref()
            .ok_or_else(|| DomainError::Internal("source has no content".into()))?;
        let media = brains_domain::media_type_essence(&content.media_type);
        if !converter.media_types().contains(&media) {
            return Err(DomainError::CapabilityUnsupported(
                "document_conversion_media_type",
            ));
        }
        let existing = self.companion_base(principal, &destination).await?;
        let converted = converter
            .convert(source.file_name(), &input.bytes, &media)
            .await?;
        if converted.markdown.trim().is_empty() {
            return Err(DomainError::InvalidFormat(
                "converter returned empty Markdown".into(),
            ));
        }
        let identity = format!("{}/{}", converter.identity().0, converter.identity().1);
        let now = brains_domain::timestamp::now_rfc3339();
        let title = converted.title.as_deref().unwrap_or(source.file_name());
        let body = render_companion(GeneratedNote {
            title,
            markdown: &converted.markdown,
            source: source.as_str(),
            revision: Some(input.head.revision.as_str()),
            sha256: &content.sha256,
            modified_at: Some(&input.head.updated_at),
            converter: &identity,
            generated_at: &now,
            stored_source: None,
            warnings: &converted.warnings,
        });
        self.save_companion(principal, &destination, body, identity, existing)
            .await
    }

    pub async fn import_public_url(
        &self,
        principal: &Principal,
        url: &str,
        destination: &VaultPath,
    ) -> DomainResult<UrlImportOutcome> {
        self.require_scope(principal, AccessLevel::Write, destination)?;
        let existing = self.companion_base(principal, destination).await?;
        let fetcher = self
            .fetcher
            .as_ref()
            .ok_or(DomainError::CapabilityUnsupported("url_import"))?;
        let converter = self
            .converter
            .as_ref()
            .ok_or(DomainError::CapabilityUnsupported("document_conversion"))?;
        let source = fetcher.fetch(url, self.limits.max_attachment_bytes).await?;
        let media = brains_domain::media_type_essence(&source.media_type);
        let hash = hex::encode(Sha256::digest(&source.bytes));
        let binary = !media.starts_with("text/")
            && !matches!(media.as_str(), "application/xml" | "application/json");
        let source_path = if binary {
            let path = VaultPath::new(&format!(
                "{destination}.sources/{hash}.{}",
                extension_for(&media)
            ))?;
            self.require_scope(principal, AccessLevel::Write, &path)?;
            Some(path)
        } else {
            None
        };
        let converted = converter
            .convert(
                &format!("source.{}", extension_for(&media)),
                &source.bytes,
                &media,
            )
            .await?;
        if converted.markdown.trim().is_empty() {
            return Err(DomainError::InvalidFormat(
                "URL has no convertible content".into(),
            ));
        }
        let source_revision = match &source_path {
            Some(path) => Some(
                self.save_url_source(principal, path, &source.bytes, &media)
                    .await?,
            ),
            None => None,
        };
        let identity = format!("{}/{}", converter.identity().0, converter.identity().1);
        let now = brains_domain::timestamp::now_rfc3339();
        let stored_source =
            source_path
                .as_ref()
                .zip(source_revision.as_ref())
                .map(|(path, revision)| brains_format::companion::SavedSource {
                    path: path.as_str(),
                    revision,
                });
        let body = render_companion(GeneratedNote {
            title: converted
                .title
                .as_deref()
                .unwrap_or(destination.file_name()),
            markdown: &converted.markdown,
            source: &source.url,
            revision: None,
            sha256: &hash,
            modified_at: None,
            converter: &identity,
            generated_at: &now,
            stored_source,
            warnings: &converted.warnings,
        });
        let outcome = self
            .save_companion(principal, destination, body, identity, existing)
            .await;
        let source_path = source_path.map(|path| path.as_str().to_owned());
        match outcome {
            Ok(outcome) => Ok(UrlImportOutcome {
                path: outcome.companion_path,
                revision: Some(outcome.companion_revision),
                source_url: source.url,
                source_path,
                source_revision,
                created: outcome.created,
                unchanged: outcome.unchanged,
                partial: false,
                error: None,
            }),
            Err(error) if source_path.is_some() => Ok(UrlImportOutcome {
                path: destination.as_str().into(),
                revision: None,
                source_url: source.url,
                source_path,
                source_revision,
                created: false,
                unchanged: false,
                partial: true,
                error: Some(error),
            }),
            Err(error) => Err(error),
        }
    }

    async fn save_url_source(
        &self,
        principal: &Principal,
        path: &VaultPath,
        bytes: &[u8],
        media: &str,
    ) -> DomainResult<String> {
        if let Some(existing) = self.companion_base(principal, path).await? {
            if existing.bytes == bytes {
                return Ok(existing.head.revision.as_str().into());
            }
            return Err(DomainError::InvalidArgument(
                "content-addressed source path contains different bytes".into(),
            ));
        }
        let saved = self
            .put_file(
                principal,
                CommitInput {
                    path,
                    content: bytes,
                    media_type: Some(media),
                    actor: principal.actor.clone(),
                    operation_id: HexId::generate(),
                    precondition: Precondition::Create,
                    executable: None,
                    copied_from: None,
                },
            )
            .await;
        match saved {
            Ok(outcome) => Ok(outcome.revision),
            Err(error @ DomainError::RevisionConflict { .. }) => {
                let current = self.read_file(principal, path).await?;
                if current.bytes == bytes {
                    Ok(current.head.revision.as_str().into())
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn companion_base(
        &self,
        principal: &Principal,
        path: &VaultPath,
    ) -> DomainResult<Option<crate::query::FileContent>> {
        // Regeneration compares existing bytes, so access must include read.
        match self.current_head(path).await? {
            None => Ok(None),
            Some(_) => self.read_file(principal, path).await.map(Some),
        }
    }

    async fn save_companion(
        &self,
        principal: &Principal,
        path: &VaultPath,
        body: String,
        converter: String,
        existing: Option<crate::query::FileContent>,
    ) -> DomainResult<ConversionOutcome> {
        if let Some(existing) = &existing
            && stable_companion(std::str::from_utf8(&existing.bytes).unwrap_or(""))
                == stable_companion(&body)
        {
            self.check_precondition(
                path,
                &Precondition::IfRevision(existing.head.revision.clone()),
            )
            .await?;
            return Ok(ConversionOutcome {
                companion_path: path.as_str().into(),
                companion_revision: existing.head.revision.as_str().into(),
                converter,
                created: false,
                unchanged: true,
            });
        }
        let precondition = existing.map_or(Precondition::Create, |file| {
            Precondition::IfRevision(file.head.revision)
        });
        let outcome = self
            .put_file(
                principal,
                CommitInput {
                    path,
                    content: body.as_bytes(),
                    media_type: Some("text/markdown"),
                    actor: principal.actor.clone(),
                    operation_id: HexId::generate(),
                    precondition,
                    executable: None,
                    copied_from: None,
                },
            )
            .await?;
        Ok(ConversionOutcome {
            companion_path: outcome.path,
            companion_revision: outcome.revision,
            converter,
            created: outcome.created,
            unchanged: false,
        })
    }
}

fn extension_for(media: &str) -> &str {
    match media {
        "text/html" => "html",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/markdown" => "md",
        "text/csv" => "csv",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        _ => "bin",
    }
}
