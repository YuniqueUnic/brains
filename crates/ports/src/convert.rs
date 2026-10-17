//! Document conversion port (issue #19): one port, two runtimes. The
//! embedded converter (Xberg) serves native builds behind the compile-time
//! `convert` feature; the Worker uses Workers AI `toMarkdown` instead. A
//! build without any converter reports `conversion: unavailable` — never a
//! silent fallback or a partial note.

use std::future::Future;

use brains_domain::DomainResult;

/// One successful conversion. The markdown is always body content: text
/// inside the document that looks like frontmatter can never override the
/// companion note's frontmatter, which the caller composes.
#[derive(Clone, Debug)]
pub struct ConvertedDocument {
    pub markdown: String,
    pub title: Option<String>,
    pub converter_id: &'static str,
    pub converter_version: &'static str,
    pub warnings: Vec<String>,
}

/// Unsupported or malformed input is an error, never a partial document.
pub trait DocumentConverter: Send + Sync {
    /// Media types this build can convert; capabilities report them.
    fn supported_media_types(&self) -> &'static [&'static str];

    /// Identity reported in the companion note's `generated.by` so a later
    /// converter upgrade stays traceable.
    fn identity(&self) -> (&'static str, &'static str);

    fn convert(
        &self,
        bytes: &[u8],
        media_type: &str,
    ) -> impl Future<Output = DomainResult<ConvertedDocument>> + Send;
}
