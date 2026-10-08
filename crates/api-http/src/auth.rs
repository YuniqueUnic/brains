//! P0 authentication (design §15.1, §15.2): a file-backed token registry,
//! bearer extraction, short-lived signed session cookies and CSRF/Origin
//! checks for cookie-driven mutations. Tokens are stored only as SHA-256
//! hashes; plaintext exists solely at bootstrap and on the client.

use std::path::PathBuf;
use std::sync::RwLock;

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use brains_domain::{DomainError, DomainResult, Principal, Scope};

const SESSION_TTL_SECONDS: u64 = 8 * 3600;
pub const SESSION_COOKIE: &str = "brains_session";

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    pub token_sha256: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    /// Plaintext, stored ONLY for tokens this instance generated itself so a
    /// restart neither rotates nor loses the root credential. Tokens supplied
    /// via `--token`/`BRAINS_TOKEN` are recorded as hashes only (0600 file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryFile {
    pub tokens: Vec<TokenRecord>,
}

pub struct SecurityStore {
    path: PathBuf,
    registry: RwLock<RegistryFile>,
    signing_key: [u8; 32],
}

/// Signed session payload. `pub` so the Worker runtime can reuse the exact
/// same cookie format with a different key source (design §15.1).
#[derive(Serialize, Deserialize)]
pub struct SessionClaims {
    pub tid: String,
    pub exp: u64,
    pub nonce: String,
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Result<Vec<u8>, ()> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| ())
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// HMAC-SHA256 under an explicit key — shared by the file-backed store and
/// the Worker's secret-backed store.
pub fn sign_with_key(key: &[u8; 32], bytes: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(bytes);
    mac.finalize().into_bytes().to_vec()
}

/// Mint a signed session cookie value, its CSRF token and an expiry
/// timestamp (design §15.1). Key comes from the runtime's secret source.
pub fn issue_session_with_key(
    key: &[u8; 32],
    principal: &Principal,
) -> DomainResult<(String, String, String)> {
    let tid = principal
        .actor
        .strip_prefix("token:")
        .unwrap_or(&principal.actor)
        .to_owned();
    let nonce = brains_domain::HexId::generate().as_str().to_owned();
    let exp = now_unix() + SESSION_TTL_SECONDS;
    let claims = SessionClaims { tid, exp, nonce };
    let json = serde_json::to_vec(&claims).expect("serializes");
    let value = format!("{}.{}", b64(&json), b64(&sign_with_key(key, &json)));
    let csrf = b64(&sign_with_key(key, format!("csrf:{value}").as_bytes()));
    let expires_iso = iso_from_unix(exp);
    Ok((value, csrf, expires_iso))
}

/// Verify a signed session cookie under an explicit key.
pub fn verify_session_with_key(key: &[u8; 32], cookie_value: &str) -> DomainResult<SessionClaims> {
    let (encoded, sig) = cookie_value.split_once('.').ok_or_else(unauthenticated)?;
    let json = unb64(encoded).map_err(|_| unauthenticated())?;
    let given = unb64(sig).map_err(|_| unauthenticated())?;
    if sign_with_key(key, &json) != given {
        return Err(unauthenticated());
    }
    let claims: SessionClaims = serde_json::from_slice(&json).map_err(|_| unauthenticated())?;
    if claims.exp < now_unix() {
        return Err(unauthenticated());
    }
    Ok(claims)
}

/// Security backend shared by both runtimes: the local file-backed store and
/// the Worker's env-secret store (design §15.1). Object-safe so handlers can
/// hold `Arc<dyn Security>`.
pub trait Security: Send + Sync {
    fn authenticate(&self, token: &str) -> DomainResult<Principal>;
    fn authenticate_session(&self, cookie_value: &str) -> DomainResult<Principal>;
    fn issue_session(&self, principal: &Principal) -> DomainResult<(String, String, String)>;
    fn csrf_for(&self, cookie_value: &str) -> String;
}

impl Security for SecurityStore {
    fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        SecurityStore::authenticate(self, token)
    }
    fn authenticate_session(&self, cookie_value: &str) -> DomainResult<Principal> {
        SecurityStore::authenticate_session(self, cookie_value)
    }
    fn issue_session(&self, principal: &Principal) -> DomainResult<(String, String, String)> {
        SecurityStore::issue_session(self, principal)
    }
    fn csrf_for(&self, cookie_value: &str) -> String {
        SecurityStore::csrf_for(self, cookie_value)
    }
}

/// How the request authenticated — cookie-driven mutations need CSRF and
/// Origin validation, bearer clients do not (design §15.2).
#[derive(Clone)]
pub enum AuthSource {
    Bearer(Principal),
    Cookie(Principal, String),
}

