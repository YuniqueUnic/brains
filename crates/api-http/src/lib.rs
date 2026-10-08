//! Runtime-agnostic HTTP layer (design §5.1, §22.1): the axum router, §11.3
//! error mapping and §15 auth shared by the native server and the Cloudflare
//! Worker. Runtimes only assemble state (adapters + security backend).

pub mod auth;
pub mod error;
#[allow(dead_code)]
pub mod probe;
pub mod routes;
pub mod state;
