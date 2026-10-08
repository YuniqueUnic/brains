//! Typed HTTP client shared by the CLI. Every remote mutation carries an
//! explicit revision precondition — the client never reads-and-overwrites
//! behind the user's back (design §12.1).

pub mod export;

use brainstore_api_types::{
    DiffResult, Entry, FilePage, HistoryPage, LinkPage, MutationResult, PatchBody, PatchDryRun,
    ReindexPage, RestoreBody, SearchPage, Session, TransferBody, TransferResult,
};
use brainstore_domain::Capabilities;
use brainstore_domain::DomainError;

#[derive(Debug)]
pub struct ClientError {
    pub status: u16,
    pub code: Option<String>,
    pub message: String,
    pub current_revision: Option<String>,
}

impl ClientError {
    pub fn transport(message: String) -> Self {
        Self {
            status: 0,
            code: None,
            message,
            current_revision: None,
        }
    }

    /// Process exit code per design §12.2.
    pub fn exit_code(&self) -> i32 {
        match self.status {
            0 => 8, // network/transport
            400 | 413 | 422 | 428 => 2,
            401 | 403 => 4,
            404 | 410 => 3,
            409 | 412 => 5,
            429 | 503 => 6,
            501 => 9,
            _ => 8,
        }
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "network error: {}", self.message)
        } else {
            write!(f, "HTTP {}: {}", self.status, self.message)
        }
    }
}

impl std::error::Error for ClientError {}

impl From<ClientError> for DomainError {
    fn from(error: ClientError) -> Self {
        DomainError::Internal(error.to_string())
    }
}

pub type ClientResult<T> = Result<T, ClientError>;

pub struct BrainClient {
    http: reqwest::blocking::Client,
    base_url: String,
    token: String,
}

#[derive(serde::Deserialize)]
struct ErrorShape {
    error: ErrorDetailShape,
}

#[derive(serde::Deserialize)]
struct ErrorDetailShape {
    code: String,
    message: String,
    #[serde(default)]
    current_revision: Option<String>,
}

impl BrainClient {
    pub fn new(base_url: &str, token: &str) -> Self {
        Self {
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        }
    }

    fn url(&self, path: &str, query: &str) -> String {
        match query.is_empty() {
            true => format!("{}{path}", self.base_url),
            false => format!("{}{path}?{query}", self.base_url),
        }
    }

    fn auth(&self) -> reqwest::header::HeaderValue {
        use reqwest::header::HeaderValue;
        HeaderValue::from_str(&format!("Bearer {}", self.token)).expect("token header")
    }

    fn decode_error(&self, status: u16, body: &str) -> ClientError {
        match serde_json::from_str::<ErrorShape>(body) {
            Ok(shape) => ClientError {
                status,
                code: Some(shape.error.code),
                message: shape.error.message,
                current_revision: shape.error.current_revision,
            },
            Err(_) => ClientError {
                status,
                code: None,
                message: body.chars().take(300).collect(),
                current_revision: None,
            },
        }
    }

    fn send(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> ClientResult<reqwest::blocking::Response> {
        let response = request
            .header(reqwest::header::AUTHORIZATION, self.auth())
            .send()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        let status = response.status().as_u16();
        if status == 401 {
            let body = response.text().unwrap_or_default();
            return Err(self.decode_error(status, &body));
        }
        Ok(response)
    }

    fn expect_json<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::blocking::Response,
    ) -> ClientResult<T> {
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        if status >= 400 {
            return Err(self.decode_error(status, &text));
        }
        serde_json::from_str(&text)
            .map_err(|e| ClientError::transport(format!("malformed response: {e}")))
    }

    pub fn capabilities(&self) -> ClientResult<Capabilities> {
        let response = self.send(self.http.get(self.url("/api/v1/capabilities", "")))?;
        self.expect_json(response)
    }

    pub fn session(&self) -> ClientResult<Session> {
        let response = self.send(self.http.get(self.url("/api/v1/session", "")))?;
        self.expect_json(response)
    }

    pub fn list_files(&self, query: &str) -> ClientResult<FilePage> {
        let response = self.send(self.http.get(self.url("/api/v1/files", query)))?;
        self.expect_json(response)
    }

    pub fn get_meta(&self, path: &str) -> ClientResult<Entry> {
        let query = format!("path={}", urlencode(path));
        let response = self.send(
            self.http
                .get(self.url("/api/v1/file", &query))
                .query(&[("format", "meta")]),
        )?;
        self.expect_json(response)
    }

