//! `bsx import-url` (issue #22): fetch a web page through the ladder
//! (markdown negotiation → static HTML → rendered HTML → document) and store
//! it as a self-contained note with provenance frontmatter. The effective
//! policy is strict by default; every relaxation is an explicit flag.

#[cfg(feature = "url-import")]
use brains_client::url_import::{self, NetworkPolicy, UrlImportInput};
use brains_client::{ClientResult, Library};

#[allow(clippy::too_many_arguments)]
#[cfg(feature = "url-import")]
pub(crate) fn import_url_command(
    client: &dyn Library,
    url: &str,
    path: &str,
    render: bool,
    allow_http: bool,
    allow_host: Vec<String>,
    ignore_robots: bool,
    timeout_secs: u64,
    max_bytes: usize,
) -> ClientResult<i32> {
    if !render
        && !allow_http
        && allow_host.is_empty()
        && !ignore_robots
        && timeout_secs == 30
        && max_bytes == 10 * 1024 * 1024
        && client.capabilities()?.features.get("remote_url_import") == Some(&true)
    {
        let result = client.import_public_url(url, path)?;
        if result.partial {
            eprintln!(
                "bsx: source preserved at {} ({}); note not saved: {}",
                result.source_path.as_deref().unwrap_or("unknown"),
                result.source_revision.as_deref().unwrap_or("unknown"),
                result
                    .error
                    .as_ref()
                    .map(|error| error.message.as_str())
                    .unwrap_or("conversion incomplete")
            );
            return Ok(1);
        }
        println!(
            "imported {} -> {} ({})",
            result.source_url,
            result.path,
            if result.unchanged {
                "unchanged"
            } else if result.created {
                "created"
            } else {
                "new revision"
            }
        );
        return Ok(0);
    }
    let mut policy = NetworkPolicy {
        allow_hosts: allow_host,
        respect_robots: !ignore_robots,
        max_bytes,
        timeout_secs,
        ..NetworkPolicy::default()
    };
    if allow_http {
        policy.allowed_schemes.push("http".into());
    }
    let result = url_import::import_url(
        client,
        UrlImportInput {
            url,
            path,
            policy,
            render,
            ignore_robots,
        },
    )?;
    println!(
        "imported {} via {} -> {} ({})",
        result.final_url,
        result.step.as_str(),
        result.note_path,
        if result.created {
            "created".to_owned()
        } else {
            format!("new revision {}", result.note_revision)
        }
    );
    if let Some(hint) = result.companion_hint {
        eprintln!("bsx: {hint}");
    }
    Ok(0)
}

#[cfg(not(feature = "url-import"))]
pub(crate) fn import_url_command(
    client: &dyn Library,
    url: &str,
    path: &str,
    _render: bool,
    _allow_http: bool,
    _allow_host: Vec<String>,
    _ignore_robots: bool,
    _timeout_secs: u64,
    _max_bytes: usize,
) -> ClientResult<i32> {
    if client.capabilities()?.features.get("remote_url_import") == Some(&true) {
        if _render
            || _allow_http
            || !_allow_host.is_empty()
            || _ignore_robots
            || _timeout_secs != 30
            || _max_bytes != 10 * 1024 * 1024
        {
            return Err(brains_client::ClientError {
                status: 400,
                code: Some("INVALID_ARGUMENT".into()),
                message: "custom network flags require the native URL-import feature".into(),
                current_revision: None,
            });
        }
        let result = client.import_public_url(url, path)?;
        if result.partial {
            eprintln!(
                "bsx: source preserved at {} ({}); note not saved: {}",
                result.source_path.as_deref().unwrap_or("unknown"),
                result.source_revision.as_deref().unwrap_or("unknown"),
                result
                    .error
                    .as_ref()
                    .map(|error| error.message.as_str())
                    .unwrap_or("conversion incomplete")
            );
            return Ok(1);
        }
        println!("imported {} -> {}", result.source_url, result.path);
        return Ok(0);
    }
    eprintln!("bsx: import-url: unavailable — this build has no URL import support");
    Ok(9)
}
