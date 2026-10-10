//! Profile storage for the CLI (design §12): `~/.brains/config.json`
//! keeps per-profile URL + token. `BRAINS_URL` / `BRAINS_TOKEN` override the
//! active profile so scripts and CI stay explicit.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub url: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub active: Option<String>,
    pub profiles: std::collections::BTreeMap<String, Profile>,
}

impl Config {
    pub fn path() -> PathBuf {
        if let Ok(path) = std::env::var("BRAIN_CONFIG") {
            return PathBuf::from(path);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".brains").join("config.json")
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("read config {}: {error}", path.display())),
        };
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid config {}: {error}", path.display()))
    }

    pub fn save(&self) -> Result<(), String> {
        write_json_atomic(&Self::path(), self)
    }

    /// Resolve URL/token: env overrides, then the active profile.
    pub fn resolve(&self, profile: Option<&str>) -> Result<(String, String), String> {
        let env_url = std::env::var("BRAINS_URL").ok();
        let env_token = std::env::var("BRAINS_TOKEN").ok();
        let name = profile
            .map(str::to_owned)
            .or_else(|| self.active.clone())
            .unwrap_or_else(|| "default".into());
        match self.profiles.get(&name) {
            Some(profile) => Ok((
                env_url.unwrap_or_else(|| profile.url.clone()),
                env_token.unwrap_or_else(|| profile.token.clone()),
            )),
            None => {
                if let (Some(url), Some(token)) = (env_url, env_token) {
                    return Ok((url, token));
                }
                Err(format!(
                    "no profile `{name}`; run `bsx profile add {name} --url <URL>` or set BRAINS_URL/BRAINS_TOKEN"
                ))
            }
        }
    }

    /// Resolve a named profile without environment overrides. Pair commands
    /// need two independent endpoints, so a single BRAINS_URL/TOKEN override
    /// would make the selected pair ambiguous.
    pub fn resolve_named(&self, name: &str) -> Result<(String, String), String> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| format!("no profile `{name}`; add it with `bsx profile add`"))?;
        if profile.url.trim().is_empty() || profile.token.is_empty() {
            return Err(format!(
                "profile `{name}` needs both URL and token before pair sync"
            ));
        }
        Ok((profile.url.clone(), profile.token.clone()))
    }
}

/// Publish profile and sync metadata without truncating the last valid file.
pub(crate) fn write_json_atomic(
    path: &std::path::Path,
    value: &impl Serialize,
) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".brains-{}.tmp", brains_domain::HexId::generate()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("save {}: {error}", path.display()))
}
