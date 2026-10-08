//! File kind detection and media-type mapping (design §6.3, §10.1).

/// `note` = Markdown, `text` = other indexable UTF-8, `script` = source code,
/// `binary` = not indexed by body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Note,
    Text,
    Script,
    Binary,
}

impl FileKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Text => "text",
            Self::Script => "script",
            Self::Binary => "binary",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "note" => Some(Self::Note),
            "text" => Some(Self::Text),
            "script" => Some(Self::Script),
            "binary" => Some(Self::Binary),
            _ => None,
        }
    }

    pub fn searchable(&self) -> bool {
        !matches!(self, Self::Binary)
    }
}

const SCRIPT_EXTS: [&str; 12] = [
    "sh", "bash", "py", "rs", "js", "mjs", "cjs", "ts", "tsx", "rb", "pl", "lua",
];
const TEXT_EXTS: [&str; 8] = ["txt", "csv", "tsv", "json", "yaml", "yml", "toml", "ini"];

pub fn extension_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => ext,
        _ => "",
    }
}

/// Classify by extension, then confirm text kinds by UTF-8 validity so a
/// corrupt "script" never enters the FTS body as mojibake.
pub fn detect_kind(path: &str, bytes: &[u8]) -> FileKind {
    let ext = extension_of(path).to_ascii_lowercase();
    let utf8 = std::str::from_utf8(bytes).is_ok();
    if ext == "md" || ext == "markdown" {
        if utf8 {
            return FileKind::Note;
        }
        return FileKind::Binary;
    }
    if SCRIPT_EXTS.contains(&ext.as_str()) {
        return if utf8 {
            FileKind::Script
        } else {
            FileKind::Binary
        };
    }
    if TEXT_EXTS.contains(&ext.as_str()) {
        return if utf8 {
            FileKind::Text
        } else {
            FileKind::Binary
        };
    }
    FileKind::Binary
}

/// Media type recorded in the immutable ContentRef. The server never trusts a
/// client-declared value for serving decisions (design §13.3/§15.3); this map
/// is the single authority.
pub fn media_type_for(path: &str) -> &'static str {
    match extension_of(path).to_ascii_lowercase().as_str() {
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "ini" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "sh" | "bash" => "text/x-shellscript; charset=utf-8",
        "py" => "text/x-python; charset=utf-8",
        "rs" => "text/x-rust; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "ts" | "tsx" => "text/typescript; charset=utf-8",
        "rb" => "text/x-ruby; charset=utf-8",
        "pl" => "text/x-perl; charset=utf-8",
        "lua" => "text/x-lua; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Whether the path's extension *declares* text content (note/script/text).
/// A file whose name promises text but whose bytes are not UTF-8 is a 422
/// INVALID_FORMAT, not a silent demotion to binary.
pub fn declares_text(path: &str) -> bool {
    let ext = extension_of(path).to_ascii_lowercase();
    ext == "md"
        || ext == "markdown"
        || SCRIPT_EXTS.contains(&ext.as_str())
        || TEXT_EXTS.contains(&ext.as_str())
}

/// Media types whose bodies are human-readable text: they never require the
/// `X-Content-SHA256` digest header; everything else counts as binary for
/// upload validation (design §22.3).
pub fn is_text_media(media_type: &str) -> bool {
    let base = media_type.split(';').next().unwrap_or("").trim();
    base.starts_with("text/")
        || matches!(
            base,
            "application/json" | "application/yaml" | "application/toml"
        )
}