impl AuthSource {
    pub fn from_request(
        security: &dyn Security,
        headers: &axum::http::HeaderMap,
    ) -> DomainResult<Self> {
        if let Some(auth) = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            let token = auth
                .strip_prefix("Bearer ")
                .or_else(|| auth.strip_prefix("bearer "));
            if let Some(token) = token {
                return Ok(Self::Bearer(security.authenticate(token.trim())?));
            }
        }
        if let Some(cookie) = cookie_value(headers, SESSION_COOKIE) {
            let principal = security.authenticate_session(&cookie)?;
            return Ok(Self::Cookie(principal, cookie));
        }
        Err(DomainError::Unauthenticated)
    }

    pub fn principal(&self) -> &Principal {
        match self {
            Self::Bearer(principal) | Self::Cookie(principal, _) => principal,
        }
    }

    pub fn cookie_value(&self) -> Option<&str> {
        match self {
            Self::Bearer(_) => None,
            Self::Cookie(_, value) => Some(value),
        }
    }
}

/// Read one cookie from the `Cookie` header.
pub fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=')
            && key.trim() == name
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// Cookie-driven mutations require a validated Origin and the session's CSRF
/// token (design §15.2). Bearer clients skip this path entirely.
pub fn guard_cookie_mutation(
    security: &dyn Security,
    headers: &axum::http::HeaderMap,
    cookie_value: &str,
    allowed_origins: &[String],
) -> DomainResult<()> {
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            DomainError::Forbidden("cookie mutations require an Origin header".into())
        })?;
    if !origin_allowed(origin, headers, allowed_origins) {
        return Err(DomainError::Forbidden(format!(
            "origin {origin} is not allowed"
        )));
    }
    let given = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| DomainError::Forbidden("missing CSRF token".into()))?;
    let expected = security.csrf_for(cookie_value);
    if !constant_time_eq(given.as_bytes(), expected.as_bytes()) {
        return Err(DomainError::Forbidden("CSRF token mismatch".into()));
    }
    Ok(())
}

pub fn origin_allowed(origin: &str, headers: &axum::http::HeaderMap, allowed: &[String]) -> bool {
    if allowed.iter().any(|o| o == origin) {
        return true;
    }
    // Same-origin mode: the Origin authority must equal the Host authority.
    headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|host| {
            origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
                .map(|o| o == host)
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

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
            let registry = RegistryFile { tokens: Vec::new() };
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
    /// so a reboot never orphans configured clients. Passing `Some(token)`
    /// registers that exact token (bootstrap convenience for CLI/e2e).
    pub fn ensure_root_token(&self, token: Option<&str>) -> DomainResult<(String, bool)> {
        let mut registry = self.registry.write().expect("registry lock");
        if let Some(existing) = registry.tokens.iter().find(|t| t.id == "default")
            && existing.enabled
            && token.is_none()
        {
            return Ok((existing.plaintext.clone().unwrap_or_default(), false));
        }
        let plaintext = match token {
            Some(t) if !t.is_empty() => (t.to_owned(), None),
            _ => {
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

    /// Authenticate a plaintext bearer token into a request principal.
    pub fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        let digest = hash_token(token);
        let registry = self.registry.read().expect("registry lock");
        let record = registry
            .tokens
            .iter()
            .find(|t| t.enabled && t.token_sha256 == digest)
            .ok_or(DomainError::Unauthenticated)?;
        let scopes = record
            .scopes
            .iter()
            .map(|s| Scope::parse(s))
            .collect::<DomainResult<Vec<_>>>()?;
        Ok(Principal {
            actor: format!("token:{}", record.id),
            scopes,
        })
    }

    /// Principal for a session cookie value; checks signature, expiry and
    /// that the token is still enabled (revocation must take effect).
    pub fn authenticate_session(&self, cookie_value: &str) -> DomainResult<Principal> {
        let claims = self.verify_session(cookie_value)?;
        let registry = self.registry.read().expect("registry lock");
        let record = registry
            .tokens
            .iter()
            .find(|t| t.id == claims.tid && t.enabled)
            .ok_or(DomainError::Unauthenticated)?;
        let scopes = record
            .scopes
            .iter()
            .map(|s| Scope::parse(s))
            .collect::<DomainResult<Vec<_>>>()?;
        Ok(Principal {
            actor: format!("token:{}", record.id),
            scopes,
        })
    }

    /// Exchange a verified principal (bearer-authenticated) for a signed
    /// session value plus its CSRF token.
    pub fn issue_session(&self, principal: &Principal) -> DomainResult<(String, String, String)> {
        issue_session_with_key(&self.signing_key, principal)
    }

    pub(crate) fn verify_session(&self, cookie_value: &str) -> DomainResult<SessionClaims> {
        verify_session_with_key(&self.signing_key, cookie_value)
    }

    /// The CSRF token bound to a session value.
    pub fn csrf_for(&self, cookie_value: &str) -> String {
        b64(&sign_with_key(
            &self.signing_key,
            format!("csrf:{cookie_value}").as_bytes(),
        ))
    }
}

fn unauthenticated() -> DomainError {
    DomainError::Unauthenticated
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> DomainResult<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| DomainError::Internal(format!("{}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| DomainError::Internal(format!("permissions: {e}")))?;
    }
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| DomainError::Internal(format!("{}: {e}", path.display())))
}

fn now_unix() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        // workerd has no platform clock for std::time; the JS clock is the
        // runtime's authoritative wall time.
        (js_sys::Date::now() / 1000.0) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

fn iso_from_unix(secs: u64) -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::from_unix_timestamp(secs as i64)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}
