//! Implementations of the generated `contract::brains` traits: the handler
//! bodies that used to live in hand-written route modules, now feeding the
//! IDL-driven transport. The domain never names a status — failures classify
//! into the typed `raises(...)` exceptions declared in `api/brains.idl`,
//! and the generated server serializes them (status, ETag headers, the
//! `{"error": ErrorDetail}` envelope).
//!
//! Layout: `error` owns failure classification and the per-operation error
//! tables; `dto` owns domain→wire shapes; `conditional` owns preconditions,
//! conditional reads and request plumbing; `files`/`transfer`/`session`/
//! `archive` are the operations. Child modules share this module's imports
//! via `use super::*` (see `archive.rs`).

use brains_domain::{DomainError, HexId, VaultPath};

use crate::context::current;
use crate::contract::brains as contract;
use crate::contract::brains::*;
use crate::state::{Blob, Head, Idx, Rev, SharedState};

pub(crate) mod archive;
mod conditional;
mod dto;
mod error;
mod files;
mod resources;
mod semantic;
mod session;
mod transfer;

pub use files::FilesService;
pub use session::SessionService;

// `archive.rs` shares this module's imports via `use super::*`; the macros
// travel by path, not by scope.
pub(crate) use error::{error_from_domain, exception};
