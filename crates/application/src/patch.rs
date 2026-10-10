//! Exact-text patch (design §11.5, P1): sequential edits on an in-memory
//! candidate; any edit whose match count differs from 1 aborts the whole
//! operation with HEAD untouched. Dry-run computes a diff and no lock.

use brains_domain::{AccessLevel, DomainError, DomainResult, HexId, Principal, VaultPath};
use similar::TextDiff;

use crate::commit::CommitInput;
use crate::{MutationOutcome, Precondition, Vault};

#[derive(Clone, Debug)]
pub struct PatchEdit {
    pub old_text: String,
    pub new_text: String,
    pub expected_matches: usize,
}

pub struct PatchInput {
    pub edits: Vec<PatchEdit>,
    pub dry_run: bool,
}

pub struct PatchOutcome {
    pub diff: String,
    pub base_revision: String,
    pub committed: Option<MutationOutcome>,
}

const MAX_DIFF_OUTPUT: usize = 512 * 1024;

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brains_ports::HeadStore + Send + Sync,
    R: brains_ports::RevisionStore + Send + Sync,
    B: brains_ports::BlobStore + Send + Sync,
    I: brains_ports::IndexStore + Send + Sync,
{
    pub async fn patch_file(
        &self,
        principal: &Principal,
        path: &VaultPath,
        precondition: Precondition,
        actor: String,
        operation_id: HexId,
        input: PatchInput,
    ) -> DomainResult<PatchOutcome> {
        if input.edits.len() > 20 {
            return Err(DomainError::InvalidArgument(
                "at most 20 edits per patch".into(),
            ));
        }
        if input.edits.iter().any(|edit| edit.expected_matches != 1) {
            return Err(DomainError::InvalidArgument(
                "P0/P1 supports expected_matches=1 only".into(),
            ));
        }
        self.require_scope(principal, AccessLevel::Write, path)?;
        let current = self.check_precondition(path, &precondition).await?;
        let Some(stored) = current else {
            return Err(DomainError::NotFound {
                path: path.as_str().to_owned(),
            });
        };
        let revision = self.load_revision(&stored.head.revision).await?;
        let bytes = self.revision_bytes(&revision).await?;
        let base_text = String::from_utf8(bytes)
            .map_err(|_| DomainError::InvalidArgument("patch applies to UTF-8 text only".into()))?;

        let mut candidate = base_text.clone();
        for (index, edit) in input.edits.iter().enumerate() {
            if edit.old_text.is_empty() {
                return Err(DomainError::InvalidArgument(
                    "old_text must not be empty".into(),
                ));
            }
            let matches = candidate.matches(&edit.old_text).count();
            if matches != edit.expected_matches {
                return Err(DomainError::EditMismatch {
                    path: path.as_str().to_owned(),
                    detail: format!(
                        "edit {index}: expected {} match(es), found {matches}",
                        edit.expected_matches
                    ),
                });
            }
            candidate = candidate.replacen(&edit.old_text, &edit.new_text, edit.expected_matches);
        }

        let diff = render_diff(&base_text, &candidate);
        if input.dry_run {
            return Ok(PatchOutcome {
                diff,
                base_revision: revision.revision.as_str().to_owned(),
                committed: None,
            });
        }

        let content_ref = revision.content.clone().expect("patched file has content");
        let outcome = self
            .put_file(
                principal,
                CommitInput {
                    path,
                    content: candidate.as_bytes(),
                    media_type: Some(&content_ref.media_type),
                    actor,
                    operation_id,
                    precondition: Precondition::IfRevision(revision.revision.clone()),
                    executable: Some(content_ref.executable),
                    copied_from: None,
                },
            )
            .await?;
        Ok(PatchOutcome {
            diff,
            base_revision: revision.revision.as_str().to_owned(),
            committed: Some(outcome),
        })
    }

    /// Bounded unified diff between two revisions of one path (P1 `diff`).
    pub async fn diff_revisions(
        &self,
        principal: &Principal,
        path: &VaultPath,
        from: &HexId,
        to: &HexId,
    ) -> DomainResult<String> {
        self.require_scope(principal, AccessLevel::Read, path)?;
        let from_rev = self.read_revision(principal, path, from).await?;
        let to_rev = self.read_revision(principal, path, to).await?;
        let from_text = String::from_utf8(self.revision_bytes(&from_rev).await?)
            .map_err(|_| DomainError::InvalidArgument("diff applies to UTF-8 text only".into()))?;
        let to_text = String::from_utf8(self.revision_bytes(&to_rev).await?)
            .map_err(|_| DomainError::InvalidArgument("diff applies to UTF-8 text only".into()))?;
        Ok(render_diff(&from_text, &to_text))
    }
}

pub fn render_diff(from: &str, to: &str) -> String {
    let diff = TextDiff::from_lines(from, to);
    let rendered = diff
        .unified_diff()
        .context_radius(3)
        .header("a", "b")
        .to_string();
    if rendered.len() > MAX_DIFF_OUTPUT {
        let mut cut = MAX_DIFF_OUTPUT;
        while !rendered.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}\n… diff truncated …\n", &rendered[..cut])
    } else {
        rendered
    }
}
