//! `bsx update`: self-upgrade from the project's GitHub releases. Downloads
//! the release tarball for the running platform, verifies its published
//! sha256, and replaces the running binary in place (plus a sibling
//! `brains-server` when one sits next to it). Windows has no release
//! artifacts and is refused explicitly.

use brains_client::{ClientError, ClientResult};
use clap::Parser;
use sha2::{Digest, Sha256};

const REPO: &str = "YuniqueUnic/brains";
/// Release tarballs are far below this; the cap bounds a hijacked or
/// misconfigured download source, not a real artifact.
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
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

/// (release matrix id, tarball prefix) for the running platform.
fn release_target() -> ClientResult<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("macos-arm64"),
        ("macos", "x86_64") => Ok("macos-x64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
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
    tarball_url: String,
    tarball_name: String,
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
    let tarball_suffix = format!("-{target}.tar.gz");
    let mut tarball_url = None;
    let mut tarball_name = None;
    let mut sha256_url = None;
    for asset in body["assets"].as_array().into_iter().flatten() {
        let (Some(name), Some(download)) = (
            asset["name"].as_str(),
            asset["browser_download_url"].as_str(),
        ) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(&tarball_suffix) {
            let _ = stem;
            tarball_url = Some(download.to_owned());
            tarball_name = Some(name.to_owned());
            sha256_url = Some(format!("{download}.sha256"));
        }
    }
    let (Some(tarball_url), Some(tarball_name), Some(sha256_url)) =
        (tarball_url, tarball_name, sha256_url)
    else {
        return Err(ClientError::transport(format!(
            "release {tag} has no .tar.gz/.sha256 asset pair"
        )));
    };
    Ok(Release {
        tag,
        tarball_url,
        tarball_name,
        sha256_url,
    })
}

fn download(client: &reqwest::blocking::Client, url: &str, label: &str) -> ClientResult<Vec<u8>> {
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| ClientError::transport(format!("{label} download failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(ClientError::transport(format!(
            "{label} download failed ({status})"
        )));
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .read_to_end(&mut bytes)
        .map_err(|e| ClientError::transport(format!("{label} download failed: {e}")))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(ClientError::transport(format!(
            "{label} exceeds the {MAX_DOWNLOAD_BYTES}-byte download cap"
        )));
    }
    Ok(bytes)
}

fn verify_sha256(tarball: &[u8], digest_file: &[u8], tarball_name: &str) -> ClientResult<()> {
    let expected = std::str::from_utf8(digest_file)
        .map_err(|_| ClientError::transport("sha256 file is not UTF-8".to_owned()))?
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ClientError::transport(
            "sha256 file does not start with a hex digest".to_owned(),
        ));
    }
    let actual = hex::encode(Sha256::digest(tarball));
    if actual != expected {
        return Err(ClientError::transport(format!(
            "{tarball_name} failed the published sha256 check ({actual} != {expected}); refusing to install"
        )));
    }
    Ok(())
}

/// Extract `<prefix>/<name>` from the verified tar.gz into `destination`.
fn extract_member(
    tarball: &[u8],
    member_name: &str,
    destination: &std::path::Path,
) -> ClientResult<()> {
    let gz = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(gz);
    for entry in archive
        .entries()
        .map_err(|e| ClientError::transport(format!("tarball is not readable gzip: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| ClientError::transport(format!("tarball is corrupt: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| ClientError::transport(format!("tarball is corrupt: {e}")))?
            .into_owned();
        if path.as_os_str() != std::ffi::OsStr::new(member_name) {
            continue;
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| ClientError::transport(format!("{member_name} could not be read: {e}")))?;
        return std::fs::write(destination, &bytes)
            .map_err(|e| ClientError::transport(format!("could not write {destination:?}: {e}")));
    }
    Err(ClientError::transport(format!(
        "tarball does not contain {member_name}"
    )))
}

fn install_binary(bytes_path: &std::path::Path, target: &std::path::Path) -> ClientResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bytes_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| ClientError::transport(format!("could not chmod staged binary: {e}")))?;
    }
    // Rename over the running executable: atomic on Unix, and the old inode
    // stays alive until the process exits.
    std::fs::rename(bytes_path, target)
        .map_err(|e| ClientError::transport(format!("could not replace {target:?}: {e}")))
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

    println!("downloading {}", release.tarball_name);
    let tarball = download(&http, &release.tarball_url, "tarball")?;
    let digest_file = download(&http, &release.sha256_url, "sha256")?;
    verify_sha256(&tarball, &digest_file, &release.tarball_name)?;

    // The tarball lays binaries out under a prefix dir named after the
    // asset itself: brains-<version>-<target>/bsx.
    let prefix = release
        .tarball_name
        .strip_suffix(".tar.gz")
        .ok_or_else(|| ClientError::transport("unexpected tarball name".to_owned()))?
        .to_owned();
    let executable = std::env::current_exe()
        .map_err(|e| ClientError::transport(format!("could not locate the running bsx: {e}")))?;
    let bin_dir = executable.parent().ok_or_else(|| {
        ClientError::transport("running binary has no parent directory".to_owned())
    })?;

    let staged = bin_dir.join(format!(".bsx.update-{}", std::process::id()));
    extract_member(&tarball, &format!("{prefix}/bsx"), &staged)?;
    install_binary(&staged, &executable)?;
    println!("updated bsx -> {}", release.tag);

    // A brains-server sitting next to bsx (the release layout) is refreshed
    // from the same verified tarball so the pair cannot drift.
    let server = bin_dir.join("brains-server");
    if server.is_file() {
        let staged_server = bin_dir.join(format!(".brains-server.update-{}", std::process::id()));
        extract_member(&tarball, &format!("{prefix}/brains-server"), &staged_server)?;
        install_binary(&staged_server, &server)?;
        println!("updated brains-server -> {}", release.tag);
    }
    println!("restart any running bsx/brains-server processes to pick up the new version");
    Ok(0)
}
