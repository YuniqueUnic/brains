//! Read-only vault quality report (issue #14): stale notes, unconfirmed
//! agent claims, unsupported inferences, broken links and frontmatter
//! warnings. Works identically in local and remote mode over the existing
//! Library API — no server endpoint, no writes, one body at a time so large
//! vaults never load every file at once.

use serde::Serialize;
use std::collections::BTreeSet;

use brains_format::markdown::{LinkKind, ResolvedLink, extract_links};
use brains_format::{Frontmatter, NoteStatus, split_frontmatter};

use crate::endpoint::FileEndpoint;
use crate::ClientResult;

/// One finding: the rule that fired, where, and a short fix hint.
#[derive(Clone, Debug, Serialize)]
pub struct CheckFinding {
    pub rule: String,
    pub path: String,
    pub hint: String,
}

#[derive(Default, Serialize)]
pub struct CheckReport {
    pub findings: Vec<CheckFinding>,
    pub checked: usize,
    pub skipped: usize,
}

impl CheckReport {
    pub fn clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// One scanned note's locally-derived facts. Deprecated backlinks need the
/// per-note status map, so notes are collected before those rules run.
struct ScannedNote {
    path: brains_domain::VaultPath,
    fm: Frontmatter,
    body: String,
    links: Vec<ResolvedLink>,
}

/// Scan every live note under `prefix` (None = whole vault). Read failures
/// are findings, never silent skips.
pub fn run_check(
    endpoint: &dyn FileEndpoint,
    prefix: Option<&str>,
    today: &str,
) -> ClientResult<CheckReport> {
    let mut report = CheckReport::default();
    let files = endpoint.current_files()?;
    let live: BTreeSet<String> = files
        .iter()
        .filter(|entry| entry.state == "live")
        .map(|entry| entry.path.clone())
        .collect();
    let mut scanned: Vec<ScannedNote> = Vec::new();
    for entry in files {
        if entry.state != "live" {
            continue;
        }
        if let Some(prefix) = prefix
            && !brains_domain::path::prefix_has(prefix, &entry.path)
        {
            continue;
        }
        report.checked += 1;
        let bytes = match endpoint.revision_bytes(&entry.path, &entry.revision) {
            Ok(bytes) => bytes,
            Err(error) => {
                report.findings.push(CheckFinding {
                    rule: "unreadable".into(),
                    path: entry.path.clone(),
                    hint: format!("body could not be read: {error}"),
                });
                continue;
            }
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            // Binary attachments: no frontmatter, no links to check.
            report.skipped += 1;
            continue;
        };
        let path = brains_domain::VaultPath::trusted(entry.path.clone());
        let (yaml, body) = split_frontmatter(text);
        let fm = yaml
            .map(brains_format::frontmatter::parse)
            .unwrap_or_default();
        let links = extract_links(&path, body);
        scanned.push(ScannedNote {
            path,
            fm,
            body: body.to_owned(),
            links,
        });
    }

    // Link existence runs per note; broken internal targets stay links (they
    // may describe knowledge not yet written) but are reported.
    for note in &scanned {
        for link in &note.links {
            match link.kind {
                LinkKind::Internal => {
                    if !live.contains(&link.target) {
                        report.findings.push(CheckFinding {
                            rule: "link-unresolved".into(),
                            path: note.path.as_str().to_owned(),
                            hint: format!(
                                "internal target `{}` does not exist (yet)",
                                link.target
                            ),
                        });
                    }
                }
                LinkKind::OutsideVault => report.findings.push(CheckFinding {
                    rule: "link-outside-vault".into(),
                    path: note.path.as_str().to_owned(),
                    hint: format!("target `{}` escapes the vault", link.target),
                }),
                LinkKind::External => {}
            }
        }
    }

    // Deprecated notes still linked from live, non-deprecated notes
    // (issue #14): orphaned deprecated notes are fine, live references to a
    // deprecated note are what needs repairing.
    for note in &scanned {
        if note.fm.status != NoteStatus::Deprecated {
            continue;
        }
        let referrers: Vec<&str> = scanned
            .iter()
            .filter(|other| {
                other.path.as_str() != note.path.as_str()
                    && other.fm.status != NoteStatus::Deprecated
                    && other
                        .links
                        .iter()
                        .any(|link| link.kind == LinkKind::Internal && link.target == note.path.as_str())
            })
            .map(|other| other.path.as_str())
            .collect();
        if !referrers.is_empty() {
            report.findings.push(CheckFinding {
                rule: "deprecated-linked".into(),
                path: note.path.as_str().to_owned(),
                hint: format!(
                    "deprecated but still linked from: {}; update them to a current source",
                    referrers.join(", ")
                ),
            });
        }
    }

    for note in &scanned {
        analyze_note(&mut report, note, today);
    }
    report
        .findings
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.rule.cmp(&b.rule)));
    Ok(report)
}

