//! First-boot template (design §19, cold-start experience): every fresh
//! vault wakes up with guide content instead of a blank screen. The notes
//! double as living documentation — including the conventions an AI agent
//! needs to operate the vault safely (§15.2 scopes, §8.2 CAS etiquette).
//!
//! Text lives in `template-notes/` next to this file, one document per file,
//! embedded at compile time via `include_str!` — content and code boundaries
//! stay separate.

use brains_domain::{HexId, Principal, Scope, VaultPath};

use crate::commit::CommitInput;
use crate::{Precondition, Vault};

/// The template: `(vault-relative path, markdown body)`, embedded from
/// `template-notes/`. Keep every note well under the text limit.
pub fn template_notes() -> &'static [(&'static str, &'static str)] {
    &[
        ("README.md", include_str!("../template-notes/README.md")),
        (
            "docs/brains-guide.md",
            include_str!("../template-notes/brains-guide.md"),
        ),
        (
            "docs/ai-guide.md",
            include_str!("../template-notes/ai-guide.md"),
        ),
        (
            "inbox/README.md",
            include_str!("../template-notes/inbox/README.md"),
        ),
    ]
}

/// Seed the template into a COMPLETELY empty vault (no live heads, no
/// tombstones): idempotent by construction — any prior content skips it.
/// Returns the number of notes written (0 = vault not empty, nothing done).
pub async fn seed_if_empty<H, R, B, I>(
    vault: &Vault<H, R, B, I>,
) -> brains_domain::DomainResult<usize>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    // The raw store listing includes tombstones: any head at all means the
    // vault has been used before and must not be touched.
    if !vault.heads.list(None, None, 1).await?.is_empty() {
        return Ok(0);
    }
    let principal = Principal {
        actor: "system:seed".into(),
        scopes: vec![
            Scope::parse("read:/").expect("valid scope"),
            Scope::parse("write:/").expect("valid scope"),
        ],
    };
    let mut seeded = 0;
    for (raw_path, body) in template_notes() {
        let path = VaultPath::new(raw_path)?;
        vault
            .put_file(
                &principal,
                CommitInput {
                    path: &path,
                    content: body.as_bytes(),
                    media_type: None,
                    actor: principal.actor.clone(),
                    operation_id: HexId::generate(),
                    precondition: Precondition::Create,
                    executable: false,
                    copied_from: None,
                },
            )
            .await?;
        seeded += 1;
    }
    Ok(seeded)
}
