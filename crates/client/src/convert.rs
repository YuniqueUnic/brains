//! Document attachment → Markdown companion notes (issue #19). The original
//! attachment is never modified; the companion is an ordinary, versioned
//! note at `<attachment>.md` whose frontmatter records provenance and keeps
//! `status: draft` until a person reviews it. Generic over the converter:
//! builds with the `convert` feature pass the embedded Xberg adapter, other
//! builds report `conversion: unavailable` before touching the store.

use brains_ports::DocumentConverter;

use crate::{ClientError, ClientResult, Library, Precondition};

pub struct ConvertReport {
    pub companion_path: String,
    pub companion_revision: String,
    pub converter: String,
    pub created: bool,
}

/// Convert `<path>` into a companion note. Re-converting (new original
/// revision or new converter output) writes a NEW revision of the SAME
/// companion path, never a new path.
pub async fn convert_attachment<C: DocumentConverter>(
    client: &dyn Library,
    converter: &C,
    path: &str,
    to: Option<&str>,
) -> ClientResult<ConvertReport> {
    let meta = client.get_meta(path)?;
    let media_type = meta
        .media_type
        .clone()
        .unwrap_or_else(|| brains_domain::media_type_for(path).to_owned());
    if !converter
        .supported_media_types()
        .contains(&media_type.as_str())
    {
        return Err(ClientError {
            status: 501,
            code: Some("CAPABILITY_UNSUPPORTED".into()),
            message: format!(
                "conversion: unavailable for {media_type}; supported: {}",
                converter.supported_media_types().join(", ")
            ),
            current_revision: None,
        });
    }
    let (bytes, _) = client.get_raw(path)?;
    let converted =
        futures::executor::block_on(converter.convert(&bytes, &media_type)).map_err(|error| {
            ClientError {
                status: 422,
                code: Some(error.code().into()),
                message: error.to_string(),
                current_revision: None,
            }
        })?;
    let companion = match to {
        Some(to) => to.to_owned(),
        None => format!("{path}.md"),
    };
    brains_domain::VaultPath::new(&companion).map_err(|error| ClientError {
        status: 400,
        code: Some(error.code().into()),
        message: error.to_string(),
        current_revision: None,
    })?;

    let (converter_id, converter_version) = converter.identity();
    let attachment = brains_domain::VaultPath::trusted(path.to_owned());
    let now = brains_domain::timestamp::now_rfc3339();
    let mut frontmatter = String::from("---\ntype: Converted Document\nstatus: draft\n");
    if let Some(title) = &converted.title {
        frontmatter.push_str(&format!("title: {}\n", yaml_scalar(title)));
    }
    frontmatter.push_str(&format!(
        "generated:\n  by: {converter_id}/{converter_version}\n  at: {now}\n"
    ));
    frontmatter.push_str("sources:\n  - id: original\n");
    frontmatter.push_str(&format!(
        "    resource: {}\n",
        yaml_scalar(attachment.as_str())
    ));
    if let Some(updated_at) = &meta.updated_at {
        frontmatter.push_str(&format!("    last_modified: \"{updated_at}\"\n"));
    }
    let sha256 = meta
        .sha256
        .clone()
        .unwrap_or_else(|| hex::encode(sha2::Sha256::digest(&bytes)));
    frontmatter.push_str(&format!("    sha256: {sha256}\n"));
    frontmatter.push_str("---\n\n");
    for warning in &converted.warnings {
        frontmatter.push_str(&format!("<!-- converter warning: {warning} -->\n"));
    }
    let title = converted
        .title
        .clone()
        .unwrap_or_else(|| attachment.file_name().to_owned());
    let body = format!("{frontmatter}# {title}\n\n{}\n", converted.markdown);

    // Re-conversion targets the same companion path with the usual CAS rules.
    let precondition = match client.get_meta(&companion) {
        Ok(existing) => Precondition::IfRevision(existing.revision),
        Err(ClientError { status: 404, .. }) => Precondition::Create,
        Err(ClientError { status: 410, .. }) => {
            return Err(ClientError {
                status: 410,
                code: Some("TRASHED".into()),
                message: format!("{companion} is trashed; restore it before re-converting"),
                current_revision: None,
            });
        }
        Err(error) => return Err(error),
    };
    let result = client.put_content(
        &companion,
        body.as_bytes(),
        "text/markdown",
        None,
        precondition,
        None,
    )?;
    Ok(ConvertReport {
        companion_path: companion,
        companion_revision: result.entry.revision,
        converter: format!("{converter_id}/{converter_version}"),
        created: result.entry.generation == 1,
    })
}

/// A frontmatter-safe double-quoted YAML scalar: JSON string escaping is a
/// subset of YAML double-quote escaping, so serializing via serde_json keeps
/// attacker-controlled titles (page `<title>`, PDF first lines) from
/// breaking the frontmatter. Shared with the URL importer.
pub(crate) fn yaml_scalar(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}

use sha2::Digest;
