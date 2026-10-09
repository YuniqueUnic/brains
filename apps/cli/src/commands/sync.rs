//! Profile-pair sync commands and their local state file.

use std::path::{Path, PathBuf};

use brains_client::sync::{self, Resolution, SyncState};
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
    let mut state = load_state(&path, &pair.endpoint_ids())?;
    match action {
        None => synchronize(&pair, &path, state, dry_run),
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
    )?;
    if !dry_run {
        save_state(state_file, &report.state)?;
    }
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
    let parent = path
        .parent()
        .ok_or_else(|| invalid("sync state path has no parent directory"))?;
    std::fs::create_dir_all(parent).map_err(|error| ClientError::transport(error.to_string()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("sync state path has no filename"))?;
    let temporary = parent.join(format!(".{name}.tmp-{}", brains_domain::HexId::generate()));
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| ClientError::transport(error.to_string()))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_result = (|| {
        use std::io::Write;
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temporary);
        return Err(ClientError::transport(error.to_string()));
    }
    Ok(())
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
