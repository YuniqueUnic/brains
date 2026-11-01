//! Private native credential persistence.

use super::*;

impl SecurityStore {
    /// Load or create the registry at `data_dir/auth.json` and the signing
    /// key at `data_dir/secret.key` (both 0600 on creation).
    pub fn open(data_dir: &std::path::Path) -> DomainResult<Self> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| DomainError::Internal(format!("data dir: {e}")))?;
        let path = data_dir.join("auth.json");
        let registry = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| DomainError::Internal(format!("auth registry: {e}")))?;
            serde_json::from_str::<RegistryFile>(&raw)
                .map_err(|e| DomainError::Internal(format!("auth registry corrupt: {e}")))?
        } else {
            let registry = RegistryFile {
                tokens: Vec::new(),
                revoked_sessions: BTreeMap::new(),
            };
            write_private(
                &path,
                &serde_json::to_vec_pretty(&registry).expect("serializes"),
            )?;
            registry
        };
        let key_path = data_dir.join("secret.key");
        let signing_key: [u8; 32] = if key_path.exists() {
            let raw = std::fs::read(&key_path)
                .map_err(|e| DomainError::Internal(format!("secret key: {e}")))?;
            raw.try_into()
                .map_err(|_| DomainError::Internal("secret.key must be exactly 32 bytes".into()))?
        } else {
            use rand::RngExt;
            let mut key = [0u8; 32];
            rand::rng().fill(&mut key);
            write_private(&key_path, &key)?;
            key
        };
        Ok(Self {
            path,
            registry: RwLock::new(registry),
            signing_key,
        })
    }

    /// Idempotently ensure a whole-vault token exists. Returns the plaintext
    /// plus whether this call created/replaced it: `None` keeps any existing
    /// root token (recovering its stored plaintext) instead of rotating it,
    /// so a reboot never restores a superseded credential. `Some(token)`
    /// bootstraps an empty registry only; existing credentials are authoritative.
    pub fn ensure_root_token(&self, token: Option<&str>) -> DomainResult<(String, bool)> {
        let mut registry = self.registry.write().expect("registry lock");
        if let Some(existing) = registry.tokens.iter().find(|t| t.id == "default")
            && existing.enabled
        {
            return Ok((existing.plaintext.clone().unwrap_or_default(), false));
        }
        let plaintext = match token {
            Some(t) if t.is_empty() || t.chars().any(char::is_whitespace) => {
                return Err(DomainError::InvalidArgument(
                    "root token must be nonempty and contain no whitespace".into(),
                ));
            }
            Some(t) => (t.to_owned(), None),
            None => {
                let generated = brains_domain::HexId::generate().as_str().to_owned()
                    + brains_domain::HexId::generate().as_str();
                let stored = generated.clone();
                (generated, Some(stored))
            }
        };
        let record = TokenRecord {
            id: "default".into(),
            name: "owner".into(),
            token_sha256: hash_token(&plaintext.0),
            scopes: vec!["read:/".into(), "write:/".into(), "admin:/".into()],
            enabled: true,
            session_epoch: String::new(),
            plaintext: plaintext.1,
        };
        if let Some(slot) = registry.tokens.iter_mut().find(|t| t.id == record.id) {
            *slot = record;
        } else {
            registry.tokens.push(record);
        }
        self.persist(&registry)?;
        Ok((plaintext.0, true))
    }

    fn persist(&self, registry: &RegistryFile) -> DomainResult<()> {
        write_private(
            &self.path,
            &serde_json::to_vec_pretty(registry).expect("serializes"),
        )
    }

    /// Authenticate against the current locked registry, including revocations.
    pub fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        self.registry
            .read()
            .expect("registry lock")
            .authenticate(token)
    }

    pub fn authenticate_session(&self, cookie_value: &str) -> DomainResult<Principal> {
        self.registry
            .read()
            .expect("registry lock")
            .authenticate_session(&self.signing_key, cookie_value)
    }

    pub fn issue_session(&self, auth: &AuthSource) -> DomainResult<(String, String, String)> {
        self.registry
            .read()
            .expect("registry lock")
            .issue_session(&self.signing_key, auth)
    }

    pub(super) fn mutate(
        &self,
        mutation: impl FnOnce(&mut RegistryFile) -> DomainResult<()>,
    ) -> DomainResult<()> {
        let mut registry = self.registry.write().expect("registry lock");
        let mut next = registry.clone();
        mutation(&mut next)?;
        self.persist(&next)?;
        *registry = next;
        Ok(())
    }

    /// The CSRF token bound to a session value.
    pub fn csrf_for(&self, cookie_value: &str) -> String {
        b64(&sign_with_key(
            &self.signing_key,
            format!("csrf:{cookie_value}").as_bytes(),
        ))
    }
}
