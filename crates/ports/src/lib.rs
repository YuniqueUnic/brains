//! Storage ports (design §9.1): HeadStore / RevisionStore / BlobStore /
//! IndexStore. The semantics are the contract — no adapter may merge them
//! into one unconditional `write(path, bytes)`. Adapters declare what they
//! can do; an adapter without atomic CAS must never back a writable HeadStore.

pub mod content;
pub mod convert;
pub mod embedding;
pub mod index;
pub mod resources;
pub mod semantic;
pub mod store;

pub use convert::{ConvertedDocument, DocumentConverter};
pub use embedding::{
    EmbedChunk, EmbeddingModel, EmbeddingProvider, NoopEmbeddingProvider, VECTOR_INDEX_VERSION,
    VectorIndex, chunk_text,
};
pub use index::{
    FileProjection, IndexStore, LinkRow, ListFilter, ListOrder, ListRow, LiteralRow, MetaFilter,
    PARSER_VERSION, meta_filter_clause,
};
pub use store::{BlobStore, CasOutcome, ExpectedHead, HeadStore, RevisionStore, StoredHead};
