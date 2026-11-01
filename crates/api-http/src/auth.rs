//! P0 authentication (design §15.1, §15.2): a file-backed token registry,
//! bearer extraction, short-lived signed session cookies and CSRF/Origin
//! checks for cookie-driven mutations. Tokens are stored only as SHA-256
//! hashes; plaintext exists solely at bootstrap and on the client.

mod registry;
mod store;

use futures::future::BoxFuture;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use brains_domain::{DomainError, DomainResult, Principal, Scope};

const SESSION_TTL_SECONDS: u64 = 8 * 3600;
pub const SESSION_COOKIE: &str = "brains_session";

/// The runtime opts into plain HTTP only for direct loopback browser origins.
/// Public origins and the Worker always retain Secure cookies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionCookiePolicy {
    Secure,
    LoopbackHttp,
}

impl SessionCookiePolicy {
    pub(crate) fn cookie_header(
        self,
        value: &str,
        max_age: u64,
        headers: &axum::http::HeaderMap,
    ) -> String {
        let secure = if self.requires_secure(headers) {
            "; Secure"
        } else {
            ""
        };
        format!(
            "{SESSION_COOKIE}={value}; HttpOnly; SameSite=Lax{secure}; Path=/; Max-Age={max_age}"
        )
    }

    fn requires_secure(self, headers: &axum::http::HeaderMap) -> bool {
        use axum::http::{header, uri::Authority};

        if self == Self::Secure {
            return true;
        }
        let authority = headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .and_then(|origin| origin.strip_prefix("http://"))
            .and_then(|origin| origin.parse::<Authority>().ok());
        let Some(authority) = authority else {
            return true;
        };
        if authority.as_str().contains('@')
            || headers
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                != Some(authority.as_str())
        {
            return true;
        }
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        !(host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback()))
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    pub token_sha256: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    #[serde(default)]
    pub session_epoch: String,
    /// Plaintext, stored ONLY for tokens this instance generated itself so a
    /// restart neither rotates nor loses the root credential. Tokens supplied
    /// via `--token`/`BRAINS_TOKEN` are recorded as hashes only (0600 file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryFile {
    pub tokens: Vec<TokenRecord>,
    #[serde(default)]
    pub revoked_sessions: BTreeMap<String, u64>,
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
    #[serde(default)]
    pub epoch: String,
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
    epoch: &str,
) -> DomainResult<(String, String, String)> {
    let tid = principal
        .actor
        .strip_prefix("token:")
        .unwrap_or(&principal.actor)
        .to_owned();
    let nonce = brains_domain::HexId::generate().as_str().to_owned();
    let exp = now_unix() + SESSION_TTL_SECONDS;
    let claims = SessionClaims {
        tid,
        exp,
        nonce,
        epoch: epoch.to_owned(),
    };
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
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(&json);
    mac.verify_slice(&given).map_err(|_| unauthenticated())?;
    let claims: SessionClaims = serde_json::from_slice(&json).map_err(|_| unauthenticated())?;
    if claims.exp <= now_unix() {
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
    fn issue_session(&self, auth: &AuthSource) -> DomainResult<(String, String, String)>;
    fn csrf_for(&self, cookie_value: &str) -> String;
    fn rotate_token<'a>(
        &'a self,
        auth: &'a AuthSource,
        current: &'a str,
        replacement: &'a str,
    ) -> BoxFuture<'a, DomainResult<()>>;
    fn revoke_sessions<'a>(
        &'a self,
        auth: &'a AuthSource,
        all: bool,
    ) -> BoxFuture<'a, DomainResult<()>>;
}

impl Security for SecurityStore {
    fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        SecurityStore::authenticate(self, token)
    }
    fn authenticate_session(&self, cookie_value: &str) -> DomainResult<Principal> {
        SecurityStore::authenticate_session(self, cookie_value)
    }
    fn issue_session(&self, auth: &AuthSource) -> DomainResult<(String, String, String)> {
        SecurityStore::issue_session(self, auth)
    }
    fn csrf_for(&self, cookie_value: &str) -> String {
        SecurityStore::csrf_for(self, cookie_value)
    }
    fn rotate_token<'a>(
        &'a self,
        auth: &'a AuthSource,
        current: &'a str,
        replacement: &'a str,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            self.mutate(|registry| {
                registry.rotate_token(&self.signing_key, auth, current, replacement)
            })
        })
    }
    fn revoke_sessions<'a>(
        &'a self,
        auth: &'a AuthSource,
        all: bool,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            self.mutate(|registry| registry.revoke_sessions(&self.signing_key, auth, all))
        })
    }
}

/// How the request authenticated — cookie-driven mutations need CSRF and
/// Origin validation, bearer clients do not (design §15.2).
#[derive(Clone)]
pub enum AuthSource {
    Bearer(Principal, String),
    Cookie(Principal, String),
}

impl AuthSource {
    /// Authenticate and apply browser guards before vault/provider I/O. Both
    /// native middleware and the Worker entrypoint use the same policy.
    pub fn authorize_request(
        security: &dyn Security,
        headers: &axum::http::HeaderMap,
        method: &axum::http::Method,
        session_create: bool,
        allowed_origins: &[String],
    ) -> DomainResult<Self> {
        let source = Self::from_request(security, headers)?;
        if session_create {
            if let Some(origin) = headers
                .get(axum::http::header::ORIGIN)
                .and_then(|value| value.to_str().ok())
                && !origin_allowed(origin, headers, allowed_origins)
            {
                return Err(DomainError::Forbidden(format!(
                    "origin {origin} is not allowed"
                )));
            }
        } else if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE")
            && let Self::Cookie(_, cookie) = &source
        {
            guard_cookie_mutation(security, headers, cookie, allowed_origins)?;
        }
        Ok(source)
    }

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
                return Ok(Self::Bearer(
                    security.authenticate(token.trim())?,
                    token.trim().to_owned(),
                ));
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
            Self::Bearer(principal, _) | Self::Cookie(principal, _) => principal,
        }
    }

    pub fn cookie_value(&self) -> Option<&str> {
        match self {
            Self::Bearer(_, _) => None,
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

fn unauthenticated() -> DomainError {
    DomainError::Unauthenticated
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> DomainResult<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| DomainError::Internal("private file has no parent directory".into()))?;
    let temporary = parent.join(format!(
        ".auth-{}",
        brains_domain::HexId::generate().as_str()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        {
            let mut file = options.open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        // Preserve the original error; removal only cleans an unpublished file.
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| DomainError::Internal(format!("{}: {error}", path.display())))
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
