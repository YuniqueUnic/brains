//! Brains Worker (design §5.1, §9.2 cloudflare profile): the shared axum
//! layer from `brains_api_http` serving R2 (HEAD/revisions/blobs) and D1
//! (rebuildable index). Static assets are served by Workers Static Assets;
//! this handler only sees `/api/*` and `/healthz` (wrangler `run_worker_first`).

mod security;
mod services;

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
        reindex_page_max: 2,
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
    let mut vault: WorkerVault = Vault::new(
        store.clone(),
        store.clone(),
        store.clone(),
        D1Index::new(d1),
        vault_name.clone(),
        limits.clone(),
        CursorSigner::new(cursor_key),
    );
    services::configure(env, &mut vault, store)
        .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;
    let capabilities = {
        let mut capabilities = Capabilities::cloudflare_rust(
            brains_build_info::VERSION,
            &vault_name,
            limits.max_text_bytes,
            limits.max_attachment_bytes,
        );
        capabilities.semantic = Some(worker_semantics(env));
        if let Some(semantic) = &vault.semantic {
            capabilities.semantic = Some(semantic.capability());
            capabilities.features.insert("semantic_search".into(), true);
        }
        if let Some(converter) = &vault.converter {
            capabilities.document_conversion =
                Some(brains_domain::capabilities::DocumentConversion {
                    converter: converter.identity().0.into(),
                    version: converter.identity().1.into(),
                    media_types: converter.media_types(),
                });
            capabilities
                .features
                .insert("document_conversion".into(), true);
            capabilities
                .features
                .insert("remote_conversion".into(), true);
        }
        capabilities
            .features
            .insert("remote_url_import".into(), vault.fetcher.is_some());
        if let Some(resources) = &vault.resources {
            capabilities.features.insert("resource_policy".into(), true);
            capabilities.resources = Some(brains_domain::capabilities::ResourceCapability {
                profile_id: resources.profile.profile.id.clone(),
                reviewed_at: resources.profile.profile.reviewed_at.clone(),
                coverage: "controlled application operations".into(),
            });
        }
        capabilities
    };
    Ok(Arc::new(AppState {
        session_cookie_policy: brains_api_http::auth::SessionCookiePolicy::Secure,
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

/// Report why this deployment has no mounted semantic provider.
fn worker_semantics(env: &Env) -> brains_domain::SemanticCapability {
    let off = str_var(env, "SEMANTIC_SEARCH")
        .unwrap_or_else(|_| "on".into())
        .eq_ignore_ascii_case("off");
    let reason = if off {
        "SEMANTIC_SEARCH=off on this deployment"
    } else {
        "no AI Search provider configured for this deployment"
    };
    Capabilities::semantic_unavailable(reason)
}

#[worker::event(fetch)]
async fn fetch(req: HttpRequest, env: Env, ctx: Context) -> Result<FetchResponse, worker::Error> {
    let path = req.uri().path().to_owned();
    // /api, /api/* and /healthz are answered by the shared axum router
    // (healthz lives there too); anything else belongs to Static Assets.
    if !path.starts_with("/api/") && path != "/api" && path != "/healthz" {
        return Ok(axum::http::StatusCode::NOT_FOUND.into_response());
    }
    let state = match build_state(&env) {
        Ok(state) => state,
        Err(response) => return Ok(*response),
    };
    if let Some(budget) = &state.vault.resources
        && let Err(error) = budget.debit("workers", "requests", 1.0).await
    {
        let status = if matches!(error, brains_domain::DomainError::QuotaExceeded { .. }) {
            429
        } else {
            503
        };
        if req.method() == axum::http::Method::HEAD {
            return Ok((
                axum::http::StatusCode::from_u16(status).expect("valid status"),
                [("X-Brain-Error-Code", error.code())],
            )
                .into_response());
        }
        let mut detail = serde_json::json!({"code":error.code(),"message":error.to_string(),"request_id":brains_domain::HexId::generate().as_str()});
        if let brains_domain::DomainError::QuotaExceeded { rule, reset_at } = &error {
            detail["resource_rule"] = serde_json::json!(rule);
            detail["reset_at"] = serde_json::json!(reset_at);
        }
        let body = serde_json::json!({"error":detail});
        return Ok((
            axum::http::StatusCode::from_u16(status).expect("valid status"),
            axum::Json(body),
        )
            .into_response());
    }
    let mutated = !matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) && !matches!(
        path.as_str(),
        "/api/v1/search" | "/api/v1/files/query" | "/api/v1/session"
    );
    let semantic = state.vault.semantic.clone();
    let mut router = brains_api_http::routes::build_router(state);
    // `Router::call` is infallible: it resolves to the §11.3 error mapping
    // inside the shared axum layer.
    let response = router.call(req).await?;
    if let Some(semantic) = semantic.filter(|_| mutated) {
        ctx.wait_until(async move {
            let _ = semantic.reconcile().await;
        });
    }
    Ok(response)
}

#[worker::event(scheduled)]
async fn scheduled(_event: worker::ScheduledEvent, env: Env, _ctx: worker::ScheduleContext) {
    if let Ok(state) = build_state(&env)
        && let Some(semantic) = &state.vault.semantic
    {
        let _ = semantic.reconcile().await;
    }
}
