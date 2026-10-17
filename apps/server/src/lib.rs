//! brains-server library: the local-rust runtime profile — filesystem +
//! Turso adapters and the file-backed security store, wired into the shared
//! axum layer from `brains-api-http`. Exposed so integration tests can boot
//! the exact production stack in-process.

use std::path::PathBuf;
use std::sync::Arc;

use brains_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore};
use brains_adapter_turso::TursoIndex;
use brains_api_http::auth::SecurityStore;
use brains_api_http::state::{AppState, Blob, Head, Idx, Rev};
use brains_domain::Limits;

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
    /// Capabilities to stop advertising (empty = everything the build has).
    pub disable: Vec<String>,
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
            disable: Vec::new(),
        }
    }
}

pub type LocalState = AppState<FsHeadStore, FsRevisionStore, FsBlobStore, TursoIndex>;
pub type SharedState = Arc<LocalState>;

/// Build the complete local application state against durable storage.
/// Returns the state, the root plaintext token and whether this boot
/// created/replaced it.
pub fn build_state(config: &ServerConfig) -> Result<(SharedState, String, bool), String> {
    let limits = Limits::default();
    let vault = brains_local::open_or_create(&config.data_dir, &config.vault)
        .map_err(|e| format!("store init failed: {e}"))?;
    let security =
        SecurityStore::open(&config.data_dir).map_err(|e| format!("security init failed: {e}"))?;
    let (plaintext, token_new) = security
        .ensure_root_token(config.token.as_deref())
        .map_err(|e| format!("token bootstrap failed: {e}"))?;

    let capabilities = {
        let capabilities = brains_domain::Capabilities::local(
            brains_build_info::VERSION,
            &config.vault,
            limits.max_text_bytes,
            limits.max_attachment_bytes,
            brains_ports::IndexStore::full_text_search(&vault.index),
        );
        // The embedded converter ships in the default build (issue #19); a
        // --no-default-features build reports conversion as absent.
        #[cfg(feature = "convert")]
        let capabilities = {
            use brains_ports::DocumentConverter;
            let converter = brains_adapter_convert::XbergConverter::new();
            capabilities
                .with_document_conversion(converter.identity(), converter.supported_media_types())
        };
        capabilities
    };
    let capabilities = capabilities
        .without_advertised(&config.disable)
        .map_err(|name| {
            format!(
                "unknown capability `{name}` for --disable; supported: document_conversion, semantic"
            )
        })?;
    let state: SharedState = Arc::new(AppState {
        session_cookie_policy: if config
            .bind
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
        {
            brains_api_http::auth::SessionCookiePolicy::LoopbackHttp
        } else {
            brains_api_http::auth::SessionCookiePolicy::Secure
        },
        vault,
        security: Arc::new(security),
        limits,
        capabilities,
        allowed_origins: config.allowed_origins.clone(),
        web_dist: config.web_dist.clone(),
    });
    Ok((state, plaintext, token_new))
}

/// Bind the server. The caller owns polling, shutdown and error reporting.
pub async fn serve(
    config: ServerConfig,
) -> Result<axum::serve::Serve<tokio::net::TcpListener, axum::Router, axum::Router>, String>
where
    FsHeadStore: Head,
    FsRevisionStore: Rev,
    FsBlobStore: Blob,
    TursoIndex: Idx,
{
    let addr = format!("{}:{}", config.bind, config.port);
    let listener = tokio::net::TcpListener::bind((config.bind.as_str(), config.port))
        .await
        .map_err(|e| format!("cannot bind {addr}: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
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
    if config.token.is_none() {
        if token_new {
            eprintln!("brains-server: generated root token (also stored in auth.json):");
            eprintln!("  {plaintext}");
            eprintln!("brains-server: pass it as `Authorization: Bearer <token>` or BRAINS_TOKEN.");
        } else {
            eprintln!("brains-server: existing root token kept (recovered from auth.json).");
        }
    }
    tracing::info!("listening on http://{local} (vault {:?})", config.vault);
    Ok(axum::serve(listener, router))
}
