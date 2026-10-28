//! Brains domain: vault paths, immutable HEAD/revision records, error codes,
//! instance limits, authorization scopes and signed pagination cursors.
//!
//! This crate knows nothing about HTTP, filesystems, SQLite or Cloudflare.

pub mod capabilities;
pub mod cursor;
pub mod entry;
pub mod error;
pub mod head;
pub mod id;
pub mod kind;
pub mod limits;
pub mod path;
pub mod resources;
pub mod retrieval;
pub mod revision;
pub mod scope;
pub mod timestamp;

pub use capabilities::{Capabilities, FullTextSearch, Runtime, SemanticCapability};
pub use cursor::{CursorSigner, Endpoint};
pub use entry::EntryInfo;
pub use error::{DomainError, DomainResult};
pub use head::{Head, HeadState};
pub use id::HexId;
pub use kind::{
    FileKind, declares_text, detect_kind, is_text_media, media_type_essence, media_type_for,
};
pub use limits::Limits;
pub use path::VaultPath;
pub use revision::{ContentRef, CopyProvenance, Revision, RevisionAction, blob_key};
pub use scope::{AccessLevel, Principal, Scope};

#[cfg(test)]
mod tests;
