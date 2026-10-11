//! Build identity from package and Git metadata, without a build timestamp.

use std::{env, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
    let root = Path::new(&manifest)
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    println!("cargo:rerun-if-env-changed=BRAINS_BUILD_GIT_SHA");
    println!("cargo:rerun-if-env-changed=BRAINS_BUILD_GIT_DIRTY");
    println!("cargo:rerun-if-changed=build.rs");
    // Watch tracked workspace inputs as well as refs/index. Changes in a
    // dependency crate must refresh the CLI's dirty marker too. Untracked
    // files are deliberately excluded from the definition of `dirty`.
    let checkout = root.join(".git").exists();
    if checkout {
        if let Some(files) = git(root, &["ls-files", "-z"]) {
            for file in files.split('\0').filter(|file| !file.is_empty()) {
                println!("cargo:rerun-if-changed={}", root.join(file).display());
            }
        }
        for name in ["HEAD", "index", "packed-refs"] {
            if let Some(path) = git(
                root,
                &["rev-parse", "--path-format=absolute", "--git-path", name],
            ) && Path::new(&path).exists()
            {
                println!("cargo:rerun-if-changed={path}");
            }
        }
        if let Some(reference) = git(root, &["symbolic-ref", "-q", "HEAD"])
            && let Some(path) = git(
                root,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    &reference,
                ],
            )
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let explicit_sha = env::var("BRAINS_BUILD_GIT_SHA").ok();
    if let Some(sha) = &explicit_sha {
        assert!(
            sha.len() >= 7 && sha.len() <= 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "BRAINS_BUILD_GIT_SHA must contain 7..64 hexadecimal characters"
        );
    }
    let sha = explicit_sha.or_else(|| {
        checkout
            .then(|| git(root, &["rev-parse", "HEAD"]))
            .flatten()
    });
    let sha = sha
        .filter(|s| s.len() >= 7 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|s| s[..s.len().min(12)].to_ascii_lowercase())
        .unwrap_or_else(|| "unknown".to_owned());
    let dirty = env::var("BRAINS_BUILD_GIT_DIRTY")
        .ok()
        .map(|value| match value.as_str() {
            "true" => true,
            "false" => false,
            _ => panic!("BRAINS_BUILD_GIT_DIRTY must be true or false"),
        })
        .unwrap_or_else(|| {
            checkout
                && !git(root, &["status", "--porcelain", "--untracked-files=no"])
                    .expect("cannot determine Git working tree status")
                    .is_empty()
        });
    let suffix = if dirty { "-dirty" } else { "" };
    println!(
        "cargo:rustc-env=BRAINS_BUILD_VERSION={} (git {sha}{suffix})",
        env::var("CARGO_PKG_VERSION").expect("package version")
    );
}
