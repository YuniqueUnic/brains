//! Instance, profile, and maintenance commands.

use super::print_pretty;
use crate::cli::ProfileCommand;
use crate::config::{Config, Profile};
use brains_client::{BrainClient, ClientError, ClientResult, Precondition};

pub(crate) fn capabilities(client: &BrainClient, json: bool) -> ClientResult<i32> {
    let caps = client.capabilities()?;
    if json {
        print_pretty(&caps)?;
    } else {
        println!("api_version   {}", caps.api_version);
        println!("store_format  {}", caps.store_format);
        println!(
            "runtime       {}",
            serde_json::to_value(caps.runtime).unwrap_or_default()
        );
        if let Some(vault) = &caps.vault {
            println!("vault         {vault}");
        }
        println!(
            "limits        text<={}B attachments<={}B",
            caps.limits.max_text_bytes, caps.limits.max_attachment_bytes
        );
        for (feature, enabled) in &caps.features {
            println!("feature       {feature}={enabled}");
        }
    }
    Ok(0)
}

pub(crate) fn whoami(client: &BrainClient, json: bool) -> ClientResult<i32> {
    let session = client.session()?;
    if json {
        print_pretty(&session)?;
    } else {
        println!("actor   {}", session.actor);
        println!("scopes  {}", session.scopes.join(", "));
    }
    Ok(0)
}

pub(crate) fn profile_command(config: &mut Config, command: ProfileCommand) -> ClientResult<i32> {
    match command {
        ProfileCommand::Add { name, url, token } => {
            config.profiles.insert(
                name.clone(),
                Profile {
                    url,
                    token: token.unwrap_or_default(),
                },
            );
            if config.active.is_none() {
                config.active = Some(name);
            }
            config.save().map_err(ClientError::transport)?;
            eprintln!("profile saved");
            Ok(0)
        }
        ProfileCommand::List => {
            for (name, profile) in &config.profiles {
                let marker = if config.active.as_deref() == Some(name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{marker} {name}  {}", profile.url);
            }
            Ok(0)
        }
        ProfileCommand::Use { name } => {
            if !config.profiles.contains_key(&name) {
                return Err(ClientError {
                    status: 400,
                    code: Some("INVALID_ARGUMENT".into()),
                    message: format!("no profile `{name}`"),
                    current_revision: None,
                });
            }
            config.active = Some(name);
            config.save().map_err(ClientError::transport)?;
            Ok(0)
        }
    }
}

pub(crate) fn login(config: &mut Config, url: &str, stdin_token: bool) -> ClientResult<i32> {
    if !stdin_token {
        return Err(ClientError {
            status: 400,
            code: Some("INVALID_ARGUMENT".into()),
            message: "pass --stdin-token; the CLI never reads tokens from argv history".into(),
            current_revision: None,
        });
    }
    let mut token = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut token)
        .map_err(|e| ClientError::transport(e.to_string()))?;
    let token = token.trim().to_owned();
    let name = config.active.clone().unwrap_or_else(|| "default".into());
    let entry = config.profiles.entry(name).or_insert_with(|| Profile {
        url: url.to_owned(),
        token: String::new(),
    });
    entry.token = token;
    config.save().map_err(ClientError::transport)?;
    eprintln!("token stored");
    Ok(0)
}

pub(crate) fn reindex(client: &BrainClient, all_pages: bool, limit: usize) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    let mut processed = 0i64;
    let mut failed = 0usize;
    loop {
        let page = client.reindex(cursor.as_deref(), limit)?;
        processed += page.processed;
        failed += page.failed.len();
        for failure in &page.failed {
            eprintln!("brain: reindex failed {}: {}", failure.path, failure.code);
        }
        if let Some(next) = page.next_cursor.clone() {
            if all_pages {
                cursor = Some(next);
                continue;
            }
            eprintln!("brain: more pages pending; rerun with --all-pages");
            return Ok(7);
        }
        break;
    }
    println!("reindexed {processed} files, {failed} failures");
    Ok(0)
}

pub(crate) fn export(client: &BrainClient, output: &str) -> ClientResult<i32> {
    let output = std::path::Path::new(output);
    let vault = client
        .capabilities()?
        .vault
        .unwrap_or_else(|| "default".into());
    let summary = brains_client::export::export_working_tree(client, output, &vault)?;
    println!("{summary}");
    Ok(0)
}

/// Push the built-in template notes into the target vault: existing paths are
/// skipped, missing ones are created. Idempotent — rerun anytime.
pub(crate) fn seed_template(client: &BrainClient) -> ClientResult<i32> {
    let mut written = 0usize;
    let mut skipped = 0usize;
    for (path, body) in brains_application::seed::template_notes() {
        match client.get_meta(path) {
            Ok(_) => skipped += 1,
            Err(error) if error.status == 404 => {
                client.put_content(
                    path,
                    body.as_bytes(),
                    "text/markdown",
                    Precondition::Create,
                    None,
                )?;
                written += 1;
                println!("seeded {path}");
            }
            Err(error) => return Err(error),
        }
    }
    println!("done: {written} seeded, {skipped} already present.");
    Ok(0)
}
