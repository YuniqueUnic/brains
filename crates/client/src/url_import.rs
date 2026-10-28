//! URL → Markdown note import (issue #22): a fetch-and-render ladder that
//! stops at the first step yielding real content. All network policy is
//! configurable; only the defaults are strict (https only, private and
//! metadata targets refused, robots.txt respected, conservative size and
//! redirect limits). Fetched content is data, never instructions: nothing
//! from a page sets note paths, frontmatter or commands.
//!
//! Native steps: 1) content negotiation (`Accept: text/markdown`),
//! 2) static HTML through Readability + HTML-to-Markdown, 3) rendered HTML
//! via the optional external `moli` tool when found on `PATH`, 4) documents
//! (PDF…) stored as attachments for `bsx convert`. These steps also work
//! with a remote Worker as the storage target. Remote cloud conversion and
//! URL import are selected by the CLI when advertised; Browser Run is optional.

use std::io::Read;
use std::net::IpAddr;

use serde::Serialize;

use crate::{ClientError, ClientResult, Library, Precondition};

const USER_AGENT: &str = concat!("brains-bsx/", env!("CARGO_PKG_VERSION"));
const MAX_REDIRECTS: usize = 5;
const DEFAULT_MAX_BYTES: usize = 10 * 1024 * 1024;
const MIN_RENDERED_TEXT: usize = 200;

/// Effective network policy (issue #22 table). Defaults refuse private,
/// loopback and link-local targets — including cloud metadata endpoints,
/// which are not overridable.
#[derive(Clone, Debug)]
pub struct NetworkPolicy {
    pub allowed_schemes: Vec<String>,
    /// Hosts or exact IPs exempt from the private-target refusal. Cloud
    /// metadata endpoints (169.254.169.254, fd00:ec2::254) stay refused even
    /// when allowlisted.
    pub allow_hosts: Vec<String>,
    pub respect_robots: bool,
    pub max_bytes: usize,
    pub timeout_secs: u64,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            allowed_schemes: vec!["https".into()],
            allow_hosts: Vec::new(),
            respect_robots: true,
            max_bytes: DEFAULT_MAX_BYTES,
            timeout_secs: 30,
        }
    }
}

/// Which ladder step produced the content; recorded in frontmatter and the
/// command output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LadderStep {
    MarkdownNegotiation,
    StaticHtml,
    RenderedHtml,
    Document,
}

impl LadderStep {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MarkdownNegotiation => "markdown-negotiation",
            Self::StaticHtml => "static-html",
            Self::RenderedHtml => "rendered-html",
            Self::Document => "document",
        }
    }
}

pub struct UrlImportResult {
    pub note_path: String,
    pub note_revision: String,
    pub final_url: String,
    pub title: Option<String>,
    pub step: LadderStep,
    pub created: bool,
    /// Set when the URL served a document type and the companion conversion
    /// needs `bsx convert` (unavailable builds store the attachment only).
    pub companion_hint: Option<String>,
}

pub struct UrlImportInput<'a> {
    pub url: &'a str,
    pub path: &'a str,
    pub policy: NetworkPolicy,
    /// Force the rendered step even when static HTML yields text.
    pub render: bool,
    /// Skip the robots.txt check for this request.
    pub ignore_robots: bool,
}

