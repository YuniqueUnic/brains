//! Native storage assembly shared by the standalone CLI and HTTP server.
//! No network listener, authentication registry, or background runtime.
use brains_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore, FsStore};
use brains_adapter_sqlite::SqliteIndex;
use brains_application::Vault;
use brains_domain::{CursorSigner, DomainError, DomainResult, Limits, Principal, Scope};
use std::path::Path;

pub type LocalVault = Vault<FsHeadStore, FsRevisionStore, FsBlobStore, SqliteIndex>;

/// Filesystem access grants local ownership; HTTP authentication stays in the server.
pub fn owner() -> Principal {
    Principal {
        actor: "local:owner".into(),
        scopes: vec![Scope::parse("admin:/").expect("constant scope")],
    }
}
pub fn open(base: &Path, vault: &str) -> DomainResult<LocalVault> {
    assemble(FsStore::open(base, vault)?)
}
pub fn create(base: &Path, vault: &str) -> DomainResult<LocalVault> {
    assemble(FsStore::create(base, vault)?)
}
pub fn open_or_create(base: &Path, vault: &str) -> DomainResult<LocalVault> {
    brains_adapter_fs::validate_vault_name(vault)?;
    if base
        .join("vaults")
        .join(vault)
        .join("format.json")
        .try_exists()
        .map_err(storage)?
    {
        open(base, vault)
    } else {
        create(base, vault)
    }
}
fn storage(e: std::io::Error) -> DomainError {
    DomainError::StorageUnavailable(e.to_string())
}
fn assemble(store: FsStore) -> DomainResult<LocalVault> {
    let index_path = store.vault_dir().join("index.db");
    if !index_path.try_exists().map_err(storage)? {
        // Publish a new derived index only after every existing HEAD was indexed.
        // An interrupted build is disposable and is never mistaken for a complete index.
        let building = store.vault_dir().join("index.building.db");
        for suffix in ["", "-wal", "-shm"] {
            let path = std::path::PathBuf::from(format!("{}{suffix}", building.display()));
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(storage(e)),
            }
        }
        {
            let temporary = vault(store.clone(), SqliteIndex::open(&building)?);
            // Native adapters do synchronous FS/SQLite work. This executor drives
            // the existing async application API without a Tokio/network runtime.
            futures::executor::block_on(async {
                let mut cursor = None;
                loop {
                    let page = temporary
                        .reindex_page(
                            &owner(),
                            cursor.as_deref(),
                            temporary.limits.reindex_page_max,
                        )
                        .await?;
                    if !page.failed.is_empty() {
                        return Err(DomainError::IndexUnavailable(format!(
                            "index rebuild failed for {} paths",
                            page.failed.len()
                        )));
                    }
                    cursor = page.next_cursor;
                    if cursor.is_none() {
                        return Ok(());
                    }
                }
            })?;
            temporary.index.close()?;
        }
        // The checkpoint and connection close succeeded before publication.
        std::fs::rename(&building, &index_path).map_err(storage)?;
    }
    let index = SqliteIndex::open(&index_path)?;
    Ok(vault(store, index))
}
fn vault(store: FsStore, index: SqliteIndex) -> LocalVault {
    let name = store.vault().to_owned();
    Vault::new(
        FsHeadStore::new(store.clone()),
        FsRevisionStore::new(store.clone()),
        FsBlobStore::new(store),
        index,
        name,
        Limits::default(),
        CursorSigner::generate(),
    )
}
