//! Black-box HTTP contract tests (design §11): the same wire behavior the
//! CLI, curl and the web app depend on, exercised against the real router.

use serde_json::{Value, json};

async fn boot() -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let config = brains_server::ServerConfig {
        data_dir: dir.path().to_path_buf(),
        vault: "default".into(),
        bind: "127.0.0.1".into(),
        port: 0,
        token: Some("contract-token-0123456789abcdef".into()),
        web_dist: None,
        allowed_origins: vec!["http://localhost:5173".into()],
    };
    let addr = brains_server::serve(config).await.unwrap();
    // Keep the tempdir alive for the process lifetime of the test.
    std::mem::forget(dir);
    (
        format!("http://{addr}"),
        "contract-token-0123456789abcdef".into(),
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test]
async fn conditional_write_contract_end_to_end() {
    let (base, token) = boot().await;
    let http = client();
    let auth = format!("Bearer {token}");
    let url = |path: &str| format!("{base}/api/v1/file?path={path}");

    // Create requires If-None-Match: *; missing precondition is 428.
    let no_precondition = http
        .put(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .header("Content-Type", "text/markdown")
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(no_precondition.status(), 428);

    let created = http
        .put(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .header("If-None-Match", "*")
        .header("Content-Type", "text/markdown")
        .header("X-Operation-ID", "11112222333344445555666677778888")
        .body("第一版 缓存")
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let body: Value = created.json().await.unwrap();
    assert_eq!(body["index_state"], "ready");
    assert_eq!(body["operation_id"], "11112222333344445555666677778888");
    let rev1 = body["revision"].as_str().unwrap().to_owned();

    // Update against rev1 returns 200 and a new ETag.
    let updated = http
        .put(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{rev1}\""))
        .header("Content-Type", "text/markdown")
        .body("第二版")
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    let rev2 = updated.json::<Value>().await.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    // Stale If-Match is 412 with the winner in the body.
    let conflict = http
        .put(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{rev1}\""))
        .header("Content-Type", "text/markdown")
        .body("stale")
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), 412);
    let conflict_body: Value = conflict.json().await.unwrap();
    assert_eq!(conflict_body["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(conflict_body["error"]["current_revision"], rev2);

    // GET carries the strong logical ETag; If-None-Match yields 304.
    let get = http
        .get(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    assert_eq!(get.headers()["ETag"], format!("\"{rev2}\""));
    assert_eq!(get.headers()["Cache-Control"], "private, no-store");
    let cached = http
        .get(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .header("If-None-Match", format!("\"{rev2}\""))
        .send()
        .await
        .unwrap();
    assert_eq!(cached.status(), 304);

    // HEAD /file has no body; errors surface as X-Brain-Error-Code.
    let head = http
        .head(url("missing.md"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), 404);
    assert_eq!(head.headers()["X-Brain-Error-Code"], "NOT_FOUND");
    assert_eq!(head.bytes().await.unwrap().len(), 0);

    // Trash needs If-Match; reading a tombstone is 410 with its ETag.
    let trashed = http
        .delete(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{rev2}\""))
        .send()
        .await
        .unwrap();
    assert_eq!(trashed.status(), 200);
    let tombstone = trashed.json::<Value>().await.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let gone = http
        .get(url("inbox%2Fa.md"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 410);
    assert_eq!(gone.headers()["ETag"], format!("\"{tombstone}\""));

    // Restore from history creates a new revision, HEAD never rewinds.
    let restored = http
        .post(format!("{base}/api/v1/restore"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{tombstone}\""))
        .json(&json!({"path": "inbox/a.md", "revision": rev1}))
        .send()
        .await
        .unwrap();
    assert_eq!(restored.status(), 200);
    let restored_body = restored.json::<Value>().await.unwrap();
    assert_eq!(restored_body["generation"], 4);
}

#[tokio::test]
async fn search_contract_reports_windows_and_status() {
    let (base, token) = boot().await;
    let http = client();
    let auth = format!("Bearer {token}");
    for i in 0..5 {
        http.put(format!("{base}/api/v1/file?path=n{i}.md"))
            .header("Authorization", &auth)
            .header("If-None-Match", "*")
            .header("Content-Type", "text/markdown")
            .body(format!("缓存条目 {i}"))
            .send()
            .await
            .unwrap();
    }
    let page: Value = http
        .get(format!("{base}/api/v1/search?q=缓存&scan_limit=3"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["complete"], false);
    assert_eq!(page["scanned"], 3);
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    assert!(page["next_cursor"].is_string());

    // Bad FTS syntax is 400, not a fake-empty page.
    let bad = http
        .get(format!("{base}/api/v1/search?q=%22unclosed&mode=fts"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    assert_eq!(
        bad.json::<Value>().await.unwrap()["error"]["code"],
        "INVALID_ARGUMENT"
    );
}

#[tokio::test]
async fn session_cookie_flow_with_csrf_and_origin() {
    let (base, token) = boot().await;
    let http = client();
    let auth = format!("Bearer {token}");

    // Bearer exchange issues an HttpOnly session cookie plus CSRF token.
    let login = http
        .post(format!("{base}/api/v1/session"))
        .header("Authorization", &auth)
        .header("Origin", "http://localhost:5173")
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let login_headers = login.headers().clone();
    let session: Value = login.json().await.unwrap();
    assert_eq!(session["actor"], "token:default");
    let csrf = session["csrf_token"].as_str().unwrap().to_owned();
    let set_cookie = login_headers["set-cookie"].to_str().unwrap().to_owned();
    assert!(set_cookie.starts_with("brains_session="));
    assert!(set_cookie.contains("HttpOnly"));

    let cookie = set_cookie.split(';').next().unwrap().to_owned();

    // Cookie-authenticated GET works without CSRF.
    let who = http
        .get(format!("{base}/api/v1/session"))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(who.status(), 200);

    // Cookie mutation without CSRF is rejected.
    let no_csrf = http
        .put(format!("{base}/api/v1/file?path=web.md"))
        .header("Cookie", &cookie)
        .header("Origin", "http://localhost:5173")
        .header("Content-Type", "text/markdown")
        .header("If-None-Match", "*")
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(no_csrf.status(), 403);

    // With CSRF token and allowed Origin the same write succeeds.
    let with_csrf = http
        .put(format!("{base}/api/v1/file?path=web.md"))
        .header("Cookie", &cookie)
        .header("Origin", "http://localhost:5173")
        .header("X-CSRF-Token", &csrf)
        .header("Content-Type", "text/markdown")
        .header("If-None-Match", "*")
        .body("from the browser")
        .send()
        .await
        .unwrap();
    assert_eq!(with_csrf.status(), 201);

    // Cross-origin cookie mutation with a foreign Origin is forbidden.
    let foreign = http
        .put(format!("{base}/api/v1/file?path=web.md"))
        .header("Cookie", &cookie)
        .header("Origin", "http://evil.example")
        .header("X-CSRF-Token", &csrf)
        .header("Content-Type", "text/markdown")
        .header("If-Match", "\"anything\"")
        .body("y")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);

    // Logout clears the cookie (and requires CSRF for the cookie session).
    let logout = http
        .delete(format!("{base}/api/v1/session"))
        .header("Cookie", &cookie)
        .header("Origin", "http://localhost:5173")
        .header("X-CSRF-Token", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 200);
}

#[tokio::test]
async fn capabilities_and_undelivered_routes() {
    let (base, token) = boot().await;
    let http = client();
    let auth = format!("Bearer {token}");

    let caps: Value = http
        .get(format!("{base}/api/v1/capabilities"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(caps["api_version"], "1");
    assert_eq!(caps["store_format"], 1);
    assert_eq!(caps["runtime"], "local-rust");
    assert_eq!(caps["vault"], "default");
    assert_eq!(caps["features"]["server_side_validate"], false);
    assert_eq!(caps["features"]["literal_search"], true);

    // Undelivered capability answers 501 CAPABILITY_UNSUPPORTED.
    let validate = http
        .post(format!("{base}/api/v1/maintenance/validate"))
        .header("Authorization", &auth)
        .json(&json!({"cursor": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(validate.status(), 501);
    assert_eq!(
        validate.json::<Value>().await.unwrap()["error"]["code"],
        "CAPABILITY_UNSUPPORTED"
    );

    // Unknown API routes stay JSON 404 and never serve a web shell.
    let unknown = http
        .get(format!("{base}/api/v1/definitely-not-a-route"))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);
    assert_eq!(
        unknown.json::<Value>().await.unwrap()["error"]["code"],
        "NOT_FOUND"
    );
}

#[tokio::test]
async fn unauthenticated_requests_are_rejected_with_json() {
    let (base, _token) = boot().await;
    let http = client();
    let files = http
        .get(format!("{base}/api/v1/files"))
        .send()
        .await
        .unwrap();
    assert_eq!(files.status(), 401);
    assert_eq!(
        files.json::<Value>().await.unwrap()["error"]["code"],
        "UNAUTHENTICATED"
    );
}

#[tokio::test]
async fn patch_copy_move_diff_contract() {
    let (base, token) = boot().await;
    let http = client();
    let auth = format!("Bearer {token}");

    let created = http
        .put(format!("{base}/api/v1/file?path=doc.md"))
        .header("Authorization", &auth)
        .header("If-None-Match", "*")
        .header("Content-Type", "text/markdown")
        .body("alpha beta beta")
        .send()
        .await
        .unwrap();
    let rev = created.json::<Value>().await.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    // Dry-run patch returns a diff and no commit.
    let dry = http
        .patch(format!("{base}/api/v1/file?path=doc.md"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{rev}\""))
        .json(&json!({"edits": [{"old_text": "alpha", "new_text": "ALPHA", "expected_matches": 1}], "dry_run": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(dry.status(), 200);
    let dry_body = dry.json::<Value>().await.unwrap();
    assert!(dry_body["diff"].as_str().unwrap().contains("+ALPHA"));
    assert_eq!(dry_body["base_revision"], rev);

    // Committed patch.
    let patched = http
        .patch(format!("{base}/api/v1/file?path=doc.md"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{rev}\""))
        .json(
            &json!({"edits": [{"old_text": "alpha", "new_text": "ALPHA", "expected_matches": 1}]}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(patched.status(), 200);
    let rev2 = patched.json::<Value>().await.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    // Ambiguous edit is 409 EDIT_MISMATCH ("beta" occurs twice).
    let ambiguous = http
        .patch(format!("{base}/api/v1/file?path=doc.md"))
        .header("Authorization", &auth)
        .header("If-Match", format!("\"{rev2}\""))
        .json(&json!({"edits": [{"old_text": "beta", "new_text": "z", "expected_matches": 1}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(ambiguous.status(), 409);
    assert_eq!(
        ambiguous.json::<Value>().await.unwrap()["error"]["code"],
        "EDIT_MISMATCH"
    );

    // Copy is create-only with provenance; move trashes the source.
    let copied = http
        .post(format!("{base}/api/v1/copy"))
        .header("Authorization", &auth)
        .json(&json!({
            "source": "doc.md",
            "target": "copy.md",
            "expected_source_revision": rev2,
            "operation_id": "aaaabbbbccccddddeeeeffff00001111"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(copied.status(), 200);
    let copy_body = copied.json::<Value>().await.unwrap();
    assert_eq!(copy_body["copied"], true);
    assert_eq!(copy_body["partial"], false);

    let moved = http
        .post(format!("{base}/api/v1/move"))
        .header("Authorization", &auth)
        .json(&json!({
            "source": "copy.md",
            "target": "moved.md",
            "expected_source_revision": copy_body["target_revision"],
            "operation_id": "aaaabbbbccccddddeeeeffff00002222"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(moved.status(), 200);
    assert_eq!(moved.json::<Value>().await.unwrap()["source_trashed"], true);

    // Diff between two revisions of one path.
    let diff = http
        .get(format!(
            "{base}/api/v1/diff?path=doc.md&from={rev}&to={rev2}"
        ))
        .header("Authorization", &auth)
        .send()
        .await
        .unwrap();
    assert_eq!(diff.status(), 200);
    assert!(
        diff.json::<Value>().await.unwrap()["diff"]
            .as_str()
            .unwrap()
            .contains("+ALPHA")
    );
}
