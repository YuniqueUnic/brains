//! CLI contract tests (design §12): stdout/stderr hygiene and the stable
//! exit-code table, driven against a real in-process server.

use assert_cmd::cargo::cargo_bin;
use std::process::{Command, Stdio};
use std::time::Duration;

struct TestServer {
    url: String,
    token: String,
}

fn boot_server() -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    // The server runs on a dedicated thread whose runtime stays alive for
    // the whole test; the tempdir is leaked for the same reason.
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let data_dir = dir.path().to_path_buf();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let config = brainstore_server::ServerConfig {
            data_dir,
            vault: "default".into(),
            bind: "127.0.0.1".into(),
            port: 0,
            token: Some("cli-contract-token-0123456789".into()),
            web_dist: None,
            allowed_origins: Vec::new(),
        };
        let addr = runtime.block_on(brainstore_server::serve(config)).unwrap();
        addr_tx.send(addr).unwrap();
        // Park the runtime thread forever; the test process exits around it.
        loop {
            std::thread::park();
        }
    });
    let addr = addr_rx.recv().unwrap();
    std::mem::forget(dir);
    std::thread::sleep(Duration::from_millis(100));
    TestServer {
        url: format!("http://{addr}"),
        token: "cli-contract-token-0123456789".into(),
    }
}

fn brain(server: &TestServer) -> Command {
    let mut command = Command::new(cargo_bin("brainstore-cli"));
    command
        .env("BRAIN_URL", &server.url)
        .env("BRAIN_TOKEN", &server.token);
    command
}

fn first_revision(server: &TestServer, path: &str) -> Option<String> {
    let output = brain(server)
        .args(["stat", path, "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(value["revision"].as_str()?.to_owned())
}

#[test]
fn put_cat_search_rm_exit_code_contract() {
    let server = boot_server();

    // Unconditional writes are refused with the precondition exit code.
    let output = brain(&server)
        .args(["put", "notes/a.md"])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    // clap exits 2 on argument errors; a missing --create/--if-revision is
    // a runtime 428 → exit 2 per the mapping (input error).
    assert_eq!(output.status.code().unwrap(), 2);

    // Create succeeds with --create (exit 0).
    let output = brain(&server)
        .args([
            "put",
            "notes/a.md",
            "--create",
            "--content-type",
            "text/markdown",
        ])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    // --create without --from reads stdin; empty stdin stores an empty file.
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "stderr: {:?}",
        output.stderr
    );

    let rev = first_revision(&server, "notes/a.md").unwrap();

    // Update via --if-revision succeeds.
    let mut put = brain(&server)
        .args([
            "put",
            "notes/a.md",
            "--from",
            "/dev/stdin",
            "--if-revision",
            &rev,
            "--content-type",
            "text/markdown",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    put.stdin
        .as_mut()
        .unwrap()
        .write_all("缓存策略 v2".as_bytes())
        .unwrap();
    let output = put.wait_with_output().unwrap();
    assert_eq!(output.status.code().unwrap(), 0);

    // The stale revision now conflicts → exit 5.
    let output = brain(&server)
        .args([
            "put",
            "notes/a.md",
            "--from",
            "/dev/stdin",
            "--if-revision",
            &rev,
            "--content-type",
            "text/markdown",
        ])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 5);
    assert!(String::from_utf8_lossy(&output.stderr).contains("current revision"));

    // Search hit → 0; complete miss → 1.
    let output = brain(&server).args(["search", "缓存"]).output().unwrap();
    assert_eq!(output.status.code().unwrap(), 0);
    let output = brain(&server)
        .args(["search", "绝不存在的词"])
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 1);

    // cat of a missing file → 3 with empty stdout.
    let output = brain(&server).args(["cat", "nope.md"]).output().unwrap();
    assert_eq!(output.status.code().unwrap(), 3);
    assert!(output.stdout.is_empty());

    // rm needs the current revision; cat on a trashed file is exit 3.
    let rev = first_revision(&server, "notes/a.md").unwrap();
    let output = brain(&server)
        .args(["rm", "notes/a.md", "--if-revision", &rev])
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 0);
    let output = brain(&server).args(["cat", "notes/a.md"]).output().unwrap();
    assert_eq!(output.status.code().unwrap(), 3);

    // Search over a trash-only vault still finds the tombstone's old body? No:
    // the index row is deleted, so this is a complete miss → exit 1. The term
    // comes from the trashed file's own body; template notes must not match.
    let output = brain(&server)
        .args(["search", "缓存策略v2"])
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 1);
}

#[test]
fn export_produces_a_readable_working_tree() {
    let server = boot_server();
    let rev_a = {
        let mut put = brain(&server)
            .args([
                "put",
                "kb/note.md",
                "--create",
                "--content-type",
                "text/markdown",
                "--from",
                "/dev/stdin",
            ])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        put.stdin
            .as_mut()
            .unwrap()
            .write_all("# 笔记\n\n正文 缓存".as_bytes())
            .unwrap();
        let output = put.wait_with_output().unwrap();
        assert_eq!(output.status.code().unwrap(), 0);
        first_revision(&server, "kb/note.md").unwrap()
    };
    let _ = rev_a;

    let output_dir = tempfile::tempdir().unwrap();
    let target = output_dir.path().join("export");
    let output = brain(&server)
        .args(["export", target.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "stderr: {:?}",
        output.stderr
    );

    let note = std::fs::read_to_string(target.join("kb/note.md")).unwrap();
    assert_eq!(note, "# 笔记\n\n正文 缓存");
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(target.join(".brain/export-manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["format"], "brainstore-working-tree");
    assert_eq!(manifest["consistency"], "per_file");
    // The four first-boot template notes plus this test's file.
    assert_eq!(manifest["entries"].as_array().unwrap().len(), 5);

    // Exporting onto an existing directory fails instead of merging.
    let output = brain(&server)
        .args(["export", target.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 2);
}

#[test]
fn whoami_reports_scopes_and_reauth_fails_cleanly() {
    let server = boot_server();
    let output = brain(&server).args(["whoami", "--json"]).output().unwrap();
    assert_eq!(output.status.code().unwrap(), 0);
    let session: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(session["actor"], "token:default");
    assert!(session["scopes"].as_array().unwrap().len() >= 3);

    // A wrong token is exit 4 with a message on stderr only.
    let output = Command::new(cargo_bin("brainstore-cli"))
        .env("BRAIN_URL", &server.url)
        .env("BRAIN_TOKEN", "wrong-token")
        .args(["whoami"])
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 4);
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
