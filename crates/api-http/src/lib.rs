//! Runtime-agnostic HTTP layer (design §5.1, §22.1): the route table is
//! generated from `api/brains.idl` (`contract`), implemented by [`service`];
//! auth, CSRF and serving-safety middleware and the runtime assembly in
//! [`routes`] are shared by the native server and the Cloudflare Worker.
//! Runtimes only assemble state (adapters + security backend).

pub mod auth;
pub mod context;
/// Generated from `api/brains.idl` by scripts/gen-api.sh: the `brains`
/// module inside holds payload types, the service traits and the axum
/// servers.
#[path = "contract/brains.rs"]
pub mod contract;
pub mod error;
pub mod routes;
pub mod service;
pub mod state;
