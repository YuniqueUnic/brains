//! Shared handler state, generic over the storage adapters (design §9.2):
//! the same routes serve filesystem+SQLite locally and R2+D1 in the Worker.

use std::sync::Arc;

use brainstore_application::Vault;

use crate::auth::Security;

/// Compact bound aliases for handler signatures.
pub trait Head: brainstore_ports::HeadStore + Send + Sync + 'static {}
impl<T: brainstore_ports::HeadStore + Send + Sync + 'static> Head for T {}
pub trait Rev: brainstore_ports::RevisionStore + Send + Sync + 'static {}
impl<T: brainstore_ports::RevisionStore + Send + Sync + 'static> Rev for T {}
pub trait Blob: brainstore_ports::BlobStore + Send + Sync + 'static {}
impl<T: brainstore_ports::BlobStore + Send + Sync + 'static> Blob for T {}
pub trait Idx: brainstore_ports::IndexStore + Send + Sync + 'static {}
impl<T: brainstore_ports::IndexStore + Send + Sync + 'static> Idx for T {}

pub struct AppState<H, R, B, I> {
    pub vault: Vault<H, R, B, I>,
    pub security: Arc<dyn Security>,
    pub limits: brainstore_domain::Limits,
    pub capabilities: brainstore_domain::Capabilities,
    /// Allowed origins for cookie flows; empty means same-origin only.
    pub allowed_origins: Vec<String>,
    /// Local profile serves the SPA from disk; the Worker uses Static Assets.
    pub web_dist: Option<std::path::PathBuf>,
}

pub type SharedState<H, R, B, I> = Arc<AppState<H, R, B, I>>;
