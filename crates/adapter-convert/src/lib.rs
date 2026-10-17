//! Embedded document converter (issue #19): the Xberg backend behind the
//! `DocumentConverter` port. Pure Rust, fully offline, no system libraries.
//! OCR, layout models, URL ingestion and transcription stay off. At xberg
//! 1.0.0-rc.5 only PDF compiled (the office/excel/html gates referenced
//! `zip` unconditionally); the zip-9-based 1.3.x line restored the full set.

#![cfg(feature = "xberg")]

use brains_domain::{DomainError, DomainResult};
use brains_ports::{ConvertedDocument, DocumentConverter};

/// Xberg-backed converter. `convert` never loads ML runtimes: the build
/// enables only the pure-Rust PDF path.
pub struct XbergConverter;

impl XbergConverter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for XbergConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentConverter for XbergConverter {
    fn supported_media_types(&self) -> &'static [&'static str] {
        &[
            "application/pdf",
            "text/csv",
            "text/plain",
            "text/html",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ]
    }

    fn identity(&self) -> (&'static str, &'static str) {
        // Keep in step with the pinned xberg version in Cargo.toml.
        ("xberg", "1.3.10")
    }

    async fn convert(&self, bytes: &[u8], media_type: &str) -> DomainResult<ConvertedDocument> {
        if !self.supported_media_types().contains(&media_type) {
            return Err(DomainError::CapabilityUnsupported("document_conversion"));
        }
        let (converter_id, converter_version) = self.identity();
        let config = xberg::ExtractionConfig::default();
        let input = xberg::ExtractInput::from_bytes(&*bytes.to_vec(), media_type.to_owned(), None);
        let output = xberg::extract(input, &config)
            .await
            .map_err(|error| DomainError::InvalidFormat(format!("conversion failed: {error}")))?;
        let first = output
            .results
            .first()
            .ok_or_else(|| DomainError::InvalidFormat("conversion produced no content".into()))?;
        let markdown = first.content.trim().to_owned();
        if markdown.is_empty() {
            return Err(DomainError::InvalidFormat(
                "conversion produced no text (scanned PDFs need OCR, which this build does not ship)"
                    .into(),
            ));
        }
        Ok(ConvertedDocument {
            markdown,
            title: first
                .content
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
            converter_id,
            converter_version,
            warnings: Vec::new(),
        })
    }
}
