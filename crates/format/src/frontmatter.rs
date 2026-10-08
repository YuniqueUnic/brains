//! Bounded YAML-subset frontmatter parsing for OKF-style documents
//! (design §6.1, §22.2). Supported shape — flat scalars, flow lists,
//! block lists and one nested map level — covers `type`, `title`, `tags`,
//! `okf_version` and `generated.by/at`. Anchors, aliases, tags and multi-line
//! block scalars are out of scope by design: unbounded YAML expansion is a
//! resource hazard, and unknown input keeps its bytes in the blob anyway.

const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
const MAX_TAGS: usize = 64;
const MAX_ENTRIES: usize = 128;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frontmatter {
    pub type_: Option<String>,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub okf_version: Option<String>,
    pub generated_by: Option<String>,
    pub generated_at: Option<String>,
    /// Non-fatal parse findings; the blob keeps the original bytes.
    pub warnings: Vec<String>,
}

/// Split a leading `---` fenced frontmatter block. Returns the YAML text
/// (without fences) and the body after the closing fence.
pub fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let rest = match text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        None => return (None, text),
        Some(rest) => rest,
    };
    let mut offset = 0;
    while let Some(nl) = rest[offset..].find('\n') {
        let line_start = offset;
        let line_end = offset + nl;
        let line = &rest[line_start..line_end];
        let trimmed = line.strip_suffix('\r').unwrap_or(line);
        if trimmed == "---" || trimmed == "..." {
            let yaml = rest[..line_start].trim_end_matches(['\r', '\n']);
            return (Some(yaml), &rest[line_end + 1..]);
        }
        offset = line_end + 1;
    }
    // Unterminated fence: the document has no frontmatter for indexing.
    (None, text)
}

/// Parse the YAML subset. Unknown or unsupported keys become warnings; the
/// blob always keeps the original bytes.
pub fn parse(yaml: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    if yaml.len() > MAX_FRONTMATTER_BYTES {
        fm.warnings.push(format!(
            "frontmatter exceeds {MAX_FRONTMATTER_BYTES} bytes; not parsed"
        ));
        return fm;
    }

    // Significant lines only: (indent, text-without-comment).
    let lines: Vec<(usize, &str)> = yaml
        .lines()
        .map(|line| {
            let stripped = strip_comment(line).trim_end();
            let indent = stripped.len() - stripped.trim_start().len();
            (indent, stripped.trim_start())
        })
        .filter(|(_, text)| !text.is_empty())
        .collect();

    let mut index = 0;
    while index < lines.len() {
        if fm.warnings.len() >= MAX_ENTRIES {
            fm.warnings
                .push("frontmatter entry limit reached; remaining entries skipped".into());
            break;
        }
        let (indent, text) = lines[index];
        if indent > 0 {
            fm.warnings
                .push(format!("ignored unexpectedly indented entry `{text}`"));
            index += 1;
            continue;
        }
        let Some((key, value)) = split_key(text) else {
            fm.warnings
                .push(format!("ignored unparsable line `{text}`"));
            index += 1;
            continue;
        };
        if value.is_empty() {
            // Consume the following more-indented block as one unit.
            let start = index + 1;
            let mut end = start;
            while end < lines.len() && lines[end].0 > indent {
                end += 1;
            }
            parse_block(key, &lines[start..end], &mut fm);
            index = end;
        } else {
            parse_scalar(key, value, &mut fm);
            index += 1;
        }
    }

    if fm.tags.len() > MAX_TAGS {
        fm.tags.truncate(MAX_TAGS);
        fm.warnings.push(format!("tags truncated to {MAX_TAGS}"));
    }
    fm
}

fn split_key(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    Some((key, value.trim()))
}

fn parse_scalar(key: &str, value: &str, fm: &mut Frontmatter) {
    if value.starts_with('[') {
        let items = parse_flow_list(value);
        if key == "tags" {
            fm.tags = items;
        } else {
            fm.warnings
                .push(format!("ignored list for unknown key `{key}`"));
        }
        return;
    }
    let scalar = unquote(value);
    match key {
        "type" => fm.type_ = Some(scalar),
        "title" => fm.title = Some(scalar),
        "okf_version" => fm.okf_version = Some(scalar),
        other => fm.warnings.push(format!("ignored unknown key `{other}`")),
    }
}

fn parse_block(key: &str, block: &[(usize, &str)], fm: &mut Frontmatter) {
    if block.is_empty() {
        fm.warnings
            .push(format!("ignored empty block for key `{key}`"));
        return;
    }
    // A block list: every line starts with `- `.
    if block.iter().all(|(_, text)| text.starts_with("- ")) {
        let items: Vec<String> = block.iter().map(|(_, text)| unquote(&text[2..])).collect();
        if key == "tags" {
            fm.tags = items;
        } else {
            fm.warnings
                .push(format!("ignored list for unknown key `{key}`"));
        }
        return;
    }
    // A nested map (one level): `by:` / `at:` under `generated:`.
    match key {
        "generated" => {
            for (_, text) in block {
                match split_key(text) {
                    Some(("by", value)) => fm.generated_by = Some(unquote(value)),
                    Some(("at", value)) => fm.generated_at = Some(unquote(value)),
                    Some((other, _)) => fm
                        .warnings
                        .push(format!("ignored nested key `{other}` under `generated`")),
                    None => fm.warnings.push(format!("ignored unparsable `{text}`")),
                }
            }
        }
        _ => fm
            .warnings
            .push(format!("ignored nested map for unknown key `{key}`")),
    }
}

fn parse_flow_list(value: &str) -> Vec<String> {
    let inner = value.trim().strip_prefix('[').unwrap_or(value.trim());
    let inner = inner.strip_suffix(']').unwrap_or(inner);
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner.split(',').map(|item| unquote(item.trim())).collect()
}

fn strip_comment(line: &str) -> &str {
    // A `#` starts a comment only at line start or after whitespace, and not
    // inside quotes.
    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && (i == 0 || bytes[i - 1].is_ascii_whitespace()) => {
                return &line[..i];
            }
            _ => {}
        }
    }
    line
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        let (first, last) = (bytes[0], bytes[trimmed.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].to_owned();
        }
    }
    trimmed.to_owned()
}
