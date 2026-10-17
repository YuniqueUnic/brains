//! Per-scope checkpoint notes (issue #15): a conventional, ordinary note that
//! records the continuity state of ongoing work. No new storage concept, no
//! session table — the note is versioned, conditionally written and
//! restorable like any other file. The helpers here are thin wrappers over
//! cat/stat/put; no server change is required.

use brains_domain::{DomainResult, VaultPath};

/// The conventional checkpoint path of one scope (`<scope>/checkpoint.md`).
pub fn checkpoint_path(scope: &str) -> DomainResult<VaultPath> {
    let trimmed = scope.trim_end_matches('/');
    let raw = if trimmed.is_empty() {
        "checkpoint.md".to_owned()
    } else {
        format!("{trimmed}/checkpoint.md")
    };
    VaultPath::new(&raw)
}

/// The conventional frontmatter of a checkpoint note.
pub const CHECKPOINT_TYPE: &str = "Checkpoint";

/// Frontmatter block every checkpoint carries. The body is the caller's own
/// content; sections (current goal, in-progress, delivered, next steps, open
/// questions) are a documentation convention, not parsed structure.
pub fn checkpoint_frontmatter(actor: &str, at: &str) -> String {
    format!("---\ntype: {CHECKPOINT_TYPE}\ngenerated:\n  by: {actor}\n  at: {at}\n---\n\n")
}

/// Default checkpoint body for `--create` on a scope with none yet.
pub fn empty_checkpoint_body() -> &'static str {
    "# Checkpoint\n\n## 当前目标\n\n(待填写)\n\n## 进行中\n\n## 已交付\n\n## 下一步\n\n## 待解问题\n"
}
