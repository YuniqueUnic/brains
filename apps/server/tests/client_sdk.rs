//! Business-level check of the generated SDK against the generated server:
//! the typed error channel, the `@http` union reads and the out-parameter
//! headers, exercised over real HTTP exactly as an external consumer would.

use brains_server::ServerConfig;
use xidl_rust_axum::reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

fn sdk_http(token: &str) -> xidl_rust_axum::reqwest::Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    xidl_rust_axum::reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[tokio::test]
async fn generated_client_talks_to_generated_server() {
    let dir = tempfile::tempdir().unwrap();
    let config = ServerConfig {
        data_dir: dir.path().to_path_buf(),
        vault: "default".into(),
        bind: "127.0.0.1".into(),
        port: 0,
        token: Some("sdk-probe-token-0000000000000000".into()),
        web_dist: None,
        allowed_origins: vec![],
        seed_templates: false,
    };
    let addr = brains_server::serve(config).await.unwrap();
    let base = format!("http://{addr}");
    // Generated routes are relative; the server nests them under /api/v1.
    let api = format!("{base}/api/v1");
    let token = "sdk-probe-token-0000000000000000";
    let files = brains_sdk::FilesClient::with_http(&api, sdk_http(token));

    // Create: `created` travels in-band, the ETag rides the out parameter.
    // The generated client uploads raw octet bytes, so the §22.3 digest
    // header applies; the stored media type falls back to the extension.
    let created = files
        .put_file(
            "sdk/a.md".into(),
            None,
            Some("*".into()),
            None,
            Some(sha256_hex(b"# hello")),
            b"# hello".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(created.r#return.created, Some(true));
    assert_eq!(
        created.etag.as_deref(),
        Some(format!("\"{}\"", created.r#return.revision).as_str())
    );
    let rev1 = created.r#return.revision.clone();

    // Update through the typed If-Match parameter.
    let updated = files
        .put_file(
            "sdk/a.md".into(),
            Some(format!("\"{rev1}\"")),
            None,
            None,
            Some(sha256_hex(b"# hello v2")),
            b"# hello v2".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(updated.r#return.created, Some(false));
    let rev2 = updated.r#return.revision.clone();

    // Stale base: typed PreconditionFailed carrying the winner + its ETag.
    let stale = files
        .put_file(
            "sdk/a.md".into(),
            Some(format!("\"{rev1}\"")),
            None,
            None,
            Some(sha256_hex(b"stale")),
            b"stale".to_vec(),
        )
        .await;
    let conflict = match stale {
        Err(brains_sdk::FilesPutFileError::PreconditionFailed(conflict)) => conflict,
        _ => panic!("expected PreconditionFailed"),
    };
    assert_eq!(conflict.error.code, "REVISION_CONFLICT");
    assert_eq!(
        conflict.error.current_revision.as_deref(),
        Some(rev2.as_str())
    );
    assert_eq!(
        conflict.etag.as_deref(),
        Some(format!("\"{rev2}\"").as_str())
    );

    // Union read: metadata view returns the Json case with the ETag out.
    let meta = files
        .get_file("sdk/a.md".into(), Some("meta".into()), None)
        .await
        .unwrap();
    let brains_sdk::FilePayload::Json(entry) = meta.r#return else {
        panic!("expected the Json case");
    };
    assert_eq!(entry.revision, rev2);
    assert_eq!(meta.etag.as_deref(), Some(format!("\"{rev2}\"").as_str()));

    // Raw view: the OctetStream case carries the bytes and the media type.
    let raw = files.get_file("sdk/a.md".into(), None, None).await.unwrap();
    let brains_sdk::FilePayload::OctetStream(bytes) = raw.r#return else {
        panic!("expected the OctetStream case");
    };
    assert_eq!(bytes, b"# hello v2");
    // The SDK uploads declare application/octet-stream, and that header
    // wins over the extension fallback.
    assert_eq!(raw.media_type.as_deref(), Some("application/octet-stream"));

    // Conditional read: typed NotModified with the unchanged ETag.
    let not_modified = match files
        .get_file("sdk/a.md".into(), None, Some(format!("\"{rev2}\"")))
        .await
    {
        Err(brains_sdk::FilesGetFileError::NotModified(not_modified)) => not_modified,
        _ => panic!("expected NotModified"),
    };
    assert_eq!(not_modified.etag, format!("\"{rev2}\""));

    // Trash then read: typed Gone, tombstone ETag attached.
    let deleted = files
        .delete_file("sdk/a.md".into(), Some(format!("\"{rev2}\"")), None)
        .await
        .unwrap();
    let tombstone = deleted.r#return.revision.clone();
    let error = match files.get_file("sdk/a.md".into(), None, None).await {
        Err(brains_sdk::FilesGetFileError::Gone(error)) => error,
        _ => panic!("expected Gone"),
    };
    assert_eq!(error.error.code, "TRASHED");
    assert_eq!(
        error.etag.as_deref(),
        Some(format!("\"{tombstone}\"").as_str())
    );

    // Session exchange over the same transport.
    let sessions = brains_sdk::SessionsClient::with_http(&api, sdk_http(token));
    let login = sessions.login(format!("Bearer {token}")).await.unwrap();
    assert!(
        login
            .set_cookie
            .as_deref()
            .unwrap_or("")
            .starts_with("brains_session=")
    );
    let current = sessions.current().await.unwrap();
    assert_eq!(current.actor, "token:default");
    let logout = sessions.logout().await.unwrap();
    assert!(logout.r#return.ok);

    // The create request really travels as raw octet bytes (no JSON shell).
    let http = xidl_rust_axum::reqwest::Client::new();
    let seed = http
        .put(format!("{api}/file?path=raw.md"))
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header("If-None-Match", "*")
        .header("Content-Type", "text/html")
        .body("<b>raw</b>")
        .send()
        .await
        .unwrap();
    assert_eq!(
        seed.status(),
        200,
        "{}",
        seed.text().await.unwrap_or_default()
    );
    let served = http
        .get(format!("{api}/file?path=raw.md"))
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(served.bytes().await.unwrap(), "<b>raw</b>");
}
