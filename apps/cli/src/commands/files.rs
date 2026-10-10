//! Commands that read or mutate individual vault files.

use super::print_pretty;
use brains_client::{ClientError, ClientResult, Library, Precondition, TransferKind};

pub(crate) fn cat(client: &dyn Library, path: &str) -> ClientResult<i32> {
    let (bytes, _etag) = client.get_raw(path)?;
    use std::io::Write;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| ClientError::transport(e.to_string()))?;
    Ok(0)
}

pub(crate) fn stat(client: &dyn Library, path: &str, json: bool) -> ClientResult<i32> {
    let entry = client.get_meta(path)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&entry)
                .map_err(|e| ClientError::transport(e.to_string()))?
        );
    } else {
        println!("path        {}", entry.path);
        println!("revision    {}", entry.revision);
        println!("generation  {}", entry.generation);
        println!("state       {}", entry.state);
        if let Some(size) = entry.size {
            println!("size        {size}");
        }
        if let Some(kind) = &entry.kind {
            println!(
                "kind        {}",
                serde_json::to_value(kind).unwrap_or_default()
            );
        }
        if let Some(updated) = &entry.updated_at {
            println!("updated_at  {updated}");
        }
    }
    Ok(0)
}

pub(crate) fn put(
    client: &dyn Library,
    path: &str,
    from: Option<String>,
    create: bool,
    if_revision: Option<String>,
    content_type: Option<String>,
    operation_id: Option<String>,
) -> ClientResult<i32> {
    let precondition = match (create, if_revision) {
        (true, None) => Precondition::Create,
        (false, Some(rev)) => Precondition::IfRevision(rev),
        (true, Some(_)) => {
            return Err(ClientError {
                status: 400,
                code: Some("INVALID_ARGUMENT".into()),
                message: "use either --create or --if-revision, not both".into(),
                current_revision: None,
            });
        }
        (false, None) => {
            return Err(ClientError {
                status: 428,
                code: Some("PRECONDITION_REQUIRED".into()),
                message: "pass --create or --if-revision <REV>; refusing an unconditional write"
                    .into(),
                current_revision: None,
            });
        }
    };
    let bytes = match &from {
        Some(file) => {
            std::fs::read(file).map_err(|e| ClientError::transport(format!("read {file}: {e}")))?
        }
        None => {
            use std::io::Read;
            let mut buffer = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buffer)
                .map_err(|e| ClientError::transport(e.to_string()))?;
            buffer
        }
    };
    let media = content_type.unwrap_or_else(|| brains_domain::media_type_for(path).to_owned());
    let result = client.put_content(
        path,
        &bytes,
        &media,
        None,
        precondition,
        operation_id.as_deref(),
    )?;
    println!(
        "{} gen={} rev={}",
        if result.entry.generation == 1 {
            "created"
        } else {
            "updated"
        },
        result.entry.generation,
        result.entry.revision
    );
    Ok(0)
}

pub(crate) fn rm(client: &dyn Library, path: &str, if_revision: &str) -> ClientResult<i32> {
    let result = client.delete(path, if_revision, None)?;
    println!(
        "trashed gen={} rev={}",
        result.entry.generation, result.entry.revision
    );
    Ok(0)
}

pub(crate) fn history(client: &dyn Library, path: &str, json: bool) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    loop {
        let page = client.history(path, cursor.as_deref(), 20)?;
        for revision in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(revision)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else {
                println!(
                    "gen {:>4}  {}  {:8}  {}  {}",
                    revision.generation,
                    revision.created_at,
                    revision.action,
                    revision.revision,
                    revision.parent.as_deref().unwrap_or("-")
                );
            }
        }
        match page.next_cursor.clone() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(0)
}

pub(crate) fn show(
    client: &dyn Library,
    path: &str,
    revision: &str,
    json: bool,
) -> ClientResult<i32> {
    if json {
        print_pretty(&Library::revision_meta(client, path, revision)?)?;
        return Ok(0);
    }
    let bytes = client.revision_raw(path, revision)?;
    use std::io::Write;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| ClientError::transport(e.to_string()))?;
    Ok(0)
}

