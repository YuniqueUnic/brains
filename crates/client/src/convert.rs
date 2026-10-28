//! Document attachment → Markdown companion notes (issue #19). The original
//! attachment is never modified; the companion is an ordinary, versioned
//! note at `<attachment>.md` whose frontmatter records provenance and keeps
//! `status: draft` until a person reviews it. Generic over the converter:
//! builds with the `convert` feature pass the embedded Xberg adapter, other
//! builds report `conversion: unavailable` before touching the store.

use brains_ports::DocumentConverter;

use crate::{ClientError, ClientResult, Library, Precondition};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConvertReport {
    pub companion_path: String,
    pub companion_revision: String,
    pub converter: String,
    pub created: bool,
    /// Regeneration produced the identical companion; nothing was written.
    pub unchanged: bool,
}

/// Convert `<path>` into a companion note. Re-converting (new original
/// revision or new converter output) writes a NEW revision of the SAME
/// companion path, never a new path; an unchanged regeneration reports the
/// existing revision and writes nothing.
pub async fn convert_attachment<C: DocumentConverter>(
    client: &dyn Library,
    converter: &C,
    path: &str,
    to: Option<&str>,
) -> ClientResult<ConvertReport> {
    let invalid = |error: brains_domain::DomainError| ClientError {
        status: 400,
        code: Some(error.code().into()),
        message: error.to_string(),
        current_revision: None,
    };
    let source = brains_domain::VaultPath::new(path).map_err(invalid)?;
    let target =
        brains_domain::VaultPath::new(to.unwrap_or(&format!("{path}.md"))).map_err(invalid)?;
    if source == target {
        return Err(ClientError {
            status: 400,
            code: Some("INVALID_ARGUMENT".into()),
            message: "conversion destination must differ from source".into(),
            current_revision: None,
        });
    }
    let (bytes, revision) = client.get_raw(path)?;
    let metadata = Library::revision_meta(client, path, &revision)?;
    let content = metadata
        .content
        .ok_or_else(|| ClientError::transport("source revision has no content".into()))?;
    let media = brains_domain::media_type_essence(&content.media_type);
    if !converter.supported_media_types().contains(&media.as_str()) {
        return Err(ClientError {
            status: 501,
            code: Some("CAPABILITY_UNSUPPORTED".into()),
            message: format!("conversion unavailable for {media}"),
            current_revision: None,
        });
    }
    let existing = match client.get_raw(target.as_str()) {
        Ok(existing) => Some(existing),
        Err(ClientError { status: 404, .. }) => None,
        Err(error) => return Err(error),
    };
    let converted = converter
        .convert(&bytes, &media)
        .await
        .map_err(|error| ClientError {
            status: 422,
            code: Some(error.code().into()),
            message: error.to_string(),
            current_revision: None,
        })?;
    if converted.markdown.trim().is_empty() {
        return Err(ClientError::transport(
            "converter returned empty Markdown".into(),
        ));
    }
    let identity = format!("{}/{}", converter.identity().0, converter.identity().1);
    let now = brains_domain::timestamp::now_rfc3339();
    let body =
        brains_format::companion::render_companion(brains_format::companion::GeneratedNote {
            title: converted.title.as_deref().unwrap_or(source.file_name()),
            markdown: &converted.markdown,
            source: path,
            revision: Some(&revision),
            sha256: &content.sha256,
            modified_at: Some(metadata.created_at.as_str()),
            converter: &identity,
            generated_at: &now,
            stored_source: None,
            warnings: &converted.warnings,
        });
    if let Some((raw, revision)) = &existing
        && brains_format::companion::stable_companion(&String::from_utf8_lossy(raw))
            == brains_format::companion::stable_companion(&body)
    {
        let current = client.get_raw(target.as_str())?.1;
        if current != *revision {
            return Err(ClientError {
                status: 412,
                code: Some("REVISION_CONFLICT".into()),
                message: "companion changed during conversion".into(),
                current_revision: Some(current),
            });
        }
        return Ok(ConvertReport {
            companion_path: target.as_str().into(),
            companion_revision: revision.clone(),
            converter: identity,
            created: false,
            unchanged: true,
        });
    }
    let precondition = existing.map_or(Precondition::Create, |(_, revision)| {
        Precondition::IfRevision(revision)
    });
    let result = client.put_content(
        target.as_str(),
        body.as_bytes(),
        "text/markdown",
        None,
        precondition,
        None,
    )?;
    Ok(ConvertReport {
        companion_path: target.as_str().into(),
        companion_revision: result.entry.revision,
        converter: identity,
        created: result.entry.generation == 1,
        unchanged: false,
    })
}

pub(crate) use brains_format::companion::yaml_scalar;
