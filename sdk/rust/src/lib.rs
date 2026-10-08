//! Generated Brains API client SDK (source of truth: `api/brains.idl`).
//! Regenerate with `scripts/gen-api.sh`; never edit `generated/` by hand.
//!
//! The web app and the CLI keep their own clients (session cookie + CSRF
//! flows and two-phase export live there); this crate is the plain
//! bearer-token SDK for external Rust consumers.

#![allow(clippy::all, warnings)]

mod brains_generated;

pub use brains_generated::brains::*;
