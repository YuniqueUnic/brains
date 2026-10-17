//! Embedding provider port (issue #17): the single abstraction for producing
//! and storing embeddings. One port, different implementations per runtime —
//! no runtime mounts a provider yet, so every runtime declares semantic
//! search unavailable. The vector index is a disposable projection: rebuilding
//! it never touches the source of truth (HEAD/revisions/blobs).

use std::future::Future;

use brains_domain::{DomainError, DomainResult};

/// Bump when the vector projection shape (chunking, key layout) changes; a
/// bump triggers a full vector rebuild on reindex, exactly like
/// `PARSER_VERSION` for the SQL projection.
pub const VECTOR_INDEX_VERSION: i64 = 1;

/// Stable identity of the active embedding model, reported by capabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingModel {
    pub model_id: String,
    pub dimensions: usize,
}

/// One chunk of a note to embed, keyed by the immutable revision it derives
/// from — superseded revisions and trashed paths are removed, never mutated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedChunk {
    pub path: String,
    pub revision: String,
    pub chunk: usize,
    pub text: String,
}

/// Embedding production. Implementations are runtime adapters: Noop locally,
/// Workers AI on the Worker. Text above the instance's `max_text_bytes`,
/// attachments and non-text kinds are never sent here.
pub trait EmbeddingProvider: Send + Sync {
    fn model(&self) -> &EmbeddingModel;
    /// `embed` returns one vector per chunk, in order.
    fn embed(
        &self,
        chunks: &[EmbedChunk],
    ) -> impl Future<Output = DomainResult<Vec<Vec<f32>>>> + Send;
}

/// The disposable vector projection. Keys are `(vault, path, revision,
/// chunk)` plus `vector_index_version`; upserts are idempotent per key.
pub trait VectorIndex: Send + Sync {
    fn upsert(
        &self,
        vault: &str,
        chunks: &[EmbedChunk],
        vectors: &[Vec<f32>],
    ) -> impl Future<Output = DomainResult<()>> + Send;
    /// Drop every vector of one path (trash, purge, superseded revision).
    fn delete_path(&self, vault: &str, path: &str)
    -> impl Future<Output = DomainResult<()>> + Send;
    /// Nearest neighbors; returns `(path, revision, distance)` triples.
    fn query(
        &self,
        vault: &str,
        vector: &[f32],
        limit: usize,
    ) -> impl Future<Output = DomainResult<Vec<(String, String, f32)>>> + Send;
}

/// Local/native provider: semantic search reports as unavailable; explicit
/// requests fail clearly (like disabled FTS), never with a fake result. The
/// interface stays open for a later local model.
pub struct NoopEmbeddingProvider;

impl EmbeddingProvider for NoopEmbeddingProvider {
    fn model(&self) -> &EmbeddingModel {
        // A stable placeholder identity: capabilities report availability
        // from `semantic_available`, which this provider answers as false.
        static MODEL: std::sync::OnceLock<EmbeddingModel> = std::sync::OnceLock::new();
        MODEL.get_or_init(|| EmbeddingModel {
            model_id: "none".into(),
            dimensions: 0,
        })
    }

    async fn embed(&self, _chunks: &[EmbedChunk]) -> DomainResult<Vec<Vec<f32>>> {
        Err(DomainError::CapabilityUnsupported("semantic_search"))
    }
}

/// Split searchable text into bounded chunks. Simple fixed windows with
/// paragraph preference: deterministic across rebuilds so equal revisions
/// produce equal vectors (the projection stays disposable and reproducible).
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if current.len() + paragraph.len() + 2 > max_chars && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        if paragraph.len() > max_chars {
            // One oversized paragraph: hard windows.
            for window in paragraph.as_bytes().chunks(max_chars) {
                let text = String::from_utf8_lossy(window).into_owned();
                if !text.is_empty() {
                    chunks.push(text);
                }
            }
        } else {
            current.push_str(paragraph);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
