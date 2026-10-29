//! Brains Worker (design §5.1, §9.2 cloudflare profile): the shared axum
//! layer from `brains_api_http` serving R2 (HEAD/revisions/blobs) and D1
//! (rebuildable index). Static assets are served by Workers Static Assets;
//! this handler only sees `/api/*` and `/healthz` (wrangler `run_worker_first`).

mod security;
mod services;

use std::sync::Arc;

use brains_adapter_d1::D1Index;
use brains_adapter_r2::R2ContentStore;
use brains_api_http::auth::{AuthSource, Security};
use brains_api_http::state::AppState;
use brains_application::Vault;
use brains_domain::{AccessLevel, Capabilities, CursorSigner, Limits};

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

fn load_security(env: &Env) -> Result<Arc<dyn Security>, Box<FetchResponse>> {
    let key = security::hex_key(&str_var(env, "SESSION_SIGNING_KEY")?, "SESSION_SIGNING_KEY")
        .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;
    let security = EnvSecurity::from_registry_json(&str_var(env, "TOKEN_REGISTRY_JSON")?, key)
        .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;
    Ok(Arc::new(security))
}

/// Assemble the cloudflare profile (design §22.1): R2 content store + D1
/// index + secret-backed security, per request bindings.
async fn build_state(
    env: &Env,
    security: Arc<dyn Security>,
    allowed_origins: Vec<String>,
) -> StateResult {
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
    let cursor_key = security::hex_key(&str_var(env, "CURSOR_SIGNING_KEY")?, "CURSOR_SIGNING_KEY")
        .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;

    let mut store = R2ContentStore::new(bucket, vault_name.clone());
    if let Ok(pages) = env.var("R2_LIST_PAGE_MAX") {
        let pages = pages.to_string().parse::<usize>().map_err(|_| {
            Box::new(plain_error(
                500,
                "R2_LIST_PAGE_MAX must be a positive integer",
            ))
        })?;
        store = store
            .with_list_page_max(pages)
            .map_err(|e| Box::new(plain_error(500, &e.to_string())))?;
    }
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
        .await
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
        if let Some(control) = &vault.resource_control {
            capabilities.features.insert("resource_policy".into(), true);
            capabilities.resources = Some(brains_domain::capabilities::ResourceCapability {
                profile_id: control.settings.profile.profile.id.clone(),
                reviewed_at: control.settings.profile.profile.reviewed_at.clone(),
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
        allowed_origins,
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
    // Liveness does not read configuration, D1, R2 or the usage ledger.
    if path == "/healthz" {
        return Ok(match *req.method() {
            axum::http::Method::GET => brains_api_http::error::healthz().await,
            axum::http::Method::HEAD => axum::http::StatusCode::OK.into_response(),
            _ => (
                axum::http::StatusCode::METHOD_NOT_ALLOWED,
                [("Allow", "GET, HEAD")],
            )
                .into_response(),
        });
    }
    let allowed_origins: Vec<String> = env
        .var("ALLOWED_ORIGINS")
        .ok()
        .map(|list| {
            list.to_string()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let security = match load_security(&env) {
        Ok(security) => security,
        Err(response) => return Ok(*response),
    };
    let session_create = req.method() == axum::http::Method::POST && path == "/api/v1/session";
    let auth = match AuthSource::authorize_request(
        security.as_ref(),
        req.headers(),
        req.method(),
        session_create,
        &allowed_origins,
    ) {
        Ok(auth) => auth,
        Err(error) => {
            let id = brains_domain::HexId::generate().as_str().to_owned();
            return Ok(match error {
                brains_domain::DomainError::Forbidden(message) => {
                    brains_api_http::error::forbidden(message, &id)
                }
                error => brains_api_http::error::unauthorized(error.to_string(), &id),
            });
        }
    };
    let state = match build_state(&env, security, allowed_origins).await {
        Ok(state) => state,
        Err(response) => return Ok(*response),
    };
    // Administration stays available to repair exhausted budgets. Its requests
    // are observed, while the API still requires admin authentication and CSRF.
    let administration = auth.principal().allows(AccessLevel::Admin, "")
        && matches!(
            path.as_str(),
            "/api/v1/resources"
                | "/api/v1/resources/settings"
                | "/api/v1/session"
                | "/api/v1/capabilities"
        );
    if let Some(budget) = &state.vault.resources
        && let Err(error) = if administration {
            budget.observe_request().await
        } else {
            budget.debit("workers", "requests", 1.0).await
        }
    {
        if let Err(accounting_error) = budget.flush_accounting().await {
            return Ok(plain_error(
                503,
                &format!("resource accounting: {}", accounting_error.code()),
            ));
        }
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
    let mutated = matches!(
        (req.method().as_str(), path.as_str()),
        ("PUT" | "PATCH" | "DELETE", "/api/v1/file")
            | ("DELETE", "/api/v1/trash")
            | (
                "POST",
                "/api/v1/restore"
                    | "/api/v1/copy"
                    | "/api/v1/move"
                    | "/api/v1/files/convert"
                    | "/api/v1/files/import/url"
                    | "/api/v1/maintenance/reindex"
                    | "/api/v1/maintenance/archive/head"
            )
    );
    let semantic = state.vault.semantic.clone();
    let budget = state.vault.resources.clone();
    let mut router = brains_api_http::routes::build_router(state);
    // `Router::call` is infallible: it resolves to the §11.3 error mapping
    // inside the shared axum layer.
    let response = router.call(req).await?;
    if let Some(budget) = &budget
        && let Err(error) = budget.flush_accounting().await
    {
        return Ok(plain_error(
            503,
            &format!("resource accounting: {}", error.code()),
        ));
    }
    if let Some(semantic) = semantic.filter(|_| mutated && response.status().is_success()) {
        ctx.wait_until(async move {
            let _ = semantic.reconcile(semantic.background_work(), 1).await;
            if let Some(budget) = budget
                && let Err(error) = budget.flush_accounting().await
            {
                worker::console_error!("Background resource accounting failed: {}", error.code());
            }
        });
    }
    Ok(response)
}

#[worker::event(scheduled)]
async fn scheduled(_event: worker::ScheduledEvent, env: Env, _ctx: worker::ScheduleContext) {
    if env
        .var("SEMANTIC_SEARCH")
        .is_ok_and(|v| v.to_string().eq_ignore_ascii_case("off"))
    {
        return;
    }
    let Ok(security) = load_security(&env) else {
        return;
    };
    if let Ok(state) = build_state(&env, security, Vec::new()).await {
        if let Some(semantic) = &state.vault.semantic {
            let _ = semantic.reconcile(semantic.background_work(), 1).await;
        }
        if let Some(budget) = &state.vault.resources
            && let Err(error) = budget.flush_accounting().await
        {
            worker::console_error!("Scheduled resource accounting failed: {}", error.code());
        }
    }
}
