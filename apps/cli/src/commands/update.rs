//! `bsx update`: self-upgrade from the project's GitHub releases. Downloads
//! the release archive for the running platform, verifies its published
//! sha256, and replaces the running binary in place (plus a sibling
//! `brains-server` when one sits next to it).

mod artifact;
use artifact::{download, extract_member, install_binary, verify_sha256};

use brains_client::{ClientError, ClientResult};
use clap::Parser;

const REPO: &str = "YuniqueUnic/brains";
const USER_AGENT: &str = concat!("bsx/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Parser)]
pub(crate) struct UpdateArgs {
    /// Report the newest release and exit without changing anything.
    #[arg(long)]
    check: bool,
    /// Upgrade to this release tag instead of the latest (e.g. v0.4.0).
    #[arg(long)]
    version: Option<String>,
    /// Replace the binaries without asking for confirmation.
    #[arg(long)]
    yes: bool,
}

/// Release matrix ID for the running platform.
fn release_target() -> ClientResult<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("macos-arm64"),
        ("macos", "x86_64") => Ok("macos-x64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("windows", "x86_64") => Ok("windows-x64"),
        (os, arch) => Err(ClientError {
            status: 400,
            code: Some("UNSUPPORTED_PLATFORM".into()),
            message: format!("no release artifact for {os}/{arch}; install from source with cargo"),
            current_revision: None,
        }),
    }
}

/// Numeric core of a `vX.Y.Z` tag or the build version (which carries a
/// ` (git …)` suffix in both development and release builds).
fn parse_version(value: &str) -> ClientResult<(u64, u64, u64)> {
    let core = value
        .split_whitespace()
        .next()
        .unwrap_or(value)
        .trim()
        .trim_start_matches('v');
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(ClientError::transport(format!(
            "unrecognized version `{value}`"
        )));
    }
    let mut numbers = [0u64; 3];
    for (slot, part) in parts.iter().enumerate() {
        numbers[slot] = part
            .parse()
            .map_err(|_| ClientError::transport(format!("unrecognized version `{value}`")))?;
    }
    Ok((numbers[0], numbers[1], numbers[2]))
}

struct Release {
    tag: String,
    archive_url: String,
    archive_name: String,
    sha256_url: String,
}

fn fetch_release(
    client: &reqwest::blocking::Client,
    tag: Option<&str>,
    target: &str,
) -> ClientResult<Release> {
    let route = match tag {
        Some(tag) => format!("tags/{tag}"),
        None => "latest".to_owned(),
    };
    let url = format!("https://api.github.com/repos/{REPO}/releases/{route}");
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|e| ClientError::transport(format!("release lookup failed: {e}")))?;
    let status = response.status();
    let body: serde_json::Value = response.json().map_err(|e| {
        ClientError::transport(format!("release lookup returned invalid JSON: {e}"))
    })?;
    if !status.is_success() {
        let message = body["message"].as_str().unwrap_or("no detail").to_owned();
        return Err(ClientError::transport(format!(
            "release lookup failed ({status}): {message}"
        )));
    }
    let tag = body["tag_name"]
        .as_str()
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| ClientError::transport("release has no tag_name".to_owned()))?
        .to_owned();
    let extension = if target.starts_with("windows-") {
        "zip"
    } else {
        "tar.gz"
    };
    let archive_name = format!(
        "brains-{}-{target}.{extension}",
        tag.trim_start_matches('v')
    );
    let asset_url = |name: &str| {
        body["assets"]
            .as_array()?
            .iter()
            .find(|asset| asset["name"].as_str() == Some(name))?["browser_download_url"]
            .as_str()
            .map(str::to_owned)
    };
    let (Some(archive_url), Some(sha256_url)) = (
        asset_url(&archive_name),
        asset_url(&format!("{archive_name}.sha256")),
    ) else {
        return Err(ClientError::transport(format!(
            "release {tag} has no {archive_name}/.sha256 asset pair"
        )));
    };
    Ok(Release {
        tag,
        archive_url,
        archive_name,
        sha256_url,
    })
}

pub(crate) fn update(args: UpdateArgs) -> ClientResult<i32> {
    let target = release_target()?;
    let http = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| ClientError::transport(e.to_string()))?;

    let release = fetch_release(&http, args.version.as_deref(), target)?;
    let latest = parse_version(&release.tag)?;
    let current = parse_version(brains_build_info::VERSION)?;
    if args.check {
        match latest > current {
            true => println!(
                "bsx {} -> {} is available: https://github.com/{REPO}/releases/tag/{}",
                brains_build_info::VERSION,
                release.tag,
                release.tag
            ),
            false => println!(
                "bsx {} is up to date (latest release {})",
                brains_build_info::VERSION,
                release.tag
            ),
        }
        return Ok(0);
    }
    if latest <= current && args.version.is_none() {
        println!(
            "bsx {} is already the latest release ({}); nothing to do",
            brains_build_info::VERSION,
            release.tag
        );
        return Ok(0);
    }
    if !args.yes {
        println!(
            "Update bsx {} -> {}? [y/N]",
            brains_build_info::VERSION,
            release.tag
        );
        let mut answer = String::new();
        let read = std::io::stdin().read_line(&mut answer);
        match read {
            Ok(0) | Err(_) => {
                return Err(ClientError {
                    status: 400,
                    code: Some("CONFIRMATION_REQUIRED".into()),
                    message: "no interactive confirmation available; rerun with --yes".into(),
                    current_revision: None,
                });
            }
            Ok(_) => {}
        }
        let answer = answer.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!("aborted; bsx unchanged");
            return Ok(0);
        }
    }

    println!("downloading {}", release.archive_name);
    let archive = download(&http, &release.archive_url, "archive")?;
    let digest_file = download(&http, &release.sha256_url, "sha256")?;
    verify_sha256(&archive, &digest_file, &release.archive_name)?;

    let prefix = release
        .archive_name
        .strip_suffix(if cfg!(windows) { ".zip" } else { ".tar.gz" })
        .ok_or_else(|| ClientError::transport("unexpected archive name".to_owned()))?;
    let executable = std::env::current_exe()
        .map_err(|e| ClientError::transport(format!("could not locate the running bsx: {e}")))?;
    let bin_dir = executable.parent().ok_or_else(|| {
        ClientError::transport("running binary has no parent directory".to_owned())
    })?;
    let stage = tempfile::tempdir_in(bin_dir)
        .map_err(|e| ClientError::transport(format!("could not stage the update: {e}")))?;
    let cli_name = format!("bsx{}", std::env::consts::EXE_SUFFIX);
    let server_name = format!("brains-server{}", std::env::consts::EXE_SUFFIX);
    let staged_cli = stage.path().join(&cli_name);
    extract_member(&archive, &format!("{prefix}/{cli_name}"), &staged_cli)?;
    let server = bin_dir.join(&server_name);
    if server.is_file() {
        let staged_server = stage.path().join(&server_name);
        extract_member(&archive, &format!("{prefix}/{server_name}"), &staged_server)?;
        // Stop the server before replacing it on Windows. A busy server fails
        // here, leaving bsx unchanged and reporting the installation error.
        install_binary(&staged_server, &server, false)?;
        println!("updated brains-server -> {}", release.tag);
    }
    install_binary(&staged_cli, &executable, true)?;
    println!("updated bsx -> {}", release.tag);
    println!("restart any running bsx/brains-server processes to pick up the new version");
    Ok(0)
}
