//! Generated-note rendering shared by local and remote conversion.

pub struct SavedSource<'a> {
    pub path: &'a str,
    pub revision: &'a str,
}

pub struct GeneratedNote<'a> {
    pub title: &'a str,
    pub markdown: &'a str,
    pub source: &'a str,
    pub revision: Option<&'a str>,
    pub sha256: &'a str,
    pub modified_at: Option<&'a str>,
    pub converter: &'a str,
    pub generated_at: &'a str,
    pub stored_source: Option<SavedSource<'a>>,
    pub warnings: &'a [String],
}

pub fn yaml_scalar(value: &str) -> String {
    serde_json::to_string(value).expect("strings serialize")
}

pub fn render_companion(note: GeneratedNote<'_>) -> String {
    let mut text = format!(
        "---\ntype: Converted Document\nstatus: draft\ntitle: {}\ngenerated:\n  by: {}\n  at: {}\nsources:\n  - id: original\n    resource: {}\n    sha256: {}\n",
        yaml_scalar(note.title),
        yaml_scalar(note.converter),
        yaml_scalar(note.generated_at),
        yaml_scalar(note.source),
        yaml_scalar(note.sha256)
    );
    if let Some(revision) = note.revision {
        text.push_str(&format!("    revision: {}\n", yaml_scalar(revision)));
    }
    if let Some(modified) = note.modified_at {
        text.push_str(&format!("    last_modified: {}\n", yaml_scalar(modified)));
    }
    if let Some(source) = note.stored_source {
        text.push_str(&format!(
            "  - id: saved-original\n    resource: {}\n    revision: {}\n",
            yaml_scalar(source.path),
            yaml_scalar(source.revision)
        ));
    }
    text.push_str("---\n\n");
    for warning in note.warnings {
        text.push_str(&format!(
            "<!-- converter warning: {} -->\n",
            warning.replace("--", "—")
        ));
    }
    text.push_str(&format!(
        "# {}\n\n{}\n",
        note.title.replace(['\n', '\r'], " "),
        note.markdown
    ));
    text
}

/// Only the generated frontmatter timestamp is volatile.
pub fn stable_companion(body: &str) -> String {
    let Some(rest) = body.strip_prefix("---\n") else {
        return body.to_owned();
    };
    let Some(end) = rest.find("\n---\n\n") else {
        return body.to_owned();
    };
    let (front, after) = rest.split_at(end);
    let stable = front
        .lines()
        .filter(|line| !line.trim_start().starts_with("at: "))
        .collect::<Vec<_>>()
        .join("\n");
    format!("---\n{stable}{after}")
}