/// Per-note content rules; every rule names itself and carries a fix hint.
fn analyze_note(report: &mut CheckReport, note: &ScannedNote, today: &str) {
    let path = &note.path;
    let fm = &note.fm;

    // Frontmatter parse problems (#13) pass through verbatim. Unrecognized
    // keys are legal in OKF (custom fields) and never findings.
    for warning in &fm.warnings {
        if warning.starts_with("ignored unknown key") {
            continue;
        }
        report.findings.push(CheckFinding {
            rule: "frontmatter".into(),
            path: path.as_str().to_owned(),
            hint: warning.clone(),
        });
    }

    if !path.as_str().ends_with(".md") && !path.as_str().ends_with(".markdown") {
        return;
    }

    // Stale: `stale_after` reached, computed against the caller's date.
    if let Some(stale_after) = &fm.stale_after
        && stale_after.as_str() <= today
    {
        report.findings.push(CheckFinding {
            rule: "stale".into(),
            path: path.as_str().to_owned(),
            hint: format!("stale_after {stale_after} reached; re-verify, update or deprecate"),
        });
    }

    // Checkpoint notes are continuity state (issue #15), not knowledge
    // claims: the unconfirmed/inferred rules do not apply to them.
    if fm.type_.as_deref() == Some("Checkpoint") {
        return;
    }

    // Agent-written and never confirmed.
    let machine_generated = fm
        .generated_by
        .as_deref()
        .is_some_and(|by| !by.starts_with("human:"));
    if machine_generated && fm.verified.is_empty() {
        report.findings.push(CheckFinding {
            rule: "unconfirmed".into(),
            path: path.as_str().to_owned(),
            hint: format!(
                "generated by `{}` without any verified entry; add verified.by or rewrite",
                fm.generated_by.as_deref().unwrap_or("?")
            ),
        });
    }

    // Inferred claims without support (issue #14): content MARKED as
    // agent-inferred — an explicit inference marker in the body, or a
    // machine-generated draft — must carry sources, a confidence statement
    // and stale_after; missing pieces are reported individually.
    let inferred = body_contains_inference_marker(&note.body)
        || (machine_generated && fm.status == NoteStatus::Draft);
    if inferred {
        for (present, label, fix) in [
            (
                !fm.sources.is_empty(),
                "sources",
                "add sources: [{id, resource}]",
            ),
            (
                body_mentions_confidence(&note.body),
                "a confidence statement",
                "state how confident the inference is",
            ),
            (
                fm.stale_after.is_some(),
                "stale_after",
                "add stale_after so the inference expires",
            ),
        ] {
            if !present {
                report.findings.push(CheckFinding {
                    rule: "inferred-unsupported".into(),
                    path: path.as_str().to_owned(),
                    hint: format!("agent-inferred content lacks {label}: {fix}"),
                });
            }
        }
    }

    // Footnote references without a matching sources[].id.
    let ids: Vec<&str> = fm.sources.iter().map(|s| s.id.as_str()).collect();
    for footnote in footnote_ids(&note.body) {
        if !ids.contains(&footnote.as_str()) {
            report.findings.push(CheckFinding {
                rule: "footnote-without-source".into(),
                path: path.as_str().to_owned(),
                hint: format!(
                    "footnote `[^{footnote}]` has no sources entry with id `{footnote}`"
                ),
            });
        }
    }
}

/// `[^id]` footnote references anywhere in the body. Definition lines
/// (`[^id]: text`) declare, they do not reference.
fn footnote_ids(body: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut index = 0;
    while let Some(offset) = body[index..].find("[^") {
        let marker = index + offset;
        let start = marker + 2;
        let Some(close) = body[start..].find(']') else {
            break;
        };
        let id = &body[start..start + close];
        let after = body[start + close + 1..].strip_prefix(':');
        let at_line_start = body[..marker]
            .rfind('\n')
            .map(|at| at + 1)
            .unwrap_or(0)
            == marker;
        let is_definition = at_line_start && after.is_some();
        if !id.is_empty() && !is_definition && !ids.iter().any(|seen| seen == id) {
            ids.push(id.to_owned());
        }
        index = start + close + 1;
    }
    ids
}

fn body_contains_inference_marker(body: &str) -> bool {
    let lowered = body.to_lowercase();
    lowered.contains("推断") || lowered.contains("inferred") || lowered.contains("inference:")
}

fn body_mentions_confidence(body: &str) -> bool {
    let lowered = body.to_lowercase();
    lowered.contains("confidence") || lowered.contains("置信") || lowered.contains("概率")
}
