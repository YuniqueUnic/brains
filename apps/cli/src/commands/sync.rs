//! Profile-pair sync commands and their local state file.

use std::path::{Path, PathBuf};

use brains_client::sync::{self, Resolution, SyncFailureStage, SyncState};
use brains_client::{BrainClient, ClientError, ClientResult};
use sha2::{Digest, Sha256};

use crate::cli::{SyncCommand, SyncSide};
use crate::config::Config;

struct Endpoint {
    profile: String,
    id: String,
    client: BrainClient,
}

struct Pair {
    endpoints: [Endpoint; 2],
    local_index: usize,
}

pub(crate) fn run(
    config: &Config,
    local_profile: Option<&str>,
    remote_profile: Option<&str>,
    dry_run: bool,
    action: Option<SyncCommand>,
) -> i32 {
    match run_inner(config, local_profile, remote_profile, dry_run, action) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("bsx: {error}");
            error.exit_code()
        }
    }
}

fn run_inner(
    config: &Config,
    local_profile: Option<&str>,
    remote_profile: Option<&str>,
    dry_run: bool,
    action: Option<SyncCommand>,
) -> ClientResult<i32> {
    if std::env::var_os("BRAINS_URL").is_some() || std::env::var_os("BRAINS_TOKEN").is_some() {
        return Err(invalid(
            "pair sync uses stored named profiles; unset BRAINS_URL and BRAINS_TOKEN",
        ));
    }
    let remote_profile = remote_profile
        .ok_or_else(|| invalid("pass --remote-profile <name> to select the other endpoint"))?;
    let local_profile = local_profile
        .or(config.active.as_deref())
        .unwrap_or("default");
    let pair = resolve_pair(config, local_profile, remote_profile)?;
    let path = state_path(&pair)?;
    if matches!(action, Some(SyncCommand::Verify)) {
        if dry_run {
            return Err(invalid("verify is already read-only; omit --dry-run"));
        }
        return verify_pair(&pair);
    }
    let mut state = load_state(&path, &pair.endpoint_ids())?;
    match action {
        None => synchronize(&pair, &path, state, dry_run),
        Some(SyncCommand::Verify) => unreachable!("verify runs without sync state"),
        Some(SyncCommand::Resolve {
            path: file_path,
            keep,
            merged_file,
            content_type,
        }) => {
            if dry_run {
                return Err(invalid("--dry-run applies to sync, not sync resolve"));
            }
            resolve_conflict(
                &pair,
                &path,
                &mut state,
                &file_path,
                keep,
                merged_file,
                content_type,
            )
        }
    }
}

fn resolve_pair(config: &Config, local: &str, remote: &str) -> ClientResult<Pair> {
    if local == remote {
        return Err(invalid(
            "local and remote profiles must have different names",
        ));
    }
    let (local_url, local_token) = config
        .resolve_named(local)
        .map_err(|message| invalid(&message))?;
    let (remote_url, remote_token) = config
        .resolve_named(remote)
        .map_err(|message| invalid(&message))?;
    let local_client = BrainClient::new(&local_url, &local_token);
    let remote_client = BrainClient::new(&remote_url, &remote_token);
    let local_id = endpoint_id(&local_client, &local_url)?;
    let remote_id = endpoint_id(&remote_client, &remote_url)?;
    if local_id == remote_id {
        return Err(invalid(
            "both profiles resolve to the same endpoint and vault",
        ));
    }
    let local_endpoint = Endpoint {
        profile: local.to_owned(),
        id: local_id,
        client: local_client,
    };
    let remote_endpoint = Endpoint {
        profile: remote.to_owned(),
        id: remote_id,
        client: remote_client,
    };
    if local_endpoint.id < remote_endpoint.id {
        Ok(Pair {
            endpoints: [local_endpoint, remote_endpoint],
            local_index: 0,
        })
    } else {
        Ok(Pair {
            endpoints: [remote_endpoint, local_endpoint],
            local_index: 1,
        })
    }
}

fn endpoint_id(client: &BrainClient, url: &str) -> ClientResult<String> {
    let capabilities = client.capabilities()?;
    let identity = format!(
        "{}\n{}",
        url.trim_end_matches('/'),
        capabilities.vault.as_deref().unwrap_or("default")
    );
    Ok(hex::encode(Sha256::digest(identity.as_bytes())))
}

