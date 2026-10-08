//! Profile storage for the CLI (design §12): `~/.brainstore/config.json`
//! keeps per-profile URL + token. `BRAIN_URL` / `BRAIN_TOKEN` override the
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
        PathBuf::from(home).join(".brainstore").join("config.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| e.to_string())?;
            use std::io::Write;
            file.write_all(
                serde_json::to_vec_pretty(self)
                    .map_err(|e| e.to_string())?
                    .as_slice(),
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        }
    }

    /// Resolve URL/token: env overrides, then the active profile.
    pub fn resolve(&self, profile: Option<&str>) -> Result<(String, String), String> {
        let env_url = std::env::var("BRAIN_URL").ok();
        let env_token = std::env::var("BRAIN_TOKEN").ok();
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
                    "no profile `{name}`; run `brain profile add {name} --url <URL>` or set BRAIN_URL/BRAIN_TOKEN"
                ))
            }
        }
    }
}