pub(crate) fn restore(
    client: &dyn Library,
    path: &str,
    revision: &str,
    if_current: &str,
) -> ClientResult<i32> {
    let result = Library::restore(client, path, revision, if_current, None)?;
    println!(
        "restored gen={} rev={}",
        result.entry.generation, result.entry.revision
    );
    Ok(0)
}

pub(crate) fn patch(
    client: &dyn Library,
    path: &str,
    if_revision: &str,
    old: &str,
    new: &str,
    dry_run: bool,
) -> ClientResult<i32> {
    let body = brains_api_types::PatchBody {
        edits: vec![brains_api_types::PatchEditDto {
            old_text: old.to_owned(),
            new_text: new.to_owned(),
            expected_matches: 1,
        }],
        dry_run,
    };
    let result = client.patch(path, if_revision, body, None)?;
    if let Some(committed) = result.committed {
        println!(
            "patched gen={} rev={}",
            committed.entry.generation, committed.entry.revision
        );
    } else {
        print!("{}", result.diff);
    }
    Ok(0)
}

pub(crate) fn transfer(
    client: &dyn Library,
    kind: TransferKind,
    source: &str,
    target: &str,
    if_revision: &str,
) -> ClientResult<i32> {
    let operation_id = brains_domain::HexId::generate().as_str().to_owned();
    let result = client.transfer(
        kind,
        brains_api_types::TransferBody {
            source: source.to_owned(),
            target: target.to_owned(),
            expected_source_revision: if_revision.to_owned(),
            operation_id,
        },
    )?;
    let verb = match kind {
        TransferKind::Copy => "copied",
        TransferKind::Move => "moved",
    };
    if result.partial {
        eprintln!(
            "bsx: partial move: target created, source NOT trashed; repair manually (operation {})",
            result.operation_id
        );
        return Ok(7);
    }
    println!(
        "{verb} target_gen_rev={}{}",
        result.target_revision.as_deref().unwrap_or("-"),
        if result.source_trashed {
            " source_trashed"
        } else {
            ""
        }
    );
    Ok(0)
}

pub(crate) fn diff(client: &dyn Library, path: &str, from: &str, to: &str) -> ClientResult<i32> {
    let result = client.diff(path, from, to)?;
    print!("{}", result.diff);
    Ok(0)
}

pub(crate) fn edit(client: &dyn Library, path: &str, if_revision: &str) -> ClientResult<i32> {
    let (bytes, _etag) = client.get_raw(path)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let temp =
        std::env::temp_dir().join(format!("brain-edit-{}", brains_domain::HexId::generate()));
    std::fs::write(&temp, &bytes).map_err(|e| ClientError::transport(e.to_string()))?;
    let status = std::process::Command::new(&editor)
        .arg(&temp)
        .status()
        .map_err(|e| ClientError::transport(format!("launch {editor}: {e}")))?;
    if !status.success() {
        eprintln!(
            "bsx: editor exited with {status}; edited bytes kept at {}",
            temp.display()
        );
        return Ok(8);
    }
    let edited = std::fs::read(&temp).map_err(|e| ClientError::transport(e.to_string()))?;
    if edited == bytes {
        eprintln!("bsx: no changes");
        let _ = std::fs::remove_file(&temp);
        return Ok(0);
    }
    let media = brains_domain::media_type_for(path).to_owned();
    match client.put_content(
        path,
        &edited,
        &media,
        None,
        Precondition::IfRevision(if_revision.to_owned()),
        None,
    ) {
        Ok(result) => {
            let _ = std::fs::remove_file(&temp);
            println!(
                "saved gen={} rev={}",
                result.entry.generation, result.entry.revision
            );
            Ok(0)
        }
        Err(error) => {
            // Keep the edited bytes locally on conflict; never auto-retry.
            eprintln!(
                "bsx: save failed ({}); edited bytes kept at {}",
                error,
                temp.display()
            );
            Err(error)
        }
    }
}