fn synchronize(
    pair: &Pair,
    state_file: &Path,
    state: SyncState,
    dry_run: bool,
) -> ClientResult<i32> {
    let report = sync::synchronize(
        &pair.endpoints[0].client,
        &pair.endpoints[1].client,
        state,
        dry_run,
        |state| save_state(state_file, state),
    )?;
    let first = &pair.endpoints[0].profile;
    let second = &pair.endpoints[1].profile;
    let counts = report.counts;
    if dry_run {
        println!("dry run — no files or sync state were written");
    }
    println!("checked {} paths", counts.checked);
    println!(
        "copied/updated: {first} → {second}: {}, {second} → {first}: {}",
        counts.copied_first_to_second, counts.copied_second_to_first
    );
    println!(
        "merged: {}, trashed: {}, unchanged: {}, skipped after permanent purge: {}",
        counts.merged, counts.trashed, counts.unchanged, counts.skipped_purged
    );
    for conflict in &report.conflicts {
        println!(
            "conflict {}: {} ({first}={}, {second}={})",
            conflict.path,
            conflict.reason,
            conflict.first_revision.as_deref().unwrap_or("absent"),
            conflict.second_revision.as_deref().unwrap_or("absent")
        );
    }
    if let Some(failure) = &report.failure {
        let progress = match failure.stage {
            SyncFailureStage::Reconcile => {
                "this path may be partially written; earlier completed paths were checkpointed"
            }
            SyncFailureStage::Checkpoint => {
                "file changes may already be committed; this path's checkpoint was not saved; fix local state storage before retrying"
            }
        };
        eprintln!(
            "sync stopped at {}: {}; {progress}",
            failure.path, failure.error,
        );
        return Ok(failure.error.exit_code());
    }
    if !report.conflicts.is_empty() {
        println!(
            "resolve with `bsx --profile {first} sync --remote-profile {second} resolve <path> --keep local|remote` or `--merged-file <file>`"
        );
        return Ok(5);
    }
    Ok(0)
}

fn resolve_conflict(
    pair: &Pair,
    state_file: &Path,
    state: &mut SyncState,
    path: &str,
    keep: Option<SyncSide>,
    merged_file: Option<String>,
    content_type: Option<String>,
) -> ClientResult<i32> {
    let resolution = match (keep, merged_file) {
        (Some(side), None) => {
            let local = pair.local_index;
            let index = match side {
                SyncSide::Local => local,
                SyncSide::Remote => 1 - local,
            };
            Resolution::Keep(index)
        }
        (None, Some(file)) => {
            let bytes = std::fs::read(&file)
                .map_err(|error| ClientError::transport(format!("read {file}: {error}")))?;
            let media =
                content_type.unwrap_or_else(|| brains_domain::media_type_for(path).to_owned());
            sync::resolve(
                &pair.endpoints[0].client,
                &pair.endpoints[1].client,
                state,
                path,
                Resolution::Merged {
                    bytes: &bytes,
                    media_type: &media,
                    executable: brains_client::import::is_executable(std::path::Path::new(&file))?,
                },
            )?;
            save_state(state_file, state)?;
            println!("resolved {path} using {file}");
            return Ok(0);
        }
        _ => return Err(invalid("choose exactly one of --keep or --merged-file")),
    };
    sync::resolve(
        &pair.endpoints[0].client,
        &pair.endpoints[1].client,
        state,
        path,
        resolution,
    )?;
    save_state(state_file, state)?;
    println!("resolved {path}; both profiles now use the selected state");
    Ok(0)
}

fn state_path(pair: &Pair) -> ClientResult<PathBuf> {
    let ids = pair.endpoint_ids();
    let pair_hash = hex::encode(Sha256::digest(format!("{}\n{}", ids[0], ids[1])));
    let config = Config::path();
    let parent = config
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.join("sync").join(format!("{pair_hash}.json")))
}

fn load_state(path: &Path, endpoint_ids: &[String; 2]) -> ClientResult<SyncState> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SyncState::new(endpoint_ids.clone()));
        }
        Err(error) => return Err(ClientError::transport(error.to_string())),
    };
    let state: SyncState = serde_json::from_slice(&bytes)
        .map_err(|error| ClientError::transport(format!("invalid sync state: {error}")))?;
    state.validate(endpoint_ids)?;
    Ok(state)
}

fn save_state(path: &Path, state: &SyncState) -> ClientResult<()> {
    crate::config::write_json_atomic(path, state).map_err(ClientError::transport)
}

impl Pair {
    fn endpoint_ids(&self) -> [String; 2] {
        [self.endpoints[0].id.clone(), self.endpoints[1].id.clone()]
    }
}

fn invalid(message: &str) -> ClientError {
    ClientError {
        status: 400,
        code: Some("INVALID_ARGUMENT".into()),
        message: message.to_owned(),
        current_revision: None,
    }
}

fn verify_pair(pair: &Pair) -> ClientResult<i32> {
    let report = brains_client::verify::verify_current(
        &pair.endpoints[0].client,
        &pair.endpoints[1].client,
    )?;
    super::print_pretty(&report)?;
    println!(
        "Verified current live content only; non-live paths need not have matching tombstones. History requires archive verification."
    );
    Ok(if report.success() { 0 } else { 5 })
}

pub(crate) fn verify_before_use(config: &Config, source: &str, target: &str) -> ClientResult<()> {
    if std::env::var_os("BRAINS_URL").is_some() || std::env::var_os("BRAINS_TOKEN").is_some() {
        return Err(invalid(
            "unset BRAINS_URL and BRAINS_TOKEN before verified profile switching",
        ));
    }
    let pair = resolve_pair(config, source, target)?;
    if verify_pair(&pair)? != 0 {
        return Err(invalid("contents differ; active profile was not changed"));
    }
    let target = &pair.endpoints[1 - pair.local_index].client;
    if super::system::reindex(target, true, 5)? != 0 {
        return Err(invalid(
            "target reindex failed; active profile was not changed",
        ));
    }
    brains_client::verify::verify_index(target)?;
    // Re-check the complete stable observation window after rebuilding indexes.
    if verify_pair(&pair)? != 0 {
        return Err(invalid(
            "contents changed during verification; active profile was not changed",
        ));
    }
    Ok(())
}
