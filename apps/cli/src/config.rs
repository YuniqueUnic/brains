//! Named local libraries and remote connections. Environment URL/token overrides
//! apply only to remote profiles; they never turn a local operation into HTTP.
use brains_client::{BrainClient, ClientError, ClientResult, Library, local::LocalClient};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Local {
        data_dir: PathBuf,
        vault: String,
    },
    Remote {
        url: String,
        #[serde(default)]
        token: String,
    },
}
impl Profile {
    pub fn connect(&self) -> ClientResult<Box<dyn Library>> {
        match self {
            Self::Local { data_dir, vault } => Ok(Box::new(LocalClient::open(data_dir, vault)?)),
            Self::Remote { url, token } => Ok(Box::new(BrainClient::new(url, token))),
        }
    }
    pub fn identity(&self, vault: &str) -> ClientResult<String> {
        use sha2::{Digest, Sha256};
        let location = match self {
            Self::Local { data_dir, .. } => format!(
                "local:{}",
                data_dir
                    .canonicalize()
                    .map_err(|e| ClientError::transport(e.to_string()))?
                    .display()
            ),
            // Keep existing remote pair identities: tokens/profile names do not identify a library.
            Self::Remote { url, .. } => url.trim_end_matches('/').to_owned(),
        };
        Ok(hex::encode(Sha256::digest(format!("{location}\n{vault}"))))
    }
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
        std::env::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".brains")
            .join("config.json")
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

    pub fn resolve(&self, profile: Option<&str>) -> Result<Profile, String> {
        let url = std::env::var("BRAINS_URL").ok();
        let token = std::env::var("BRAINS_TOKEN").ok();
        let name = profile.or(self.active.as_deref()).unwrap_or("default");
        match self.profiles.get(name) {
            Some(Profile::Local { .. }) if url.is_some() || token.is_some() => Err("local mode does not accept BRAINS_URL/BRAINS_TOKEN; unset these variables or select a remote profile".into()),
            Some(Profile::Local { data_dir, vault }) => Ok(Profile::Local { data_dir: data_dir.clone(), vault: vault.clone() }),
            Some(Profile::Remote { url: stored_url, token: stored_token }) => Ok(Profile::Remote { url: url.unwrap_or_else(|| stored_url.clone()), token: token.unwrap_or_else(|| stored_token.clone()) }),
            None => match (url, token) {
                (Some(url), Some(token)) => Ok(Profile::Remote { url, token }),
                _ => Err(format!("no profile `{name}`; run `bsx init <directory>` for local use, or `bsx profile add {name} --url <URL>`")),
            }
        }
    }

    /// Pair operations require two explicit profiles and never apply environment overrides.
    pub fn resolve_named(&self, name: &str) -> Result<Profile, String> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| format!("no profile `{name}`; add it with `bsx profile add`"))?;
        if let Profile::Remote { url, token } = profile
            && (url.trim().is_empty() || token.is_empty())
        {
            return Err(format!(
                "profile `{name}` needs both URL and token before pair sync"
            ));
        }
        Ok(profile.clone())
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
        drop(file);
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
