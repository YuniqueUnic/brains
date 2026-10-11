//! Instance, profile, and maintenance commands.

use super::print_pretty;
use crate::cli::ProfileCommand;
use crate::config::{Config, Profile};
use brains_client::{ClientError, ClientResult, Library, Precondition};

pub(crate) fn capabilities(client: &dyn Library, json: bool) -> ClientResult<i32> {
    let caps = client.capabilities()?;
    if json {
        print_pretty(&caps)?;
    } else {
        println!("build_version {}", caps.build_version);
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

pub(crate) fn whoami(client: &dyn Library, json: bool) -> ClientResult<i32> {
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
        ProfileCommand::Add {
            name,
            url,
            token,
            data_dir,
            vault,
        } => {
            if config.profiles.contains_key(&name) {
                return Err(brains_domain::DomainError::InvalidArgument(format!(
                    "profile `{name}` already exists; use login to update its remote token"
                ))
                .into());
            }
            let profile = match (url, data_dir) {
                (Some(url), None) => Profile::Remote {
                    url,
                    token: token.unwrap_or_default(),
                },
                (None, Some(path)) => {
                    let data_dir = path
                        .canonicalize()
                        .map_err(|e| ClientError::transport(e.to_string()))?;
                    let profile = Profile::Local {
                        data_dir,
                        vault: vault.unwrap_or_else(|| "default".into()),
                    };
                    profile.connect()?;
                    profile
                }
                _ => {
                    return Err(brains_domain::DomainError::InvalidArgument(
                        "choose --url or --data-dir".into(),
                    )
                    .into());
                }
            };
            config.profiles.insert(name.clone(), profile);
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
                match profile {
                    Profile::Local { data_dir, vault } => println!(
                        "{marker} {name}  local  {}  vault={vault}",
                        data_dir.display()
                    ),
                    Profile::Remote { url, .. } => println!("{marker} {name}  remote  {url}"),
                }
            }
            Ok(0)
        }
        ProfileCommand::Use { name, verify_with } => {
            if !config.profiles.contains_key(&name) {
                return Err(ClientError {
                    status: 400,
                    code: Some("INVALID_ARGUMENT".into()),
                    message: format!("no profile `{name}`"),
                    current_revision: None,
                });
            }
            if let Some(source) = verify_with {
                super::sync::verify_before_use(config, &source, &name)?;
            }
            config.active = Some(name);
            config.save().map_err(ClientError::transport)?;
            Ok(0)
        }
    }
}

pub(crate) fn login(
    config: &mut Config,
    profile: Option<&str>,
    url: &str,
    stdin_token: bool,
) -> ClientResult<i32> {
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
    let name = profile
        .map(str::to_owned)
        .or_else(|| config.active.clone())
        .unwrap_or_else(|| "default".into());
    let entry = config
        .profiles
        .entry(name)
        .or_insert_with(|| Profile::Remote {
            url: url.to_owned(),
            token: String::new(),
        });
    match entry {
        Profile::Remote { token: stored, .. } => *stored = token,
        Profile::Local { .. } => {
            return Err(brains_domain::DomainError::InvalidArgument(
                "local profiles use filesystem access, not tokens".into(),
            )
            .into());
        }
    }
    config.save().map_err(ClientError::transport)?;
    eprintln!("token stored");
    Ok(0)
}

pub(crate) fn reindex(client: &dyn Library, all_pages: bool, limit: usize) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    let mut processed = 0i64;
    let mut failed = 0usize;
    loop {
        let page = client.reindex(cursor.as_deref(), limit)?;
        processed += page.processed;
        failed += page.failed.len();
        for failure in &page.failed {
            eprintln!("bsx: reindex failed {}: {}", failure.path, failure.code);
        }
        if let Some(next) = page.next_cursor.clone() {
            if all_pages {
                cursor = Some(next);
                continue;
            }
            eprintln!("bsx: more pages pending; rerun with --all-pages");
            return Ok(7);
        }
        break;
    }
    println!("reindexed {processed} files, {failed} failures");
    Ok(if failed == 0 { 0 } else { 7 })
}

pub(crate) fn export(
    client: &dyn Library,
    output: &std::path::Path,
    format: crate::cli::TransferFormat,
) -> ClientResult<i32> {
    let vault = client
        .capabilities()?
        .vault
        .unwrap_or_else(|| "default".into());
    let summary = match format {
        crate::cli::TransferFormat::WorkingTree => {
            brains_client::export::export_working_tree(client, output, &vault)?
        }
        crate::cli::TransferFormat::Archive => {
            brains_client::archive::export_archive(client, output, &vault)?
        }
    };
    println!("{summary}");
    Ok(0)
}

/// Push the built-in template notes into the target vault: existing paths are
/// skipped, missing ones are created. Idempotent — rerun anytime.
pub(crate) fn seed_template(client: &dyn Library) -> ClientResult<i32> {
    let mut written = 0usize;
    let mut skipped = 0usize;
    for (path, body) in brains_application::seed::template_notes() {
        match client.get_meta(path) {
            Ok(_) | Err(ClientError { status: 410, .. }) => skipped += 1,
            Err(error) if error.status == 404 => {
                client.put_content(
                    path,
                    body.as_bytes(),
                    "text/markdown",
                    None,
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

pub(crate) fn import(
    client: &dyn Library,
    input: &std::path::Path,
    format: crate::cli::TransferFormat,
    overwrite: bool,
    dry_run: bool,
) -> ClientResult<i32> {
    match format {
        crate::cli::TransferFormat::WorkingTree => {
            let report =
                brains_client::import::import_working_tree(client, input, overwrite, dry_run)?;
            print_pretty(&report)?;
            Ok(if report.success() { 0 } else { 5 })
        }
        crate::cli::TransferFormat::Archive => {
            if overwrite {
                return Err(ClientError {
                    status: 400,
                    code: Some("INVALID_ARGUMENT".into()),
                    message: "archive restore never overwrites different history; omit --overwrite"
                        .into(),
                    current_revision: None,
                });
            }
            println!(
                "{}",
                brains_client::archive::import_archive(client, input, dry_run)?
            );
            Ok(0)
        }
    }
}

/// Create an empty owned library and register its absolute location.
pub(crate) fn init(
    config: &mut Config,
    name: Option<&str>,
    directory: &std::path::Path,
    vault: String,
) -> ClientResult<i32> {
    let name = name.unwrap_or("local").to_owned();
    if config.profiles.contains_key(&name) {
        return Err(brains_domain::DomainError::InvalidArgument(format!(
            "profile `{name}` already exists; select a new --profile name"
        ))
        .into());
    }
    let client = brains_client::local::LocalClient::create(directory, &vault)?;
    let data_dir = directory
        .canonicalize()
        .map_err(|e| ClientError::transport(e.to_string()))?;
    config.profiles.insert(
        name.clone(),
        Profile::Local {
            data_dir: data_dir.clone(),
            vault,
        },
    );
    config.active = Some(name.clone());
    config.save().map_err(ClientError::transport)?;
    println!(
        "initialized {} (local profile `{name}`, now active)",
        data_dir.display()
    );
    println!(
        "Import documents with `bsx --profile {name} import <directory>`; no server or token required."
    );
    drop(client);
    Ok(0)
}