pub fn import_url(
    client: &dyn Library,
    input: UrlImportInput<'_>,
) -> ClientResult<UrlImportResult> {
    let requested = parse_and_check_url(input.url, &input.policy)?;
    // The policy check precedes ANY network traffic, robots.txt included.
    // Resolved addresses are pinned into the request client (see
    // `check_url_host`), so the connection dials exactly the IPs that were
    // validated — a second, divergent resolution between check and connect
    // is impossible.
    let pins = check_url_host(&requested, &input.policy)?;
    if input.policy.respect_robots
        && !input.ignore_robots
        && robots_disallow_all(&requested, &pins, &input.policy)?
    {
        return Err(ClientError {
            status: 403,
            code: Some("ROBOTS_DISALLOWED".into()),
            message: format!(
                "{requested} disallows crawling in robots.txt (--ignore-robots overrides)"
            ),
            current_revision: None,
        });
    }
    // Manual redirect loop: every hop is re-checked against the policy, and
    // its validated addresses are pinned for that hop's request.
    let mut current = requested.clone();
    let mut hops = 0usize;
    let response = loop {
        let pins = check_url_host(&current, &input.policy)?;
        let http = http_client(&input.policy, &pins)?;
        let next = fetch_once(&http, &current, &input.policy)?;
        if next.status().is_redirection() {
            hops += 1;
            if hops > MAX_REDIRECTS {
                return Err(ClientError {
                    status: 508,
                    code: Some("TOO_MANY_REDIRECTS".into()),
                    message: format!("more than {MAX_REDIRECTS} redirects"),
                    current_revision: None,
                });
            }
            let location = next
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| ClientError {
                    status: 502,
                    code: Some("BAD_REDIRECT".into()),
                    message: "redirect without Location".into(),
                    current_revision: None,
                })?;
            current = resolve_redirect(&current, location)?;
            continue;
        }
        break next;
    };
    let final_url = current;
    let status = response.status();
    if !status.is_success() {
        return Err(ClientError {
            status: 502,
            code: Some("FETCH_FAILED".into()),
            message: format!("fetch {final_url}: HTTP {status}"),
            current_revision: None,
        });
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let bytes = bounded_body(response, input.policy.max_bytes)?;

    // Ladder step 4: documents. The attachment lands at the requested path;
    // conversion to a companion note is a separate, explicit step.
    if !content_type.starts_with("text/") && content_type != "application/json" {
        if content_type != "application/pdf" {
            return Err(ClientError {
                status: 422,
                code: Some("UNSUPPORTED_MEDIA_TYPE".into()),
                message: format!(
                    "{final_url} serves `{content_type}`, which is not a page or a convertible document"
                ),
                current_revision: None,
            });
        }
        return store_document(client, input.path, &bytes, &content_type, &final_url);
    }

    // Ladder steps 1–3 for pages.
    let (markdown, title, step) = page_ladder(&bytes, &content_type, &input, &final_url)?;
    store_page(
        client, input.path, &markdown, &title, &requested, &final_url, step, &bytes,
    )
}

fn http_client(
    policy: &NetworkPolicy,
    pins: &[(String, std::net::SocketAddr)],
) -> ClientResult<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(policy.timeout_secs));
    for (host, address) in pins {
        builder = builder.resolve(host.as_str(), *address);
    }
    builder
        .build()
        .map_err(|e| ClientError::transport(e.to_string()))
}

fn parse_and_check_url(raw: &str, policy: &NetworkPolicy) -> ClientResult<String> {
    let url = url::Url::parse(raw).map_err(|error| ClientError {
        status: 400,
        code: Some("INVALID_ARGUMENT".into()),
        message: format!("invalid URL `{raw}`: {error}"),
        current_revision: None,
    })?;
    let scheme = url.scheme().to_lowercase();
    if !policy.allowed_schemes.contains(&scheme) {
        return Err(ClientError {
            status: 400,
            code: Some("INVALID_ARGUMENT".into()),
            message: format!(
                "scheme `{scheme}` is not allowed (allowed: {}); raise the policy explicitly",
                policy.allowed_schemes.join(", ")
            ),
            current_revision: None,
        });
    }
    url.host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| ClientError {
            status: 400,
            code: Some("INVALID_ARGUMENT".into()),
            message: format!("URL `{raw}` has no host"),
            current_revision: None,
        })?;
    Ok(url.to_string())
}

