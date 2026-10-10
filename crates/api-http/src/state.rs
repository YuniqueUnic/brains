//! Shared handler state, generic over the storage adapters (design §9.2):
//! the same routes serve filesystem+SQLite locally and R2+D1 in the Worker.

use std::sync::Arc;

use brains_application::Vault;

use crate::auth::{Security, SessionCookiePolicy};

/// Compact bound aliases for handler signatures.
pub trait Head: brains_ports::HeadStore + Send + Sync + 'static {}
impl<T: brains_ports::HeadStore + Send + Sync + 'static> Head for T {}
pub trait Rev: brains_ports::RevisionStore + Send + Sync + 'static {}
impl<T: brains_ports::RevisionStore + Send + Sync + 'static> Rev for T {}
pub trait Blob: brains_ports::BlobStore + Send + Sync + 'static {}
impl<T: brains_ports::BlobStore + Send + Sync + 'static> Blob for T {}
pub trait Idx: brains_ports::IndexStore + Send + Sync + 'static {}
impl<T: brains_ports::IndexStore + Send + Sync + 'static> Idx for T {}

pub struct AppState<H, R, B, I> {
    pub vault: Vault<H, R, B, I>,
    pub security: Arc<dyn Security>,
    pub session_cookie_policy: SessionCookiePolicy,
    pub limits: brains_domain::Limits,
    pub capabilities: brains_domain::Capabilities,
    /// Allowed origins for cookie flows; empty means same-origin only.
    pub allowed_origins: Vec<String>,
    /// Local profile serves the SPA from disk; the Worker uses Static Assets.
    pub web_dist: Option<std::path::PathBuf>,
}

pub type SharedState<H, R, B, I> = Arc<AppState<H, R, B, I>>;
