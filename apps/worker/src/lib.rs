//! Brains Worker (design §5.1, §9.2 cloudflare profile): the shared axum
//! layer from `brains_api_http` serving R2 (HEAD/revisions/blobs) and D1
//! (rebuildable index). Static assets are served by Workers Static Assets;
//! this handler only sees `/api/*` and `/healthz` (wrangler `run_worker_first`).

mod security;

use std::sync::Arc;

use brains_adapter_d1::D1Index;
use brains_adapter_r2::R2ContentStore;
use brains_api_http::auth::Security;
use brains_api_http::state::AppState;
use brains_application::Vault;
use brains_domain::{Capabilities, CursorSigner, Limits};

use axum::response::IntoResponse;
use security::EnvSecurity;
use tower::Service;
use worker::{Context, Env, HttpRequest};

type WorkerVault = Vault<R2ContentStore, R2ContentStore, R2ContentStore, D1Index>;

fn int_var(env: &Env, name: &str) -> Result<usize, Box<FetchResponse>> {
    let raw = str_var(env, name)?;
    raw.parse::<usize>()
        .map_err(|_| Box::new(plain_error(500, &format!("var {name} must be an integer"))))
}

fn str_var(env: &Env, name: &str) -> Result<String, Box<FetchResponse>> {
    Ok(env
        .var(name)
        .map_err(|_| Box::new(plain_error(500, &format!("missing var {name}"))))?
        .to_string())
}

type FetchResponse = axum::http::Response<axum::body::Body>;
type StateResult = Result<
    Arc<AppState<R2ContentStore, R2ContentStore, R2ContentStore, D1Index>>,
    Box<FetchResponse>,
>;

fn plain_error(status: u16, message: &str) -> FetchResponse {
    use brains_api_types::{ErrorBody, ErrorDetail};
    (
        axum::http::StatusCode::from_u16(status).expect("valid status"),
        axum::Json(ErrorBody {
            error: ErrorDetail {
                code: "INTERNAL".into(),
                message: message.to_owned(),
                request_id: brains_domain::HexId::generate().as_str().to_owned(),
                path: None,
                current_revision: None,
                details: None,
            },
        }),
    )
        .into_response()
}

/// Assemble the cloudflare profile (design §22.1): R2 content store + D1
/// index + secret-backed security, per request bindings.
fn build_state(env: &Env) -> StateResult {
    let vault_name = str_var(env, "VAULT_ID")?;
    let bucket = env
        .bucket("BRAINS_BUCKET")
        .map_err(|e| Box::new(plain_error(500, &format!("BRAINS_BUCKET binding: {e}"))))?;
    let d1 = env
        .d1("BRAINS_INDEX")
        .map_err(|e| Box::new(plain_error(500, &format!("BRAINS_INDEX binding: {e}"))))?;

    let limits = Limits {
        max_text_bytes: int_var(env, "MAX_TEXT_BYTES")?,
        max_attachment_bytes: int_var(env, "MAX_ATTACHMENT_BYTES")?,
        ..Limits::default()
    };
    let session_key =
        security::hex_key(&str_var(env, "SESSION_SIGNING_KEY")?, "SESSION_SIGNING_KEY")
            .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;
    let cursor_key = security::hex_key(&str_var(env, "CURSOR_SIGNING_KEY")?, "CURSOR_SIGNING_KEY")
        .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;
    let security: Arc<dyn Security> = Arc::new(
        EnvSecurity::from_registry_json(&str_var(env, "TOKEN_REGISTRY_JSON")?, session_key)
            .map_err(|e| plain_error(500, &e.to_string()))?,
    );

    let store = R2ContentStore::new(bucket, vault_name.clone());
    let vault: WorkerVault = Vault::new(
        store.clone(),
        store.clone(),
        store,
        D1Index::new(d1),
        vault_name.clone(),
        limits.clone(),
        CursorSigner::new(cursor_key),
    );
    let capabilities = Capabilities::cloudflare_rust(
        &vault_name,
        limits.max_text_bytes,
        limits.max_attachment_bytes,
    );
    Ok(Arc::new(AppState {
        vault,
        security,
        limits,
        capabilities,
        // Cross-origin cookie flows are a deploy-time allowlist (design §5.2):
        // optional comma-separated ALLOWED_ORIGINS var, same-origin only when
        // unset.
        allowed_origins: env
            .var("ALLOWED_ORIGINS")
            .ok()
            .map(|list| {
                list.to_string()
                    .split(',')
                    .map(str::trim)
                    .filter(|origin| !origin.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        // Static assets come from the ASSETS binding, not the filesystem.
        web_dist: None,
    }))
}

#[worker::event(fetch)]
async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<FetchResponse, worker::Error> {
    let path = req.uri().path().to_owned();
    if path == "/healthz" {
        return Ok(axum::Json(serde_json::json!({ "ok": true })).into_response());
    }
    if !path.starts_with("/api/") && path != "/api" {
        // Only /api, /api/* and /healthz reach the worker (wrangler.toml);
        // anything else belongs to Static Assets.
        return Ok(axum::http::StatusCode::NOT_FOUND.into_response());
    }
    let state = match build_state(&env) {
        Ok(state) => state,
        Err(response) => return Ok(*response),
    };
    let mut router = brains_api_http::routes::build_router(state);
    // `Router::call` is infallible: it resolves to the §11.3 error mapping
    // inside the shared axum layer.
    Ok(router.call(req).await?)
}
