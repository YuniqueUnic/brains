//! `bsx checkpoint` (issue #15): thin wrappers over cat/stat/put against the
//! conventional `<scope>/checkpoint.md` note. Restore works through the
//! existing restore command — a checkpoint is an ordinary versioned file.

use brains_client::{ClientError, ClientResult, Library, Precondition};

pub(crate) fn show(client: &dyn Library, scope: &str) -> ClientResult<i32> {
    let path = brains_application::checkpoint::checkpoint_path(scope).map_err(|e| ClientError {
        status: 400,
        code: Some(e.code().into()),
        message: e.to_string(),
        current_revision: None,
    })?;
    let meta = client.get_meta(path.as_str())?;
    let (bytes, _) = client.get_raw(path.as_str())?;
    let body = String::from_utf8_lossy(&bytes);
    println!("{body}");
    eprintln!(
        "checkpoint {} at revision {} (generation {})",
        path.as_str(),
        meta.revision,
        meta.generation
    );
    Ok(0)
}

/// Replace the `generated:` mapping of an existing frontmatter block.
fn refresh_generated_block(mut body: Vec<u8>, actor: &str, now: &str) -> Vec<u8> {
    if let Ok(text) = std::str::from_utf8(&body)
        && let Some(start) = text.find("generated:")
    {
        let after = &text[start + "generated:".len()..];
        let end = after
            .find("\n  at:")
            .and_then(|at| {
                after[at..]
                    .find('\n')
                    .map(|nl| start + "generated:".len() + at + nl)
            })
            .unwrap_or(start + "generated:".len());
        let replacement = format!("\ngenerated:\n  by: {actor}\n  at: {now}");
        let mut refreshed = String::with_capacity(text.len());
        refreshed.push_str(&text[..start]);
        refreshed.push_str(&replacement);
        refreshed.push_str(&text[end..]);
        return std::mem::replace(&mut body, refreshed.into_bytes());
    }
    body
}

pub(crate) fn update(
    client: &dyn Library,
    scope: &str,
    from: Option<&str>,
    if_revision: Option<&str>,
    create: bool,
) -> ClientResult<i32> {
    let path = brains_application::checkpoint::checkpoint_path(scope).map_err(|e| ClientError {
        status: 400,
        code: Some(e.code().into()),
        message: e.to_string(),
        current_revision: None,
    })?;
    let body = match from {
        Some(file) => std::fs::read(file).map_err(|e| ClientError::transport(e.to_string()))?,
        None => {
            use std::io::Read;
            let mut buffer = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buffer)
                .map_err(|e| ClientError::transport(e.to_string()))?;
            buffer
        }
    };
    // Prepend the conventional frontmatter unless the supplied body already
    // declares its own; the note stays an ordinary markdown file either way.
    // A supplied Checkpoint frontmatter gets its generated block refreshed:
    // `generated` describes who produced the CURRENT content (ai-guide §6).
    let body = if body.starts_with(b"---\n") || body.starts_with(b"---\r\n") {
        let actor = client
            .session()
            .map(|session| session.actor)
            .unwrap_or_else(|_| "human:unknown".into());
        let now = brains_domain::timestamp::now_rfc3339();
        refresh_generated_block(body, &actor, &now)
    } else {
        let actor = client
            .session()
            .map(|session| session.actor)
            .unwrap_or_else(|_| "human:unknown".into());
        let header = brains_application::checkpoint::checkpoint_frontmatter(
            &actor,
            &brains_domain::timestamp::now_rfc3339(),
        );
        let mut with_header = header.into_bytes();
        with_header.extend_from_slice(&body);
        with_header
    };
    let precondition = if create {
        Precondition::Create
    } else {
        match if_revision {
            Some(rev) => Precondition::IfRevision(rev.to_owned()),
            None => {
                return Err(ClientError {
                    status: 428,
                    code: Some("PRECONDITION_REQUIRED".into()),
                    message: "checkpoint update needs --if-revision <rev> (from `checkpoint show`) or --create"
                        .into(),
                    current_revision: None,
                });
            }
        }
    };
    let result = client.put_content(
        path.as_str(),
        &body,
        "text/markdown",
        None,
        precondition,
        None,
    )?;
    println!(
        "checkpoint {} written at revision {}",
        path.as_str(),
        result.entry.revision
    );
    Ok(0)
}
