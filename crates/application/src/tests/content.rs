use std::sync::{Arc, Mutex, mpsc};

use brains_domain::{
    DomainError, DomainResult, Principal, Scope, VaultPath, resources::UsageEvidence,
};
use brains_ports::{
    ConvertedDocument,
    content::{ContentConverter, FetchedSource, SourceFetcher},
};
use futures::{executor::block_on, future::BoxFuture};

use super::support::{open_vault, owner_principal, put_text};
use crate::Precondition;

type Gate = (mpsc::Sender<()>, mpsc::Receiver<()>);

struct Converter {
    gate: Mutex<Option<Gate>>,
}

impl ContentConverter for Converter {
    fn identity(&self) -> (&str, &str) {
        ("fixture", "1")
    }
    fn media_types(&self) -> Vec<String> {
        vec![
            "text/plain".into(),
            "text/markdown".into(),
            "application/pdf".into(),
        ]
    }
    fn cost_evidence(&self, _: &str) -> UsageEvidence {
        UsageEvidence::Exact
    }
    fn convert<'a>(
        &'a self,
        _: &'a str,
        bytes: &'a [u8],
        _: &'a str,
    ) -> BoxFuture<'a, DomainResult<ConvertedDocument>> {
        Box::pin(async move {
            if let Some((ready, release)) = self.gate.lock().unwrap().take() {
                ready.send(()).unwrap();
                release.recv().unwrap();
            }
            Ok(ConvertedDocument {
                markdown: String::from_utf8_lossy(bytes).into_owned(),
                title: None,
                converter_id: "fixture",
                converter_version: "1",
                warnings: vec![],
            })
        })
    }
}

struct Source {
    media: &'static str,
}
impl SourceFetcher for Source {
    fn fetch<'a>(&'a self, _: &'a str, _: usize) -> BoxFuture<'a, DomainResult<FetchedSource>> {
        Box::pin(async move {
            Ok(FetchedSource {
                url: "https://example.com/paper".into(),
                bytes: b"source bytes".to_vec(),
                media_type: self.media.into(),
            })
        })
    }
}

#[test]
fn unchanged_conversion_detects_a_concurrent_destination_edit() {
    let fixture = open_vault();
    let mut vault = fixture.vault;
    let converter = Arc::new(Converter {
        gate: Mutex::new(None),
    });
    vault.converter = Some(converter.clone());
    let vault = Arc::new(vault);
    let owner = owner_principal();
    block_on(put_text(
        vault.as_ref(),
        &owner,
        "source.txt",
        "stable source",
        Precondition::Create,
    ))
    .unwrap();
    let first = block_on(vault.convert_document(
        &owner,
        &VaultPath::new("source.txt").unwrap(),
        Some("note.md"),
    ))
    .unwrap();
    assert!(
        block_on(vault.convert_document(
            &owner,
            &VaultPath::new("source.txt").unwrap(),
            Some("note.md")
        ))
        .unwrap()
        .unchanged
    );
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *converter.gate.lock().unwrap() = Some((ready_tx, release_rx));
    let converting = vault.clone();
    let task = std::thread::spawn(move || {
        block_on(converting.convert_document(
            &owner_principal(),
            &VaultPath::new("source.txt").unwrap(),
            Some("note.md"),
        ))
    });
    ready_rx.recv().unwrap();
    let updated = block_on(put_text(
        vault.as_ref(),
        &owner,
        "note.md",
        "human edit",
        Precondition::IfRevision(
            brains_domain::HexId::from_hex(&first.companion_revision).unwrap(),
        ),
    ))
    .unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(
        task.join().unwrap(),
        Err(DomainError::RevisionConflict { .. })
    ));
    assert_eq!(
        block_on(vault.read_file(&owner, &VaultPath::new("note.md").unwrap()))
            .unwrap()
            .head
            .revision
            .as_str(),
        updated.revision
    );
}

#[test]
fn binary_import_preserves_owned_source_and_reports_a_note_conflict() {
    let fixture = open_vault();
    let mut vault = fixture.vault;
    let converter = Arc::new(Converter {
        gate: Mutex::new(None),
    });
    vault.converter = Some(converter.clone());
    vault.fetcher = Some(Arc::new(Source {
        media: "application/pdf",
    }));
    let vault = Arc::new(vault);
    let owner = owner_principal();
    block_on(put_text(
        vault.as_ref(),
        &owner,
        "paper.md.source",
        "unrelated file",
        Precondition::Create,
    ))
    .unwrap();
    let first = block_on(vault.import_public_url(
        &owner,
        "https://example.com/paper",
        &VaultPath::new("paper.md").unwrap(),
    ))
    .unwrap();
    let source_path = first.source_path.unwrap();
    let source_revision = first.source_revision.unwrap();
    let note = block_on(vault.read_file(&owner, &VaultPath::new("paper.md").unwrap())).unwrap();
    let text = String::from_utf8(note.bytes).unwrap();
    assert!(text.contains(&source_path) && text.contains(&source_revision));
    assert_eq!(
        block_on(vault.read_file(&owner, &VaultPath::new("paper.md.source").unwrap()))
            .unwrap()
            .bytes,
        b"unrelated file"
    );
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *converter.gate.lock().unwrap() = Some((ready_tx, release_rx));
    let importing = vault.clone();
    let task = std::thread::spawn(move || {
        block_on(importing.import_public_url(
            &owner_principal(),
            "https://example.com/paper",
            &VaultPath::new("paper.md").unwrap(),
        ))
    });
    ready_rx.recv().unwrap();
    block_on(put_text(
        vault.as_ref(),
        &owner,
        "paper.md",
        "edited elsewhere",
        Precondition::IfRevision(
            brains_domain::HexId::from_hex(first.revision.as_ref().unwrap()).unwrap(),
        ),
    ))
    .unwrap();
    release_tx.send(()).unwrap();
    let partial = task.join().unwrap().unwrap();
    assert!(partial.partial);
    assert_eq!(partial.source_path.as_deref(), Some(source_path.as_str()));
    assert_eq!(
        partial.source_revision.as_deref(),
        Some(source_revision.as_str())
    );
    assert!(matches!(
        partial.error,
        Some(DomainError::RevisionConflict { .. })
    ));
}

#[test]
fn text_import_needs_only_its_selected_destination_scope() {
    let fixture = open_vault();
    let mut vault = fixture.vault;
    vault.converter = Some(Arc::new(Converter {
        gate: Mutex::new(None),
    }));
    vault.fetcher = Some(Arc::new(Source {
        media: "text/plain",
    }));
    let writer = Principal {
        actor: "narrow-writer".into(),
        scopes: vec![Scope::parse("write:only.md").unwrap()],
    };
    let result = block_on(vault.import_public_url(
        &writer,
        "https://example.com/paper",
        &VaultPath::new("only.md").unwrap(),
    ))
    .unwrap();
    assert!(!result.partial);
    assert!(result.source_path.is_none());
}
