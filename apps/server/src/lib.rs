//! brains-server library: the local-rust runtime profile — filesystem +
//! SQLite adapters and the file-backed security store, wired into the shared
//! axum layer from `brains-api-http`. Exposed so integration tests can boot
//! the exact production stack in-process.

use std::path::PathBuf;
use std::sync::Arc;

use brains_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore, FsStore};
use brains_adapter_sqlite::SqliteIndex;
use brains_api_http::auth::SecurityStore;
use brains_api_http::state::{AppState, Blob, Head, Idx, Rev};
use brains_application::Vault;
use brains_domain::{CursorSigner, Limits};

pub use brains_api_http::error;
pub use brains_api_http::routes;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub data_dir: PathBuf,
    pub vault: String,
    pub bind: String,
    pub port: u16,
    /// Register (or replace) a whole-vault root token; `None` generates one.
    pub token: Option<String>,
    pub web_dist: Option<PathBuf>,
    pub allowed_origins: Vec<String>,
    /// Lay down the template notes when the vault is completely empty.
    /// Contract tests that need a pristine corpus turn this off.
    pub seed_templates: bool,
}

impl ServerConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            vault: "default".into(),
            bind: "127.0.0.1".into(),
            port: 45800,
            token: None,
            web_dist: None,
            allowed_origins: Vec::new(),
            seed_templates: true,
        }
    }
}

pub type LocalVault = Vault<FsHeadStore, FsRevisionStore, FsBlobStore, SqliteIndex>;
pub type LocalState = AppState<FsHeadStore, FsRevisionStore, FsBlobStore, SqliteIndex>;
pub type SharedState = Arc<LocalState>;

/// Build the complete local application state against durable storage.
/// Returns the state, the root plaintext token and whether this boot
/// created/replaced it.
pub fn build_state(config: &ServerConfig) -> Result<(SharedState, String, bool), String> {
    let limits = Limits::default();
    let store = FsStore::create(&config.data_dir, &config.vault)
        .or_else(|_| FsStore::open(&config.data_dir, &config.vault))
        .map_err(|e| format!("store init failed: {e}"))?;
    let index = brains_adapter_sqlite::SqliteIndex::open(&config.data_dir.join("index.db"))
        .map_err(|e| format!("index init failed: {e}"))?;
    let security =
        SecurityStore::open(&config.data_dir).map_err(|e| format!("security init failed: {e}"))?;
    let (plaintext, token_new) = security
        .ensure_root_token(config.token.as_deref())
        .map_err(|e| format!("token bootstrap failed: {e}"))?;

    let heads = FsHeadStore::new(store.clone());
    let revisions = FsRevisionStore::new(store.clone());
    let blobs = FsBlobStore::new(store);
    let vault: LocalVault = Vault::new(
        heads,
        revisions,
        blobs,
        index,
        config.vault.clone(),
        limits.clone(),
        CursorSigner::generate(),
    );
    let capabilities = brains_domain::Capabilities::local(
        &config.vault,
        limits.max_text_bytes,
        limits.max_attachment_bytes,
    );
    let state: SharedState = Arc::new(AppState {
        vault,
        security: Arc::new(security),
        limits,
        capabilities,
        allowed_origins: config.allowed_origins.clone(),
        web_dist: config.web_dist.clone(),
    });
    Ok((state, plaintext, token_new))
}

/// Bind and serve until the process stops. Returns the bound address.
pub async fn serve(config: ServerConfig) -> Result<std::net::SocketAddr, String>
where
    FsHeadStore: Head,
    FsRevisionStore: Rev,
    FsBlobStore: Blob,
    SqliteIndex: Idx,
{
    let (state, plaintext, token_new) = build_state(&config)?;
    // First boot on a completely empty vault: lay down the template notes so
    // humans and AI agents start from guide content, not a blank screen.
    if config.seed_templates {
        match brains_application::seed::seed_if_empty(&state.vault).await {
            Ok(0) => {}
            Ok(count) => {
                eprintln!("brains-server: seeded {count} template notes into the empty vault.")
            }
            Err(e) => eprintln!("brains-server: template seeding failed (continuing): {e}"),
        }
    }
    // build_router owns the body limit (from limits) and the JSON fallbacks;
    // here we only attach the outer JSON 404 for the no-SPA profile.
    let router = routes::build_router(state);
    let router = match config.web_dist {
        Some(_) => router,
        None => router.fallback(brains_api_http::error::api_not_found),
    };
    let addr = format!("{}:{}", config.bind, config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("cannot bind {addr}: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    if config.token.is_none() && std::env::var("BRAINS_TOKEN").is_err() {
        if token_new {
            eprintln!("brains-server: generated root token (also stored in auth.json):");
            eprintln!("  {plaintext}");
            eprintln!("brains-server: pass it as `Authorization: Bearer <token>` or BRAINS_TOKEN.");
        } else {
            eprintln!("brains-server: existing root token kept (recovered from auth.json).");
        }
    }
    tracing::info!("listening on http://{local} (vault {:?})", config.vault);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("brains-server: serve failed: {e}");
        }
    });
    Ok(local)
}
