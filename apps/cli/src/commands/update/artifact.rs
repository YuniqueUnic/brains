//! Verified release bytes, exact archive members, and platform installation.

use std::{io::Read, path::Path};

use brains_client::{ClientError, ClientResult};
use sha2::{Digest, Sha256};

const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    label: &str,
) -> ClientResult<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .map_err(|e| ClientError::transport(format!("{label} download failed: {e}")))?;
    if !response.status().is_success() {
        return Err(ClientError::transport(format!(
            "{label} download failed ({})",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ClientError::transport(format!("{label} download failed: {e}")))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(ClientError::transport(format!(
            "{label} exceeds the {MAX_DOWNLOAD_BYTES}-byte download cap"
        )));
    }
    Ok(bytes)
}

pub(super) fn verify_sha256(
    archive: &[u8],
    digest_file: &[u8],
    archive_name: &str,
) -> ClientResult<()> {
    let expected = std::str::from_utf8(digest_file)
        .map_err(|_| ClientError::transport("sha256 file is not UTF-8".to_owned()))?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ClientError::transport(
            "sha256 file does not start with a hex digest".to_owned(),
        ));
    }
    let actual = hex::encode(Sha256::digest(archive));
    if actual != expected {
        return Err(ClientError::transport(format!(
            "{archive_name} failed the published sha256 check ({actual} != {expected}); refusing to install"
        )));
    }
    Ok(())
}

/// Read only the requested member; archive paths never choose a disk location.
pub(super) fn extract_member(bytes: &[u8], member: &str, destination: &Path) -> ClientResult<()> {
    #[cfg(windows)]
    let result = (|| -> std::io::Result<()> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut entry = zip.by_name(member)?;
        let mut output = std::fs::File::create(destination)?;
        std::io::copy(&mut entry, &mut output)?;
        Ok(())
    })();
    #[cfg(not(windows))]
    let result = (|| -> std::io::Result<()> {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.path()?.as_ref() == Path::new(member) {
                let mut output = std::fs::File::create(destination)?;
                std::io::copy(&mut entry, &mut output)?;
                return Ok(());
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "member not found",
        ))
    })();
    result.map_err(|e| ClientError::transport(format!("could not extract {member}: {e}")))
}

pub(super) fn install_binary(
    staged: &Path,
    target: &Path,
    is_running_cli: bool,
) -> ClientResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| ClientError::transport(format!("could not chmod staged binary: {e}")))?;
    }
    #[cfg(windows)]
    if is_running_cli {
        return self_replace::self_replace(staged).map_err(|e| {
            ClientError::transport(format!("could not replace the running bsx: {e}"))
        });
    }
    #[cfg(not(windows))]
    let _ = is_running_cli;
    std::fs::rename(staged, target).map_err(|e| {
        ClientError::transport(format!(
            "could not replace {}: {e}; stop any running brains-server before updating",
            target.display()
        ))
    })
}
