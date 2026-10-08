//! Signed pagination cursors (design §11.1): bound to an endpoint, a filter
//! fingerprint and the auth context; HMAC-SHA256 verified so clients cannot
//! forge or tamper. A cursor is an opaque position, never an authorization
//! credential — every page re-checks permissions independently.

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::error::{DomainError, DomainResult};

type HmacSha256 = Hmac<Sha256>;

const ENDPOINT_FILES: &str = "/files";
const ENDPOINT_SEARCH: &str = "/search";
const ENDPOINT_REVISIONS: &str = "/revisions";
const ENDPOINT_LINKS: &str = "/links";
const ENDPOINT_REINDEX: &str = "/maintenance/reindex";

/// Endpoint identifiers, part of the signed payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Files,
    Search,
    Revisions,
    Links,
    Reindex,
}

impl Endpoint {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Files => ENDPOINT_FILES,
            Self::Search => ENDPOINT_SEARCH,
            Self::Revisions => ENDPOINT_REVISIONS,
            Self::Links => ENDPOINT_LINKS,
            Self::Reindex => ENDPOINT_REINDEX,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CursorPayload<'a> {
    e: &'a str,
    f: &'a str,
    p: &'a str,
}

#[derive(Clone)]
pub struct CursorSigner {
    key: [u8; 32],
}

impl CursorSigner {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Random per-process key: cursors do not survive a restart, which is a
    /// documented property, not a bug — clients restart pagination.
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        use rand::RngExt;
        rand::rng().fill(&mut key);
        Self::new(key)
    }

    fn sign_payload(&self, bytes: &[u8]) -> Vec<u8> {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac accepts any key length");
        mac.update(bytes);
        mac.finalize().into_bytes().to_vec()
    }

    /// `position` is caller-defined and opaque (last path, "anchor:parent", …);
    /// `filter` must change whenever the query parameters change.
    pub fn issue(&self, endpoint: Endpoint, filter: &str, position: &str) -> String {
        let payload = CursorPayload {
            e: endpoint.as_str(),
            f: filter,
            p: position,
        };
        let json = serde_json::to_vec(&payload).expect("payload serializes");
        let sig = self.sign_payload(&json);
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&json),
            URL_SAFE_NO_PAD.encode(sig)
        )
    }

    /// Verify signature, endpoint and filter; return the position.
    pub fn verify(&self, endpoint: Endpoint, filter: &str, cursor: &str) -> DomainResult<String> {
        let (encoded, sig) = cursor
            .split_once('.')
            .ok_or_else(|| DomainError::InvalidArgument("malformed cursor".into()))?;
        let json = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| DomainError::InvalidArgument("malformed cursor".into()))?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(sig)
            .map_err(|_| DomainError::InvalidArgument("malformed cursor".into()))?;
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac accepts any key length");
        mac.update(&json);
        mac.verify_slice(&sig_bytes)
            .map_err(|_| DomainError::InvalidArgument("cursor signature mismatch".into()))?;
        let payload: CursorPayload = serde_json::from_slice(&json)
            .map_err(|_| DomainError::InvalidArgument("malformed cursor".into()))?;
        if payload.e != endpoint.as_str() {
            return Err(DomainError::InvalidArgument(
                "cursor was issued for a different endpoint".into(),
            ));
        }
        if payload.f != filter {
            return Err(DomainError::InvalidArgument(
                "cursor does not match current filters".into(),
            ));
        }
        Ok(payload.p.to_owned())
    }
}

const URL_SAFE_NO_PAD: base64::engine::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;
