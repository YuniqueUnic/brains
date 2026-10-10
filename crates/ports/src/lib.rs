//! Storage ports (design §9.1): HeadStore / RevisionStore / BlobStore /
//! IndexStore. The semantics are the contract — no adapter may merge them
//! into one unconditional `write(path, bytes)`. Adapters declare what they
//! can do; an adapter without atomic CAS must never back a writable HeadStore.

pub mod index;
pub mod store;

pub use index::{
    FileProjection, IndexStore, LinkRow, ListFilter, ListOrder, ListRow, LiteralRow, PARSER_VERSION,
};
pub use store::{BlobStore, CasOutcome, ExpectedHead, HeadStore, RevisionStore, StoredHead};
