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
    // The generated client sends raw bytes with the explicitly declared
    // media type; the digest verifies the exact uploaded bytes.
    let created = files
        .put_file(
            "sdk/a.md".into(),
            None,
            Some("*".into()),
            None,
            Some(sha256_hex(b"# hello")),
            Some("text/markdown".into()),
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
            Some("text/markdown".into()),
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
            Some("text/markdown".into()),
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
    assert_eq!(raw.media_type.as_deref(), Some("text/markdown"));

    // Conditional read: typed NotModified with the unchanged ETag.
    let not_modified = match files
        .get_file("sdk/a.md".into(), None, Some(format!("\"{rev2}\"")))
        .await
    {
        Err(brains_sdk::FilesGetFileError::NotModified(not_modified)) => not_modified,
        _ => panic!("expected NotModified"),
    };
    assert_eq!(not_modified.etag, format!("\"{rev2}\""));

    // Preview and commit use one success contract, preserving HEAD on preview.
    let patch = |dry_run| brains_sdk::PatchBody {
        edits: vec![brains_sdk::PatchEdit {
            old_text: "v2".into(),
            new_text: "v3".into(),
            expected_matches: 1,
        }],
        dry_run: Some(dry_run),
    };
    let preview = files
        .patch_file(
            "sdk/a.md".into(),
            Some(format!("\"{rev2}\"")),
            None,
            patch(true),
        )
        .await
        .unwrap();
    assert!(preview.r#return.committed.is_none());
    assert_eq!(preview.r#return.base_revision, rev2);
    assert!(preview.r#return.diff.contains("+# hello v3"));
    let unchanged = files.get_file("sdk/a.md".into(), None, None).await.unwrap();
    assert_eq!(
        unchanged.etag.as_deref(),
        Some(format!("\"{rev2}\"").as_str())
    );
    let brains_sdk::FilePayload::OctetStream(bytes) = unchanged.r#return else {
        panic!("expected raw bytes after preview");
    };
    assert_eq!(bytes, b"# hello v2");
    let patched = files
        .patch_file(
            "sdk/a.md".into(),
            Some(format!("\"{rev2}\"")),
            None,
            patch(false),
        )
        .await
        .unwrap();
    let committed = patched.r#return.committed.unwrap();
    assert_ne!(committed.revision, rev2);
    assert_eq!(
        patched.etag.as_deref(),
        Some(format!("\"{}\"", committed.revision).as_str())
    );
    let rev2 = committed.revision;
    let after_commit = files.get_file("sdk/a.md".into(), None, None).await.unwrap();
    assert_eq!(
        after_commit.etag.as_deref(),
        Some(format!("\"{rev2}\"").as_str())
    );
    assert!(
        matches!(after_commit.r#return, brains_sdk::FilePayload::OctetStream(bytes) if bytes == b"# hello v3")
    );

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

    // Explicit media types and raw bytes round-trip through the generated SDK.
    let seed = files
        .put_file(
            "raw.md".into(),
            None,
            Some("*".into()),
            None,
            None,
            Some("text/html".into()),
            b"<b>raw</b>".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(seed.r#return.created, Some(true));
    let served = files.get_file("raw.md".into(), None, None).await.unwrap();
    assert_eq!(served.media_type.as_deref(), Some("text/html"));
    assert!(
        served
            .content_disposition
            .as_deref()
            .is_some_and(|value| value.starts_with("attachment;"))
    );
    assert!(
        matches!(served.r#return, brains_sdk::FilePayload::OctetStream(bytes) if bytes == b"<b>raw</b>")
    );
}

#[tokio::test]
async fn generated_client_preserves_permission_and_size_failures() {
    let dir = tempfile::tempdir().unwrap();
    let root = "root-business-test-token-0000000000";
    let limited = "limited-business-test-token-000000";
    let registry = serde_json::json!({"tokens": [{
        "id": "limited", "name": "limited",
        "token_sha256": sha256_hex(limited.as_bytes()),
        "scopes": ["read:allowed"], "enabled": true,
    }]});
    std::fs::write(dir.path().join("auth.json"), registry.to_string()).unwrap();
    let address = brains_server::serve(ServerConfig {
        data_dir: dir.path().to_path_buf(),
        vault: "default".into(),
        bind: "127.0.0.1".into(),
        port: 0,
        token: Some(root.into()),
        web_dist: None,
        allowed_origins: vec![],
        seed_templates: false,
    })
    .await
    .unwrap();
    let api = format!("http://{address}/api/v1");
    let limited_files = brains_sdk::FilesClient::with_http(&api, sdk_http(limited));
    assert!(matches!(
        limited_files
            .get_file("private.md".into(), None, None)
            .await,
        Err(brains_sdk::FilesGetFileError::Forbidden(_))
    ));
    let files = brains_sdk::FilesClient::with_http(&api, sdk_http(root));
    let created = files
        .put_file(
            "size.md".into(),
            None,
            Some("*".into()),
            None,
            None,
            Some("text/markdown".into()),
            b"small".to_vec(),
        )
        .await
        .unwrap();
    let revision = created.r#return.revision;
    let oversized = files
        .patch_file(
            "size.md".into(),
            Some(format!("\"{revision}\"")),
            None,
            brains_sdk::PatchBody {
                edits: vec![brains_sdk::PatchEdit {
                    old_text: "small".into(),
                    new_text: "x".repeat(8 * 1024 * 1024 + 1),
                    expected_matches: 1,
                }],
                dry_run: Some(false),
            },
        )
        .await;
    assert!(matches!(
        oversized,
        Err(brains_sdk::FilesPatchFileError::PayloadTooLarge(_))
    ));
    // The extractor's 9 MiB limit uses the framework envelope, not the domain exception.
    let rejected = files
        .put_file(
            "size.md".into(),
            Some(format!("\"{revision}\"")),
            None,
            None,
            None,
            Some("text/markdown".into()),
            vec![b'x'; 9 * 1024 * 1024 + 1],
        )
        .await;
    let Err(brains_sdk::FilesPutFileError::Framework(error)) = rejected else {
        panic!("expected framework 413 for an oversized HTTP body");
    };
    assert_eq!(error.http_status().as_u16(), 413);
    let current = files.get_file("size.md".into(), None, None).await.unwrap();
    assert_eq!(
        current.etag.as_deref(),
        Some(format!("\"{revision}\"").as_str())
    );
    let brains_sdk::FilePayload::OctetStream(bytes) = current.r#return else {
        panic!("expected raw bytes");
    };
    assert_eq!(bytes, b"small");
}
