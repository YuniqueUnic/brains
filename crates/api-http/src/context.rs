//! Per-request context handed from the [`crate::routes::authenticate`]
//! middleware to the generated handlers.
//!
//! Generated trait methods receive only the parameters the IDL declares, so
//! ambient request data — who authenticated, the request id, the raw header
//! map — travels alongside the handler future in a task-local. The
//! middleware scopes `next.run(request)` in it, which covers handler
//! execution because axum polls the whole middleware/handler chain inside
//! one connection task.

use crate::auth::AuthSource;
use axum::http::HeaderMap;

#[derive(Clone)]
pub struct RequestContext {
    pub auth: AuthSource,
    pub request_id: String,
    pub headers: HeaderMap,
}

tokio::task_local! {
    static REQUEST: RequestContext;
}

/// Runs `f` with `ctx` installed; every generated handler polled inside it
/// can call [`current`].
pub async fn scope<F: std::future::Future>(ctx: RequestContext, f: F) -> F::Output {
    REQUEST.scope(ctx, f).await
}

/// The context of the request being handled. Handlers only run inside the
/// middleware scope, so the context always exists.
pub fn current() -> RequestContext {
    REQUEST.with(|ctx| ctx.clone())
}