    pub fn get_raw(&self, path: &str) -> ClientResult<(Vec<u8>, String)> {
        let query = format!("path={}", urlencode(path));
        let response = self.send(self.http.get(self.url("/api/v1/file", &query)))?;
        let status = response.status().as_u16();
        if status >= 400 {
            let text = response.text().unwrap_or_default();
            return Err(self.decode_error(status, &text));
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = response
            .bytes()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        Ok((bytes.to_vec(), etag))
    }

    /// PUT one file's content. The SHA-256 digest is always computed locally
    /// and sent as `X-Content-SHA256` (design §22.3) — this is what makes
    /// binary media possible from CLI: the server requires the header for
    /// non-text uploads and re-verifies it server-side before CAS.
    pub fn put_content(
        &self,
        path: &str,
        body: &[u8],
        media_type: &str,
        precondition: Precondition,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult> {
        use sha2::{Digest, Sha256};
        let digest = hex::encode(Sha256::digest(body));
        let query = format!("path={}", urlencode(path));
        let mut request = self
            .http
            .put(self.url("/api/v1/file", &query))
            .header(reqwest::header::CONTENT_TYPE, media_type)
            .header("X-Content-SHA256", digest);
        request = match precondition {
            Precondition::Create => request.header(reqwest::header::IF_NONE_MATCH, "*"),
            Precondition::IfRevision(rev) => {
                request.header(reqwest::header::IF_MATCH, format!("\"{rev}\""))
            }
        };
        if let Some(op) = operation_id {
            request = request.header("X-Operation-ID", op);
        }
        let response = self.send(request.body(body.to_vec()));
        self.expect_json(response?)
    }

    pub fn delete(
        &self,
        path: &str,
        if_revision: &str,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult> {
        let query = format!("path={}", urlencode(path));
        let mut request = self
            .http
            .delete(self.url("/api/v1/file", &query))
            .header(reqwest::header::IF_MATCH, format!("\"{if_revision}\""));
        if let Some(op) = operation_id {
            request = request.header("X-Operation-ID", op);
        }
        let response = self.send(request);
        self.expect_json(response?)
    }

    pub fn history(
        &self,
        path: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> ClientResult<HistoryPage> {
        let mut query = format!("path={}", urlencode(path));
        if let Some(cursor) = cursor {
            query.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
        query.push_str(&format!("&limit={limit}"));
        let response = self.send(self.http.get(self.url("/api/v1/revisions", &query)))?;
        self.expect_json(response)
    }

    pub fn revision_meta(
        &self,
        path: &str,
        revision: &str,
    ) -> ClientResult<brainstore_api_types::RevisionDto> {
        let query = format!("path={}&revision={revision}&format=meta", urlencode(path));
        let response = self.send(self.http.get(self.url("/api/v1/revision", &query)))?;
        self.expect_json(response)
    }

    pub fn revision_raw(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>> {
        let query = format!("path={}&revision={revision}", urlencode(path));
        let response = self.send(self.http.get(self.url("/api/v1/revision", &query)))?;
        let status = response.status().as_u16();
        if status >= 400 {
            let text = response.text().unwrap_or_default();
            return Err(self.decode_error(status, &text));
        }
        let bytes = response
            .bytes()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        Ok(bytes.to_vec())
    }

    pub fn restore(
        &self,
        path: &str,
        revision: &str,
        if_current: &str,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult> {
        let mut request = self
            .http
            .post(self.url("/api/v1/restore", ""))
            .header(reqwest::header::IF_MATCH, format!("\"{if_current}\""))
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(op) = operation_id {
            request = request.header("X-Operation-ID", op);
        }
        let response = self.send(request.json(&RestoreBody {
            path: path.to_owned(),
            revision: revision.to_owned(),
        }));
        self.expect_json(response?)
    }

    pub fn search(&self, query: &str) -> ClientResult<SearchPage> {
        let response = self.send(self.http.get(self.url("/api/v1/search", query)))?;
        self.expect_json(response)
    }

    pub fn links(
        &self,
        path: &str,
        backlinks: bool,
        cursor: Option<&str>,
    ) -> ClientResult<LinkPage> {
        let direction = if backlinks { "backlinks" } else { "outbound" };
        let mut query = format!("path={}&direction={direction}", urlencode(path));
        if let Some(cursor) = cursor {
            query.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
        let response = self.send(self.http.get(self.url("/api/v1/links", &query)))?;
        self.expect_json(response)
    }

    pub fn reindex(&self, cursor: Option<&str>, limit: usize) -> ClientResult<ReindexPage> {
        let body = serde_json::json!({"cursor": cursor, "limit": limit});
        let response = self.send(
            self.http
                .post(self.url("/api/v1/maintenance/reindex", ""))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string()),
        )?;
        self.expect_json(response)
    }

    pub fn patch(
        &self,
        path: &str,
        if_revision: &str,
        body: PatchBody,
        operation_id: Option<&str>,
    ) -> ClientResult<PatchOutcomeClient> {
        let query = format!("path={}", urlencode(path));
        let mut request = self
            .http
            .patch(self.url("/api/v1/file", &query))
            .header(reqwest::header::IF_MATCH, format!("\"{if_revision}\""))
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(op) = operation_id {
            request = request.header("X-Operation-ID", op);
        }
        let response =
            self.send(request.body(serde_json::to_string(&body).expect("serializes")))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        if status >= 400 {
            return Err(self.decode_error(status, &text));
        }
        // Dry-run answers PatchDryRun; a committed patch answers MutationResult.
        if let Ok(dry) = serde_json::from_str::<PatchDryRun>(&text) {
            return Ok(PatchOutcomeClient::DryRun(dry));
        }
        serde_json::from_str::<MutationResult>(&text)
            .map(PatchOutcomeClient::Committed)
            .map_err(|e| ClientError::transport(format!("malformed response: {e}")))
    }

    pub fn transfer(&self, kind: TransferKind, body: TransferBody) -> ClientResult<TransferResult> {
        let path = match kind {
            TransferKind::Copy => "/api/v1/copy",
            TransferKind::Move => "/api/v1/move",
        };
        let response = self.send(
            self.http
                .post(self.url(path, ""))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_string(&body).expect("serializes")),
        )?;
        self.expect_json(response)
    }

    pub fn diff(&self, path: &str, from: &str, to: &str) -> ClientResult<DiffResult> {
        let query = format!("path={}&from={from}&to={to}", urlencode(path));
        let response = self.send(self.http.get(self.url("/api/v1/diff", &query)))?;
        self.expect_json(response)
    }
}

#[derive(Clone, Debug)]
pub enum Precondition {
    Create,
    IfRevision(String),
}

#[derive(Clone, Copy, Debug)]
pub enum TransferKind {
    Copy,
    Move,
}

pub enum PatchOutcomeClient {
    DryRun(PatchDryRun),
    Committed(MutationResult),
}

pub fn urlencode(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