/// Resolve the host and refuse private/loopback/link-local targets unless
/// explicitly allowlisted. Metadata endpoints stay refused regardless.
/// Returns the validated `(host, address)` pairs for DNS pinning: the caller
/// dials exactly these, closing the resolve-between-check-and-connect gap.
fn check_url_host(
    raw: &str,
    policy: &NetworkPolicy,
) -> ClientResult<Vec<(String, std::net::SocketAddr)>> {
    let url = url::Url::parse(raw).map_err(|e| ClientError::transport(e.to_string()))?;
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| ClientError::transport("URL without host".to_owned()))?
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(443);
    let sockets: Vec<std::net::SocketAddr> = if let Ok(ip) = host.parse() {
        vec![std::net::SocketAddr::new(ip, port)]
    } else {
        use std::net::ToSocketAddrs;
        (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| ClientError::transport(format!("DNS resolution failed for {host}: {e}")))?
            .collect()
    };
    let mut pins = Vec::new();
    for socket in sockets {
        let address = socket.ip();
        if metadata_endpoint(&address) {
            return Err(ClientError {
                status: 403,
                code: Some("FORBIDDEN_TARGET".into()),
                message: format!(
                    "{host} resolves to a cloud metadata endpoint ({address}); never fetchable"
                ),
                current_revision: None,
            });
        }
        if is_private_target(&address) && !policy.allow_hosts.contains(&host) {
            return Err(ClientError {
                status: 403,
                code: Some("FORBIDDEN_TARGET".into()),
                message: format!(
                    "{host} resolves to a private, loopback or link-local address ({address}); allowlist the host explicitly to fetch it"
                ),
                current_revision: None,
            });
        }
        pins.push((host.clone(), socket));
    }
    Ok(pins)
}

/// Cloud metadata endpoints are never fetchable, on any policy (issue #22).
/// AWS/Azure/GCP/Alibaba well-known addresses; IPv4-mapped IPv6 forms are
/// normalized first so `::ffff:169.254.169.254` cannot slip through.
fn metadata_endpoint(address: &IpAddr) -> bool {
    let address = match address {
        IpAddr::V4(ip) => IpAddr::V4(*ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(ip) => IpAddr::V4(ip),
            None => {
                return ip.to_string() == "fd00:ec2::254";
            }
        },
    };
    matches!(
        address.to_string().as_str(),
        "169.254.169.254" | "168.63.129.16" | "100.100.100.200"
    )
}

