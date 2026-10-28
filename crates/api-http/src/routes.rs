//! Router assembly and the authenticate middleware.
//!
//! The route table itself, input extraction and response serialization are
//! generated from `api/brains.idl` (`gen::brains`); this module layers the
//! cross-cutting concerns around them: request ids, authentication, the
//! CSRF/Origin guard for cookie-driven mutations, the serving-safety header
//! for raw byte responses and the local-profile SPA fallback.

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

use crate::auth::AuthSource;
use crate::context::{RequestContext, scope};
use crate::error;
use crate::state::{Blob, Head, Idx, Rev, SharedState};
use xidl_rust_axum::Service;

pub fn build_router<H: Head, R: Rev, B: Blob, I: Idx>(state: SharedState<H, R, B, I>) -> Router {
    let files = crate::contract::brains::FilesServer::new(crate::service::FilesService {
        state: state.clone(),
    });
    let archives =
        crate::contract::brains::ArchivesServer::new(crate::service::archive::ArchiveService {
            state: state.clone(),
        });
    let session = crate::contract::brains::SessionsServer::new(crate::service::SessionService {
        state: state.clone(),
    });
    let api = files
        .into_router()
        .merge(session.into_router())
        .merge(archives.into_router())
        .layer(axum::middleware::from_fn(serving_safety))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        // Unknown /api/v1 routes answer JSON, never the SPA shell (§5.1).
        .fallback(error::api_not_found)
        // One body-limit owner for both runtimes (§6.1): the largest payload
        // plus JSON envelope headroom, instead of a per-app magic constant.
        .layer(axum::extract::DefaultBodyLimit::max(
            state.limits.max_attachment_bytes + 1024 * 1024,
        ));

    // `mut` is consumed by the local-profile SPA fallback below.
    #[allow(unused_mut)]
    let mut router = Router::new()
        .nest("/api/v1", api)
        // Anything under /api but outside /api/v1 is an API miss: JSON 404,
        // never the SPA shell (§5.1).
        .route("/api", axum::routing::any(error::api_not_found))
        .route("/api/{*rest}", axum::routing::any(error::api_not_found))
        .route("/healthz", axum::routing::get(error::healthz));
    // SPA fallback is local-profile only (§9.2); the Worker serves assets via
    // the ASSETS binding and never sees non-API paths.
    #[cfg(feature = "local")]
    if let Some(dist) = &state.web_dist {
        let index = dist.join("index.html");
        let serve = tower_http::services::ServeDir::new(dist)
            .append_index_html_on_directories(true)
            .fallback(tower_http::services::ServeFile::new(index));
        router = router.fallback_service(serve);
    }
    #[cfg(not(feature = "local"))]
    let _ = &state.web_dist;
    router
}

/// Every /api/v1 request carries a request id and an authenticated principal
/// (design §15.1); cookie-driven mutations additionally pass the CSRF/Origin
/// guard (§15.2). `POST /session` accepts only Bearer credentials — the
/// cookie path reaches the handler, which answers a typed 400.
async fn authenticate<H: Head, R: Rev, B: Blob, I: Idx>(
    State(state): State<SharedState<H, R, B, I>>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = brains_domain::HexId::generate().as_str().to_owned();
    let headers = request.headers().clone();
    // The nest strips the /api/v1 prefix before this middleware runs.
    let session_create = request.method() == Method::POST && request.uri().path() == "/session";

    let source = match AuthSource::authorize_request(
        state.security.as_ref(),
        &headers,
        request.method(),
        session_create,
        &state.allowed_origins,
    ) {
        Ok(source) => source,
        Err(brains_domain::DomainError::Forbidden(message)) => {
            return error::forbidden(message, &request_id);
        }
        Err(err) => return error::unauthorized(err.to_string(), &request_id),
    };

    request.extensions_mut().insert(request_id.clone());
    let mut response = scope(
        RequestContext {
            auth: source,
            request_id: request_id.clone(),
            headers,
        },
        next.run(request),
    )
    .await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

/// Serving safety (§15.3): raw byte bodies never sniff-render. The union
/// transport fixes their Content-Type at application/octet-stream, so the
/// header attaches in middleware — the generated handlers cannot know it.
async fn serving_safety(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let raw_bytes = response.status() == StatusCode::OK
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/octet-stream"));
    if raw_bytes {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
    }
    response
}