fn is_private_target(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback() || ip.is_unspecified() || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

fn resolve_redirect(base: &str, location: &str) -> ClientResult<String> {
    let base = url::Url::parse(base).map_err(|e| ClientError::transport(e.to_string()))?;
    let next = base.join(location).map_err(|e| ClientError {
        status: 502,
        code: Some("BAD_REDIRECT".into()),
        message: format!("unresolvable redirect target `{location}`: {e}"),
        current_revision: None,
    })?;
    Ok(next.to_string())
}

fn fetch_once(
    http: &reqwest::blocking::Client,
    url: &str,
    policy: &NetworkPolicy,
) -> ClientResult<reqwest::blocking::Response> {
    // Step 1 of the ladder: ask for Markdown first.
    http.get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/markdown, text/html;q=0.9, */*;q=0.1",
        )
        .timeout(std::time::Duration::from_secs(policy.timeout_secs))
        .send()
        .map_err(|e| ClientError::transport(format!("fetch {url}: {e}")))
}

fn bounded_body(
    mut response: reqwest::blocking::Response,
    max_bytes: usize,
) -> ClientResult<Vec<u8>> {
    if let Some(length) = response.content_length()
        && length as usize > max_bytes
    {
        return Err(ClientError {
            status: 413,
            code: Some("TOO_LARGE".into()),
            message: format!("response of {length} bytes exceeds the {max_bytes} byte limit"),
            current_revision: None,
        });
    }
    let mut body = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = response
            .read(&mut chunk)
            .map_err(|e| ClientError::transport(e.to_string()))?;
        if read == 0 {
            break;
        }
        if body.len() + read > max_bytes {
            return Err(ClientError {
                status: 413,
                code: Some("TOO_LARGE".into()),
                message: format!("response exceeds the {max_bytes} byte limit"),
                current_revision: None,
            });
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(body)
}

/// `User-agent: *` with `Disallow: /` means "not for crawlers". A minimal
/// line parser; anything ambiguous allows the fetch.
fn robots_disallow_all(
    url: &str,
    pins: &[(String, std::net::SocketAddr)],
    policy: &NetworkPolicy,
) -> ClientResult<bool> {
    let base = url::Url::parse(url).map_err(|e| ClientError::transport(e.to_string()))?;
    let mut robots = base.clone();
    robots.set_path("/robots.txt");
    // Same policy and DNS pins as the page fetch — allowlist, size bound and
    // scheme all carry over; only the idle timeout tightens for this side
    // request. Failures are normalized so the probe cannot distinguish
    // refusal from reachability.
    let policy = NetworkPolicy {
        timeout_secs: policy.timeout_secs.min(10),
        ..policy.clone()
    };
    let http = http_client(&policy, pins)?;
    let response = match http
        .get(robots.clone())
        .header(reqwest::header::ACCEPT, "text/plain")
        .send()
    {
        Ok(response) => response,
        Err(_) => return Ok(false),
    };
    if !response.status().is_success() {
        return Ok(false);
    }
    let body = bounded_body(response, policy.max_bytes)?;
    let text = String::from_utf8_lossy(&body).into_owned();
    let mut applies_to_all = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(agent) = line.strip_prefix("User-agent:") {
            applies_to_all = agent.trim() == "*";
        } else if applies_to_all
            && let Some(disallow) = line.strip_prefix("Disallow:")
            && disallow.trim() == "/"
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Ladder steps 1–3. `Accept: text/markdown` already answered with markdown
/// (step 1) when the content type says so; otherwise Readability + HTML→
/// Markdown (step 2); if that yields too little text and `moli` is on PATH
/// (or --render forced it), the rendered step answers (step 3).
fn page_ladder(
    bytes: &[u8],
    content_type: &str,
    input: &UrlImportInput<'_>,
    final_url: &str,
) -> ClientResult<(String, Option<String>, LadderStep)> {
    if content_type.contains("markdown") {
        let text = decode_utf8(bytes)?;
        return Ok((text, None, LadderStep::MarkdownNegotiation));
    }
    let html = decode_utf8(bytes)?;
    let (markdown, title) = html_to_markdown(&html)?;
    let enough = markdown.trim().len() >= MIN_RENDERED_TEXT;
    if !enough || input.render {
        if let Some(rendered) = render_with_moli(final_url)? {
            let markdown = html_to_markdown(&rendered)
                .map(|(markdown, _)| markdown)
                .unwrap_or(rendered);
            return Ok((markdown, title, LadderStep::RenderedHtml));
        }
        if input.render {
            return Err(ClientError {
                status: 501,
                code: Some("CAPABILITY_UNSUPPORTED".into()),
                message: "rendering: unavailable — the external `moli` tool is not on PATH".into(),
                current_revision: None,
            });
        }
    }
    Ok((markdown, title, LadderStep::StaticHtml))
}

fn decode_utf8(bytes: &[u8]) -> ClientResult<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| ClientError {
        status: 422,
        code: Some("INVALID_FORMAT".into()),
        message: "page is not valid UTF-8".into(),
        current_revision: None,
    })
}

/// Readability extraction + HTML-to-Markdown conversion (issue #22 step 2).
fn html_to_markdown(html: &str) -> ClientResult<(String, Option<String>)> {
    let extraction = dom_smoothie::Readability::new(html, None, None)
        .map_err(|e| ClientError {
            status: 422,
            code: Some("INVALID_FORMAT".into()),
            message: format!("unreadable page: {e}"),
            current_revision: None,
        })?
        .parse()
        .map_err(|e| ClientError {
            status: 422,
            code: Some("INVALID_FORMAT".into()),
            message: format!("readability extraction failed: {e}"),
            current_revision: None,
        })?;
    let title = (!extraction.title.is_empty()).then_some(extraction.title);
    let markdown = htmd::convert(&extraction.content).map_err(|e| ClientError {
        status: 422,
        code: Some("INVALID_FORMAT".into()),
        message: format!("html-to-markdown conversion failed: {e}"),
        current_revision: None,
    })?;
    Ok((markdown, title))
}

/// Step 3: the optional external renderer, invoked from PATH, never linked.
fn render_with_moli(url: &str) -> ClientResult<Option<String>> {
    let output = std::process::Command::new("moli")
        .args(["fetch", "--dump", "markdown", url])
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
        }
        Ok(output) => Err(ClientError {
            status: 502,
            code: Some("RENDER_FAILED".into()),
            message: format!(
                "moli render failed: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(200)
                    .collect::<String>()
            ),
            current_revision: None,
        }),
        // Not installed (or not executable): the step reports unavailable and
        // the ladder stays on steps 1, 2 and 4.
        Err(_) => Ok(None),
    }
}

/// Ladder step 4: store the ORIGINAL bytes as an attachment — documents are
/// binary, so provenance lives in the companion note that `bsx convert`
/// later writes (issue #19), never in a rewritten attachment.
fn store_document(
    client: &dyn Library,
    path: &str,
    bytes: &[u8],
    media_type: &str,
    final_url: &str,
) -> ClientResult<UrlImportResult> {
    let note_path = path.to_owned();
    let precondition = put_precondition(client, &note_path)?;
    let result = client.put_content(&note_path, bytes, media_type, None, precondition, None)?;
    Ok(UrlImportResult {
        note_path: note_path.clone(),
        note_revision: result.entry.revision,
        final_url: final_url.to_owned(),
        title: None,
        step: LadderStep::Document,
        created: result.entry.generation == 1,
        companion_hint: Some(format!(
            "attachment stored at {note_path}; run `bsx convert {note_path}` to produce a searchable companion note"
        )),
    })
}

#[allow(clippy::too_many_arguments)]
fn store_page(
    client: &dyn Library,
    path: &str,
    markdown: &str,
    title: &Option<String>,
    requested: &str,
    final_url: &str,
    step: LadderStep,
    raw: &[u8],
) -> ClientResult<UrlImportResult> {
    // Fetched content is data: the note body is fenced off from the note's
    // own frontmatter by construction (we write the frontmatter ourselves).
    let now = brains_domain::timestamp::now_rfc3339();
    let frontmatter = provenance_frontmatter(requested, final_url, title.as_deref(), step, &now);
    let mut body = frontmatter;
    if let Some(title) = title {
        body.push_str(&format!("# {title}\n\n"));
    }
    body.push_str(markdown.trim_end());
    body.push('\n');
    let precondition = put_precondition(client, path)?;
    let result = client.put_content(
        path,
        body.as_bytes(),
        "text/markdown",
        None,
        precondition,
        None,
    )?;
    let _ = raw;
    Ok(UrlImportResult {
        note_path: path.to_owned(),
        note_revision: result.entry.revision,
        final_url: final_url.to_owned(),
        title: title.clone(),
        step,
        created: result.entry.generation == 1,
        companion_hint: None,
    })
}

/// `status: draft` until reviewed; `sources[].via` names the ladder step so
/// the origin stays explainable. Extra keys inside a sources entry are
/// preserved by the parser without warnings.
fn provenance_frontmatter(
    requested: &str,
    final_url: &str,
    title: Option<&str>,
    step: LadderStep,
    now: &str,
) -> String {
    let mut frontmatter = String::from("---\ntype: Imported Page\nstatus: draft\n");
    if let Some(title) = title {
        frontmatter.push_str(&format!("title: {}\n", yaml_quote(title)));
    }
    frontmatter.push_str(&format!(
        "generated:\n  by: bsx-url-import/{}\n  at: {now}\n",
        step.as_str()
    ));
    frontmatter.push_str("sources:\n  - id: page\n");
    frontmatter.push_str(&format!("    resource: {}\n", yaml_quote(final_url)));
    frontmatter.push_str(&format!("    via: {}\n", step.as_str()));
    frontmatter.push_str(&format!("    fetched_at: \"{now}\"\n"));
    if requested != final_url {
        frontmatter.push_str("  - id: requested\n");
        frontmatter.push_str(&format!("    resource: {}\n", yaml_quote(requested)));
    }
    frontmatter.push_str("---\n\n");
    frontmatter
}

fn put_precondition(client: &dyn Library, path: &str) -> ClientResult<Precondition> {
    match client.get_meta(path) {
        Ok(existing) => Ok(Precondition::IfRevision(existing.revision)),
        Err(ClientError { status: 404, .. }) => Ok(Precondition::Create),
        Err(ClientError { status: 410, .. }) => Err(ClientError {
            status: 410,
            code: Some("TRASHED".into()),
            message: format!("{path} is trashed; restore it before re-importing"),
            current_revision: None,
        }),
        Err(error) => Err(error),
    }
}

use crate::convert::yaml_scalar as yaml_quote;
